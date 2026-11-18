"""用 Chrome DevTools Protocol 驱动 WebView2 —— 给 e2e 一个「不靠截图」的状态探针。

为什么需要它：这台机器上 `SendInput` / `PostMessage` 的**键盘**事件送不进 WebView2
（鼠标事件能进，键盘进不去；试过 SendInput+UNICODE、keybd_event、WM_CHAR、WM_KEYDOWN、
强制 SetForegroundWindow / SwitchToThisWindow，全部无效）。于是改用 CDP：

- `eval()` 读 DOM 文本 → 断言界面状态，比截图可靠，也不吃 token
- `insert_text()` / `key()` → 真的把字打进输入框
- `click()` 用 DOM 坐标点击（`Runtime.evaluate` 里调 elementFromPoint + dispatchEvent）

前提：应用必须带 `--remote-debugging-port=9222` 启动，即
`export WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222"` 后再 `npm run app:dev`。

纯标准库：手写一份够用的 WebSocket 客户端（握手 + 掩码帧 + 小包）。
"""
import base64
import json
import os
import socket
import struct
import subprocess
import time
import urllib.request

PORT = int(os.environ.get('CDP_PORT', '9222'))


# ---------------------------------------------------------------- WebSocket

class WS:
    def __init__(self, url, timeout=20):
        assert url.startswith('ws://'), url
        rest = url[5:]
        hostport, _, path = rest.partition('/')
        host, _, port = hostport.partition(':')
        self.sock = socket.create_connection((host, int(port or 80)), timeout=timeout)
        self.buf = b''
        key = base64.b64encode(os.urandom(16)).decode()
        req = (
            f'GET /{path} HTTP/1.1\r\nHost: {hostport}\r\n'
            'Upgrade: websocket\r\nConnection: Upgrade\r\n'
            f'Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n'
        )
        self.sock.sendall(req.encode())
        # 读到头部结束
        while b'\r\n\r\n' not in self.buf:
            chunk = self.sock.recv(4096)
            if not chunk:
                raise RuntimeError('握手失败：连接被关闭')
            self.buf += chunk
        head, _, self.buf = self.buf.partition(b'\r\n\r\n')
        if b'101' not in head.split(b'\r\n')[0]:
            raise RuntimeError(f'握手失败：{head[:200]!r}')

    def send_text(self, text):
        data = text.encode()
        header = bytearray([0x81])
        n = len(data)
        if n < 126:
            header.append(0x80 | n)
        elif n < 65536:
            header.append(0x80 | 126)
            header += struct.pack('>H', n)
        else:
            header.append(0x80 | 127)
            header += struct.pack('>Q', n)
        mask = os.urandom(4)
        header += mask
        masked = bytes(b ^ mask[i % 4] for i, b in enumerate(data))
        self.sock.sendall(bytes(header) + masked)

    def _read_exact(self, n):
        while len(self.buf) < n:
            chunk = self.sock.recv(65536)
            if not chunk:
                raise RuntimeError('连接已关闭')
            self.buf += chunk
        out, self.buf = self.buf[:n], self.buf[n:]
        return out

    def recv_text(self):
        while True:
            b0, b1 = self._read_exact(2)
            opcode = b0 & 0x0F
            masked = b1 & 0x80
            n = b1 & 0x7F
            if n == 126:
                n = struct.unpack('>H', self._read_exact(2))[0]
            elif n == 127:
                n = struct.unpack('>Q', self._read_exact(8))[0]
            mask = self._read_exact(4) if masked else b''
            payload = self._read_exact(n)
            if masked:
                payload = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
            if opcode == 0x8:
                raise RuntimeError('服务端关闭连接')
            if opcode in (0x9, 0xA):  # ping/pong
                continue
            if opcode in (0x1, 0x2, 0x0):
                return payload.decode('utf-8', 'replace')

    def close(self):
        try:
            self.sock.close()
        except Exception:
            pass


# ---------------------------------------------------------------- CDP 会话

