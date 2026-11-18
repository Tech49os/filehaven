"""按「客户区坐标」点击 —— shot.py 截的是客户区物理像素，这里的坐标同一套，省得手算窗口偏移。

用法：python test/e2e-click.py 146,275 [146,300 ...]
"""
import ctypes
import ctypes.wintypes as wt
import sys
import time

try:
    ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception:
    pass

user32 = ctypes.windll.user32
TITLE = '文件管家'

hwnd = user32.FindWindowW(None, TITLE)
if not hwnd:
    raise SystemExit('找不到窗口')

origin = wt.POINT(0, 0)
user32.ClientToScreen(hwnd, ctypes.byref(origin))

for token in sys.argv[1:]:
    x, y = (int(v) for v in token.split(','))
    user32.SetCursorPos(origin.x + x, origin.y + y)
    time.sleep(0.18)
    user32.mouse_event(0x0002, 0, 0, 0, 0)
    time.sleep(0.05)
    user32.mouse_event(0x0004, 0, 0, 0, 0)
    time.sleep(0.45)
print('clicked', origin.x, origin.y)
