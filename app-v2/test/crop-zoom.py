"""裁剪 PNG 并放大（最近邻），纯标准库。用于把截图局部放大看清细节。

用法：python crop-zoom.py <in.png> <out.png> <x> <y> <w> <h> <scale>
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
    return w, h, ch, out


def main():
    src, dst, x0, y0, bw, bh, scale = sys.argv[1:8]
    x0, y0, bw, bh, scale = int(x0), int(y0), int(bw), int(bh), int(scale)
    w, h, ch, buf = decode(src)
    ow, oh = bw * scale, bh * scale
    rows = []
    for yy in range(oh):
        sy = y0 + yy // scale
        row = bytearray()
        for xx in range(ow):
            sx = x0 + xx // scale
            sx = max(0, min(w - 1, sx)); syc = max(0, min(h - 1, sy))
            i = (syc * w + sx) * ch
            row += bytes(buf[i:i + 3])
        rows.append(row)
    raw = b''.join(b'\x00' + r for r in rows)

    def chunk(typ, payload):
        c = struct.pack('>I', len(payload)) + typ + payload
        return c + struct.pack('>I', zlib.crc32(typ + payload) & 0xFFFFFFFF)

    png = (b'\x89PNG\r\n\x1a\n'
           + chunk(b'IHDR', struct.pack('>IIBBBBB', ow, oh, 8, 2, 0, 0, 0))
           + chunk(b'IDAT', zlib.compress(raw, 6))
           + chunk(b'IEND', b''))
    open(dst, 'wb').write(png)
    print(f'ok {w}x{h} -> crop({x0},{y0},{bw}x{bh}) x{scale} = {ow}x{oh} -> {dst}')


if __name__ == '__main__':
    main()
