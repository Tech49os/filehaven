"""截窗口 → PNG（纯标准库，BMP 一个就 14MB，PNG 只要几百 KB）。

用法：python test/shotp.py test/e2e/xx.png ["窗口标题"]

和 shot.py 一样：WebView2 是硬件合成表面，PrintWindow 抓不到，
所以从屏幕 DC 抠窗口区域；坐标与 e2e-click / uia.click_client 同一套（客户区物理像素）。
"""
import ctypes
import ctypes.wintypes as wt
import struct
import sys
import time
import zlib

try:
    ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception:
    pass

user32 = ctypes.windll.user32
gdi32 = ctypes.windll.gdi32


class BITMAPINFOHEADER(ctypes.Structure):
    _fields_ = [
        ('biSize', wt.DWORD), ('biWidth', wt.LONG), ('biHeight', wt.LONG),
        ('biPlanes', wt.WORD), ('biBitCount', wt.WORD), ('biCompression', wt.DWORD),
        ('biSizeImage', wt.DWORD), ('biXPelsPerMeter', wt.LONG), ('biYPelsPerMeter', wt.LONG),
        ('biClrUsed', wt.DWORD), ('biClrImportant', wt.DWORD),
    ]


def png_bytes(w, h, bgra):
    """把自上而下的 BGRA 原始像素写成 8 位 RGB PNG。"""
    raw = bytearray()
    stride = w * 4
    for y in range(h):
        row = bgra[y * stride : (y + 1) * stride]
        raw.append(0)  # filter type 0
        # BGRA → RGB
        rgb = bytearray(w * 3)
        rgb[0::3] = row[2::4]
        rgb[1::3] = row[1::4]
        rgb[2::3] = row[0::4]
        raw.extend(rgb)

    def chunk(tag, data):
        return (
            struct.pack('>I', len(data))
            + tag
            + data
            + struct.pack('>I', zlib.crc32(tag + data) & 0xFFFFFFFF)
        )

    ihdr = struct.pack('>IIBBBBB', w, h, 8, 2, 0, 0, 0)
    return (
        b'\x89PNG\r\n\x1a\n'
        + chunk(b'IHDR', ihdr)
        + chunk(b'IDAT', zlib.compress(bytes(raw), 6))
        + chunk(b'IEND', b'')
    )


def capture(title):
    hwnd = user32.FindWindowW(None, title)
    if not hwnd:
        raise SystemExit(f'找不到窗口：{title}')
    user32.SetForegroundWindow(hwnd)
    user32.ShowWindow(hwnd, 9)
    time.sleep(0.6)

    rect = wt.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(rect))
    x, y = rect.left, rect.top
    w, h = rect.right - rect.left, rect.bottom - rect.top
    if w <= 0 or h <= 0:
        raise SystemExit('窗口尺寸为 0')

    hdc_win = user32.GetDC(0)
    hdc_mem = gdi32.CreateCompatibleDC(hdc_win)
    hbm = gdi32.CreateCompatibleBitmap(hdc_win, w, h)
    gdi32.SelectObject(hdc_mem, hbm)
    gdi32.BitBlt(hdc_mem, 0, 0, w, h, hdc_win, x, y, 0x00CC0020)

    bi = BITMAPINFOHEADER()
    bi.biSize = ctypes.sizeof(BITMAPINFOHEADER)
    bi.biWidth = w
    bi.biHeight = -h
    bi.biPlanes = 1
    bi.biBitCount = 32
    bi.biCompression = 0
    buf = ctypes.create_string_buffer(w * h * 4)
    if not gdi32.GetDIBits(hdc_mem, hbm, 0, h, buf, ctypes.byref(bi), 0):
        raise SystemExit('GetDIBits 失败')

    data = buf.raw[: w * h * 4]
    gdi32.DeleteObject(hbm)
    gdi32.DeleteDC(hdc_mem)
    user32.ReleaseDC(0, hdc_win)
    return w, h, data


def main():
    out = sys.argv[1] if len(sys.argv) > 1 else 'shot.png'
    title = sys.argv[2] if len(sys.argv) > 2 else '文件管家'
    w, h, data = capture(title)
    with open(out, 'wb') as f:
        f.write(png_bytes(w, h, data))
    print(f'ok {w}x{h} -> {out}')


if __name__ == '__main__':
    main()
