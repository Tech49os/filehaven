"""用 ctypes 抓窗口截图（不依赖第三方库，也不走 Add-Type）。

用法：python test/shot.py <输出bmp路径>
"""
import ctypes
import ctypes.wintypes as wt
import struct
import sys

# 先声明进程 DPI 感知，否则拿到的坐标是虚拟化的、与实际像素对不上
try:
    ctypes.windll.shcore.SetProcessDpiAwareness(2)  # PER_MONITOR_AWARE
except Exception:
    try:
        ctypes.windll.user32.SetProcessDPIAware()
    except Exception:
        pass

user32 = ctypes.windll.user32
gdi32 = ctypes.windll.gdi32

TITLE = sys.argv[2] if len(sys.argv) > 2 else '文件管家'
out = sys.argv[1] if len(sys.argv) > 1 else 'shot.bmp'

hwnd = user32.FindWindowW(None, TITLE)
if not hwnd:
    raise SystemExit(f'找不到窗口：{TITLE}')

rect = wt.RECT()
user32.GetClientRect(hwnd, ctypes.byref(rect))
w, h = rect.right - rect.left, rect.bottom - rect.top
if w <= 0 or h <= 0:
    raise SystemExit('窗口尺寸为 0')

user32.SetForegroundWindow(hwnd)
user32.ShowWindow(hwnd, 9)  # SW_RESTORE
import time as _t
_t.sleep(0.8)

# WebView2 是硬件合成表面，PrintWindow 抓不到，直接从屏幕 DC 抠窗口区域
prect = wt.RECT()
user32.GetWindowRect(hwnd, ctypes.byref(prect))
pt = wt.POINT(prect.left, prect.top)
w, h = prect.right - prect.left, prect.bottom - prect.top
pw, ph = w, h

hdc_win = user32.GetDC(0)
hdc_mem = gdi32.CreateCompatibleDC(hdc_win)
hbm = gdi32.CreateCompatibleBitmap(hdc_win, pw, ph)
gdi32.SelectObject(hdc_mem, hbm)
gdi32.BitBlt(hdc_mem, 0, 0, pw, ph, hdc_win, pt.x, pt.y, 0x00CC0020)
w, h = pw, ph

class BITMAPINFOHEADER(ctypes.Structure):
    _fields_ = [
        ('biSize', wt.DWORD), ('biWidth', wt.LONG), ('biHeight', wt.LONG),
        ('biPlanes', wt.WORD), ('biBitCount', wt.WORD), ('biCompression', wt.DWORD),
        ('biSizeImage', wt.DWORD), ('biXPelsPerMeter', wt.LONG), ('biYPelsPerMeter', wt.LONG),
        ('biClrUsed', wt.DWORD), ('biClrImportant', wt.DWORD),
    ]

bi = BITMAPINFOHEADER()
bi.biSize = ctypes.sizeof(BITMAPINFOHEADER)
bi.biWidth = w
bi.biHeight = -h  # 负值 = 自上而下
bi.biPlanes = 1
bi.biBitCount = 32
bi.biCompression = 0

buf = ctypes.create_string_buffer(w * h * 4)
got = gdi32.GetDIBits(hdc_mem, hbm, 0, h, buf, ctypes.byref(bi), 0)
if not got:
    raise SystemExit('GetDIBits 失败')

# 写 32 位 BMP
row = w * 4
with open(out, 'wb') as f:
    f.write(b'BM')
    f.write(struct.pack('<IHHI', 14 + 40 + row * h, 0, 0, 14 + 40))
    f.write(struct.pack('<IiiHHIIiiII', 40, w, -h, 1, 32, 0, row * h, 2835, 2835, 0, 0))
    f.write(buf.raw[: row * h])

gdi32.DeleteObject(hbm)
gdi32.DeleteDC(hdc_mem)
user32.ReleaseDC(0, hdc_win)
print(f'ok {w}x{h} -> {out}')
