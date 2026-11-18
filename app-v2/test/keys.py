"""向应用窗口发真实键盘事件，验证「只用键盘走完一份」这条验收（T6.14 / 验收 3）。

用法：python test/keys.py <动作串>
动作串用空格分隔：
  n            按 n（打开新建目录）
  t:ceshi      逐字打 ASCII 文本
  enter / esc  回车 / Esc
  ctrl-z       Ctrl+Z
  shot:名字     截一张图到 test/<名字>.bmp
  wait:800     等 800ms
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

hwnd = user32.FindWindowW(None, TITLE)
if not hwnd:
    raise SystemExit('找不到窗口')

user32.ShowWindow(hwnd, 9)
user32.SetForegroundWindow(hwnd)
time.sleep(0.4)

# 点一下窗口内容区再打字：某些情况下 SetForegroundWindow 拿不到真焦点
rect = wt.RECT()
user32.GetWindowRect(hwnd, ctypes.byref(rect))
cx = (rect.left + rect.right) // 2
cy = (rect.top + rect.bottom) // 2 + 120
user32.SetCursorPos(cx, cy)
user32.mouse_event(0x0002, 0, 0, 0, 0)  # LEFTDOWN
user32.mouse_event(0x0004, 0, 0, 0, 0)  # LEFTUP
time.sleep(0.35)

fg = user32.GetForegroundWindow()
print('foreground == window:', fg == hwnd)

INPUT_KEYBOARD = 1
KEYEVENTF_KEYUP = 0x0002
KEYEVENTF_UNICODE = 0x0004
VK = {'enter': 0x0D, 'esc': 0x1B, 'ctrl': 0x11, 'z': 0x5A, 'n': 0x4E}


class KEYBDINPUT(ctypes.Structure):
    _fields_ = [('wVk', wt.WORD), ('wScan', wt.WORD), ('dwFlags', wt.DWORD),
                ('time', wt.DWORD), ('dwExtraInfo', ctypes.POINTER(wt.ULONG))]


class INPUT(ctypes.Structure):
    class _U(ctypes.Union):
        _fields_ = [('ki', KEYBDINPUT)]

    _anonymous_ = ('u',)
    _fields_ = [('type', wt.DWORD), ('u', _U)]


def send(*inputs):
    arr = (INPUT * len(inputs))(*inputs)
    user32.SendInput(len(inputs), arr, ctypes.sizeof(INPUT))


def vk_of(name):
    return VK[name]


def key_down(vk):
    i = INPUT(type=INPUT_KEYBOARD)
    i.ki = KEYBDINPUT(wVk=vk, wScan=0, dwFlags=0, time=0, dwExtraInfo=None)
    send(i)


def key_up(vk):
    i = INPUT(type=INPUT_KEYBOARD)
    i.ki = KEYBDINPUT(wVk=vk, wScan=0, dwFlags=KEYEVENTF_KEYUP, time=0, dwExtraInfo=None)
    send(i)


def tap(vk):
    key_down(vk)
    time.sleep(0.03)
    key_up(vk)


def type_text(text):
    for ch in text:
        down = INPUT(type=INPUT_KEYBOARD)
        down.ki = KEYBDINPUT(wVk=0, wScan=ord(ch), dwFlags=KEYEVENTF_UNICODE, time=0, dwExtraInfo=None)
        up = INPUT(type=INPUT_KEYBOARD)
        up.ki = KEYBDINPUT(wVk=0, wScan=ord(ch), dwFlags=KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                           time=0, dwExtraInfo=None)
        send(down, up)
        time.sleep(0.03)


def shot(name):
    subprocess.run([sys.executable, 'test/shot.py', f'test/{name}.bmp', TITLE], check=True)


for token in sys.argv[1:]:
    if token.startswith('t:'):
        type_text(token[2:])
    elif token.startswith('shot:'):
        time.sleep(0.35)
        shot(token[5:])
    elif token.startswith('wait:'):
        time.sleep(int(token[5:]) / 1000)
    elif token == 'ctrl-z':
        key_down(VK['ctrl'])
        tap(VK['z'])
        key_up(VK['ctrl'])
    elif token in VK:
        tap(vk_of(token))
        time.sleep(0.12)
    elif len(token) == 1 and (token.isalpha() or token.isdigit()):
        tap(ord(token.upper()))
        time.sleep(0.12)
    else:
        raise SystemExit(f'不认识的动作：{token}')

print('done')
