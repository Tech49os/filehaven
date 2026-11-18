"""把按键直接 PostMessage 给 WebView2 的子窗口（SendInput 有时打不进 WebView）。

用法：python test/postkeys.py n s enter t:abc ctrl-z
"""
import ctypes
import ctypes.wintypes as wt
import subprocess
import sys
import time

try:
    ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception:
    pass

user32 = ctypes.windll.user32
TITLE = '文件管家'
WM_KEYDOWN, WM_KEYUP, WM_CHAR = 0x0100, 0x0101, 0x0102

hwnd = user32.FindWindowW(None, TITLE)
if not hwnd:
    raise SystemExit('找不到窗口')

# 找最深的子窗口（WebView2 的渲染窗口）
targets = []
def walk(h, depth=0):
    children = []
    buf = ctypes.create_string_buffer(256)
    @ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
    def cb(ch, _):
        children.append(ch)
        return True
    user32.EnumChildWindows(h, cb, 0)
    for ch in children:
        cls = ctypes.create_string_buffer(256)
        user32.GetClassNameW(ch, cls, 256)
        targets.append((depth, ch, cls.value.decode('utf-8', 'ignore')))
        walk(ch, depth + 1)

walk(hwnd)
targets.sort(key=lambda t: -t[0])
if not targets:
    raise SystemExit('没有子窗口')
print('最深子窗口:', targets[0][2])

target = targets[0][1]


def post(vk, char=None, ctrl=False):
    if ctrl:
        user32.PostMessageW(target, WM_KEYDOWN, 0x11, 0)
    user32.PostMessageW(target, WM_KEYDOWN, vk, 0x00010001)
    user32.PostMessageW(target, WM_KEYUP, vk, 0xC0010001)
    if ctrl:
        user32.PostMessageW(target, WM_KEYUP, 0x11, 0xC0010001)
    time.sleep(0.06)


def type_char(ch):
    # 只发 WM_CHAR：再发 WM_KEYDOWN 会被 Chromium 二次插入，变成 nnccee...
    user32.PostMessageW(target, WM_CHAR, ord(ch), 0x00010001)
    time.sleep(0.04)


def shot(name):
    time.sleep(0.3)
    subprocess.run([sys.executable, 'test/shot.py', f'test/{name}.bmp', TITLE], check=True)


for token in sys.argv[1:]:
    if token.startswith('t:'):
        for ch in token[2:]:
            type_char(ch)
    elif token.startswith('shot:'):
        shot(token[5:])
    elif token.startswith('wait:'):
        time.sleep(int(token[5:]) / 1000)
    elif token == 'ctrl-z':
        post(0x5A, None, ctrl=True)
    elif token == 'enter':
        post(0x0D, '\r')
    elif token == 'esc':
        post(0x1B)
    elif token.startswith('click:'):
        x, y = token[6:].split(',')
        user32.SetCursorPos(int(x), int(y))
        time.sleep(0.15)
        user32.mouse_event(0x0002, 0, 0, 0, 0)
        time.sleep(0.05)
        user32.mouse_event(0x0004, 0, 0, 0, 0)
        time.sleep(0.25)
    elif len(token) == 1:
        post(ord(token.upper()), token)
    else:
        raise SystemExit(f'不认识：{token}')

print('done')
