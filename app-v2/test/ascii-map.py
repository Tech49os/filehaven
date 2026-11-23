"""把 PNG 局部区域打成字符图（亮度分档），用于肉眼判断形状。

用法：python ascii-map.py <in.png> <x> <y> <w> <h> [step]
字符：' ' 极亮  '.' 亮  ':' 中  '*' 暗  '#' 很深  '@' 黑
"""
import struct
import sys
import zlib


def decode(path):
    data = open(path, 'rb').read()
    assert data[:8] == b'\x89PNG\r\n\x1a\n'
    pos, idat = 8, b''
    while pos < len(data):
        ln = struct.unpack('>I', data[pos:pos + 4])[0]
        typ = data[pos + 4:pos + 8]
        chunk = data[pos + 8:pos + 8 + ln]
        if typ == b'IHDR':
            w, h, bd, ct = struct.unpack('>IIBB', chunk[:10])
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


RAMP = ' .:-=+*#%@'


def main():
    src, x0, y0, bw, bh = sys.argv[1:6]
    step = int(sys.argv[6]) if len(sys.argv) > 6 else 1
    x0, y0, bw, bh = int(x0), int(y0), int(bw), int(bh)
    w, h, ch, buf = decode(src)
    print(f'# {src} crop({x0},{y0}) {bw}x{bh} step={step}')
    print('   ' + ''.join(str((x0 + i) // 100 % 10) for i in range(0, bw, step)))
    print('   ' + ''.join(str((x0 + i) // 10 % 10) for i in range(0, bw, step)))
    print('   ' + ''.join(str((x0 + i) % 10) for i in range(0, bw, step)))
    for yy in range(y0, min(y0 + bh, h), step):
        row = []
        for xx in range(x0, min(x0 + bw, w), step):
            i = (yy * w + xx) * ch
            r, g, b = buf[i], buf[i + 1], buf[i + 2]
            lum = 0.299 * r + 0.587 * g + 0.114 * b
            # 越亮 → 字符越靠前（空格表示极亮）
            idx = min(len(RAMP) - 1, int((255 - lum) / 255 * (len(RAMP) - 1) + 0.5))
            row.append(RAMP[idx])
        print(f'{yy:3d}' + ''.join(row))


if __name__ == '__main__':
    main()