class Page:
    def __init__(self, url=None):
        self.ws = WS(url or self._pick_url())
        self.id = 0

    @staticmethod
    def _targets():
        with urllib.request.urlopen(f'http://127.0.0.1:{PORT}/json', timeout=5) as r:
            return json.loads(r.read().decode('utf-8', 'replace'))

    @classmethod
    def _pick_url(cls):
        best = None
        for t in cls._targets():
            if t.get('type') == 'page' and t.get('webSocketDebuggerUrl'):
                # dev 模式下页面地址是 127.0.0.1:5273
                if '5273' in t.get('url', '') or best is None:
                    best = t
        if not best:
            raise RuntimeError('没有可用的 page target，确认应用带 --remote-debugging-port=9222 启动')
        return best['webSocketDebuggerUrl']

    def call(self, method, params=None, timeout=25):
        self.id += 1
        mid = self.id
        self.ws.send_text(json.dumps({'id': mid, 'method': method, 'params': params or {}}))
        t0 = time.time()
        while True:
            msg = json.loads(self.ws.recv_text())
            if msg.get('id') == mid:
                if 'error' in msg:
                    raise RuntimeError(f"{method} 失败：{msg['error']}")
                return msg.get('result', {})
            if time.time() - t0 > timeout:
                raise TimeoutError(f'{method} 超时')

    # ---- 便捷方法

    def eval(self, expr, await_promise=False, timeout=25):
        r = self.call(
            'Runtime.evaluate',
            {
                'expression': expr,
                'returnByValue': True,
                'awaitPromise': await_promise,
                'userGesture': True,
            },
            timeout=timeout,
        )
        if r.get('exceptionDetails'):
            raise RuntimeError(f'JS 抛异常：{r["exceptionDetails"].get("text")} {r.get("result")}')
        return r.get('result', {}).get('value')

    def text(self):
        """整页可见文字（含按钮文案），一行一个块。"""
        return self.eval("document.body.innerText") or ''

    def url(self):
        return self.eval('location.href')

    def insert_text(self, s):
        self.call('Input.insertText', {'text': s})

    def key(self, key, code=None, windows_vk=None):
        base = {'key': key, 'code': code or key, 'windowsVirtualKeyCode': windows_vk or 0}
        self.call('Input.dispatchKeyEvent', {**base, 'type': 'keyDown', 'text': ''})
        self.call('Input.dispatchKeyEvent', {**base, 'type': 'keyUp'})

    def press(self, name):
        table = {
            'Enter': ('Enter', 13, '\r'),
            'Escape': ('Escape', 27, ''),
            'Tab': ('Tab', 9, ''),
            'ArrowDown': ('ArrowDown', 40, ''),
            'ArrowUp': ('ArrowUp', 38, ''),
        }
        key, vk, text = table[name]
        common = {'key': key, 'code': key, 'windowsVirtualKeyCode': vk, 'nativeVirtualKeyCode': vk}
        self.call('Input.dispatchKeyEvent', {**common, 'type': 'keyDown', 'text': text})
        self.call('Input.dispatchKeyEvent', {**common, 'type': 'keyUp'})

    def click(self, x, y, css_px=True):
        """按 CSS 像素坐标点击（截图是物理像素，2 倍 DPI 时除以 2）。"""
        if css_px:
            self.call('Input.dispatchMouseEvent', {'type': 'mouseMoved', 'x': x, 'y': y})
            time.sleep(0.1)
            self.call('Input.dispatchMouseEvent', {'type': 'mousePressed', 'x': x, 'y': y,
                                                   'button': 'left', 'clickCount': 1})
            self.call('Input.dispatchMouseEvent', {'type': 'mouseReleased', 'x': x, 'y': y,
                                                   'button': 'left', 'clickCount': 1})

    def click_text(self, label, tag='button', exact=True):
        """按可见文字点击：找到该文字所在元素并点它的中心。

        `exact=False` 时按「包含」匹配 —— 侧边栏条目的 innerText 会带徽标数字
        （如「待迁移\\n5」），精确匹配会找不到。
        """
        js = """
        (() => {
          const want = %s;
          const exact = %s;
          const els = [...document.querySelectorAll('%s, a, [role=button], li, label, input')];
          const txt = e => (e.innerText || e.value || '').trim().replace(/\\s+/g, '');
          const hit = els.find(e => exact ? txt(e) === want : txt(e).includes(want));
          if (!hit) return null;
          hit.scrollIntoView({block: 'center'});
          const r = hit.getBoundingClientRect();
          return [r.left + r.width / 2, r.top + r.height / 2];
        })()
        """ % (json.dumps(label), 'true' if exact else 'false', tag)
        pos = self.eval(js)
        if not pos:
            raise RuntimeError(f'没找到可点的元素：{label}')
        self.click(pos[0], pos[1])
        return pos

    def screenshot(self, path):
        r = self.call('Page.captureScreenshot', {'format': 'png'})
        with open(path, 'wb') as f:
            f.write(base64.b64decode(r['data']))
        return path


def wait_port(timeout=60):
    t0 = time.time()
    while time.time() - t0 < timeout:
        try:
            with urllib.request.urlopen(f'http://127.0.0.1:{PORT}/json/version', timeout=2) as r:
                return json.loads(r.read().decode())
        except Exception:
            time.sleep(1)
    raise TimeoutError('9222 端口没起来')


if __name__ == '__main__':
    info = wait_port()
    print(info.get('Browser'))
    p = Page()
    print('url', p.url())
    print('--- text ---')
    print(p.text()[:600])
