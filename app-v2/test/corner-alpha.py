"""窗口四角「圆外是否被着色」探针 v2（按 alpha 通道判定，不受圆角大小影响）。

原理：窗口圆角之外必须是全透明（alpha=0），否则就是溢出。
窗口本身是 transparent:true 的 Tauri 窗口，桌面由 DWM 合成，
所以 CDP 截图的圆角外区域应当 alpha=0 —— 任何 a>8 的像素都是「多画出来的东西」。

用法：
  python corner-alpha.py                       # 量当前状态
  python corner-alpha.py --css /tmp/x.css      # 注入 CSS 后量（对比修复前后）
"""
import base64
import json
import struct
import sys
import time
import zlib

from cdp import Page

SCALE = 4
BOX = 30          # 角部取样边长（CSS px）
ALPHA_MIN = 8     # alpha 阈值


def decode_png(data):
    assert data[:8] == b'\x89PNG\r\n\x1a\n'
    pos, idat = 8, b''
    w = h = ct = None
    while pos < len(data):
        ln = struct.unpack('>I', data[pos:pos + 4])[0]
        typ = data[pos + 4:pos + 8]
        chunk = data[pos + 8:pos + 8 + ln]
        if typ == b'IHDR':
            w, h, _bd, ct = struct.unpack('>IIBB', chunk[:10])
        elif typ == b'IDAT':
            idat += chunk
        pos += 12 + ln
    raw = zlib.decompress(idat)
    ch = {0: 1, 2: 3, 4: 2, 6: 4}[ct]
    stride = w * ch
    out = bytearray(w * h * ch)
    prev = bytearray(stride)
    p = 0
    for _ in range(h):
        f = raw[p]; p += 1
        line = bytearray(raw[p:p + stride]); p += stride
        if f == 1:
            for i in range(ch, stride):
                line[i] = (line[i] + line[i - ch]) & 0xFF
        elif f == 2:
            for i in range(stride):
                line[i] = (line[i] + prev[i]) & 0xFF
        elif f == 3:
            for i in range(stride):
                a = line[i - ch] if i >= ch else 0
                line[i] = (line[i] + ((a + prev[i]) >> 1)) & 0xFF
        elif f == 4:
            for i in range(stride):
                a = line[i - ch] if i >= ch else 0
                b = prev[i]; c = prev[i - ch] if i >= ch else 0
                pa, pb, pc = abs(b - c), abs(a - c), abs(a + b - 2 * c)
                pr = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                line[i] = (line[i] + pr) & 0xFF
        out[_ * stride:(_ + 1) * stride] = line
        prev = line
    return w, h, ch, out


def probe(page, name, x, y, radius):
    r = page.call('Page.captureScreenshot', {
        'format': 'png',
        'clip': {'x': x, 'y': y, 'width': BOX, 'height': BOX, 'scale': SCALE},
    }, timeout=30)
    w, h, ch, buf = decode_png(base64.b64decode(r['data']))
    if ch != 4:
        print(f'  {name}: 截图无 alpha 通道（ch={ch}），无法判定')
        return -1, None
    rr = radius * SCALE
    tol = 1.5 * SCALE
    cx = rr if x == 0 else w - rr
    cy = rr if y == 0 else h - rr
    bad = []
    for yy in range(h):
        for xx in range(w):
            dx, dy = xx - cx, yy - cy
            # 只看圆弧外的区域（含 1.5px 抗锯齿容差）
            if dx * dx + dy * dy <= (rr + tol) ** 2 and not (dx < 0 and dy < 0):
                continue
            if dx > 0 and dy > 0:
                continue  # 这个象限在圆心指向窗口内部侧，不属于圆角外
            i = (yy * w + xx) * ch
            a = buf[i + 3]
            if a > ALPHA_MIN:
                bad.append((round(xx / SCALE, 1), round(yy / SCALE, 1), a,
                            buf[i], buf[i + 1], buf[i + 2]))
    bad.sort(key=lambda t: -t[2])
    print(f'  {name:4s} 圆外不透明像素 {len(bad):5d}')
    for t in bad[:3]:
        print(f'        css({t[0]},{t[1]}) a={t[2]} rgb={t[3]},{t[4]},{t[5]}')
    return len(bad), (bad[0] if bad else None)


def main():
    css_file = None
    if '--css' in sys.argv:
        css_file = sys.argv[sys.argv.index('--css') + 1]

    page = Page()
    page.eval("(() => { const s=document.getElementById('__probe_css'); if(s) s.remove(); return 1; })()")
    if css_file:
        css = open(css_file, encoding='utf-8').read()
        page.eval('''(() => {
          const s = document.createElement('style');
          s.id = '__probe_css';
          s.textContent = %s;
          document.head.appendChild(s);
          return true;
        })()''' % json.dumps(css))
        time.sleep(0.8)

    geo = page.eval('''(() => {
      const win = document.querySelector('.win');
      const r = win.getBoundingClientRect();
      const cs = getComputedStyle(win);
      return {
        w: Math.round(r.width), h: Math.round(r.height),
        left: Math.round(r.left), top: Math.round(r.top),
        radius: parseFloat(cs.borderTopLeftRadius) || 0,
        noGlass: document.documentElement.classList.contains('no-glass'),
        cssInjected: !!document.getElementById('__probe_css'),
      };
    })()''')
    print('几何:', json.dumps(geo, ensure_ascii=False))

    R = geo['radius']
    res = []
    res.append(probe(page, 'TL', geo['left'], geo['top'], R))
    res.append(probe(page, 'TR', geo['left'] + geo['w'] - BOX, geo['top'], R))
    res.append(probe(page, 'BL', geo['left'], geo['top'] + geo['h'] - BOX, R))
    res.append(probe(page, 'BR', geo['left'] + geo['w'] - BOX, geo['top'] + geo['h'] - BOX, R))

    if any(n < 0 for n, _ in res):
        print('\n无法判定（截图不含 alpha）')
        return
    total = sum(n for n, _ in res)
    print(f'\n合计圆外不透明像素: {total}')
    print('判定:', 'PASS 圆角干净（圆外全透明）' if total == 0 else 'FAIL 圆角外有溢出')


if __name__ == '__main__':
    main()
