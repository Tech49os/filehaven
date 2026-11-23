"""窗口四角探针：量「圆角外面有没有被画上东西」。

为什么需要：Chromium/WebView2 里，带 backdrop-filter 的子元素不会被祖先的
border-radius + overflow:hidden 裁剪，矩形采样层会从圆角外溢出一小块方角
（BUG-011 的玻璃框架踩过）。肉眼在浅色渐变上很难分辨，必须按像素判定。

用法：
  python corner-probe.py            # 只量当前状态
  python corner-probe.py --css x.css  # 先注入额外 CSS 再量（比较修复前后）
"""
import base64
import json
import struct
import sys
import time
import zlib

from cdp import Page

SCALE = 6


def decode_png(data):
    assert data[:8] == b'\x89PNG\r\n\x1a\n', 'not png'
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
    for y in range(h):
        f = raw[p]
        p += 1
        line = bytearray(raw[p:p + stride])
        p += stride
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
                b = prev[i]
                c = prev[i - ch] if i >= ch else 0
                pa, pb, pc = abs(b - c), abs(a - c), abs(a + b - 2 * c)
                pr = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                line[i] = (line[i] + pr) & 0xFF
        out[y * stride:(y + 1) * stride] = line
        prev = line
    return w, h, ch, out


def probe(page, name, clip_x, clip_y, w_css, h_css, radius):
    """在给定角区域内统计「圆弧之外但仍被画上」的像素。"""
    r = page.call('Page.captureScreenshot', {
        'format': 'png',
        'clip': {'x': clip_x, 'y': clip_y, 'width': w_css, 'height': h_css, 'scale': SCALE},
        'captureBeyondViewport': True,
    }, timeout=30)
    data = base64.b64decode(r['data'])
    w, h, ch, buf = decode_png(data)
    rr = radius * SCALE
    # 圆弧边缘 1px 抗锯齿是正常渲染，不算溢出；只统计离圆弧 >= 2 图上像素的着色点
    tol = 2
    worst = []
    for y in range(h):
        for x in range(w):
            # 该角圆弧的圆心
            if clip_x == 0:
                cx = rr
            else:
                cx = w - rr
            if clip_y == 0:
                cy = rr
            else:
                cy = h - rr
            dx, dy = x - cx, y - cy
            if dx * dx + dy * dy <= (rr + tol) * (rr + tol):
                continue  # 圆内或贴边抗锯齿，正常
            i = (y * w + x) * ch
            a = buf[i + 3] if ch == 4 else 255
            if a > 10:
                worst.append((round(x / SCALE, 1), round(y / SCALE, 1), a,
                              buf[i], buf[i + 1], buf[i + 2]))
    worst.sort(key=lambda t: -t[2])
    print(f'  {name:4s} 圆外着色像素 {len(worst):4d}')
    for t in worst[:3]:
        print(f'        css({t[0]},{t[1]}) a={t[2]} rgb={t[3]},{t[4]},{t[5]}')
    return len(worst), (worst[0] if worst else None)


def main():
    css_file = None
    if '--css' in sys.argv:
        css_file = sys.argv[sys.argv.index('--css') + 1]

    page = Page()
    if css_file:
        css = open(css_file, encoding='utf-8').read()
        page.eval('''(() => {
          const s = document.getElementById('__probe_css') || document.createElement('style');
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
      const rail = document.querySelector('.side-rail');
      const tb = document.querySelector('.titlebar');
      return {
        w: r.width, h: r.height,
        left: r.left, top: r.top,
        radius: parseFloat(cs.borderTopLeftRadius) || 0,
        border: cs.borderTopWidth,
        railBlur: rail && getComputedStyle(rail).backdropFilter,
        railRadius: rail && getComputedStyle(rail).borderTopLeftRadius,
        tbBlur: tb && getComputedStyle(tb).backdropFilter,
        tbRadius: tb && getComputedStyle(tb).borderTopRightRadius,
        clipPath: cs.clipPath,
        noGlass: document.documentElement.classList.contains('no-glass'),
      };
    })()''')
    print('几何:', json.dumps(geo, ensure_ascii=False))
    print(f'圆角 {geo["radius"]}px，窗口 {int(geo["w"])}×{int(geo["h"])}，no-glass={geo["noGlass"]}')

    R = geo['radius']
    box = 26  # 角部取样边长（CSS px），要 > 圆角
    res = []
    res.append(probe(page, 'TL', geo['left'], geo['top'], box, box, R))
    res.append(probe(page, 'TR', geo['left'] + geo['w'] - box, geo['top'], box, box, R))
    res.append(probe(page, 'BL', geo['left'], geo['top'] + geo['h'] - box, box, box, R))
    res.append(probe(page, 'BR', geo['left'] + geo['w'] - box, geo['top'] + geo['h'] - box, box, box, R))

    total = sum(n for n, _ in res)
    print(f'\n合计圆外着色像素: {total}')
    print('判定:', 'PASS 圆角干净' if total == 0 else 'FAIL 圆角外溢出')


if __name__ == '__main__':
    main()
