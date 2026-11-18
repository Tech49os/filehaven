"""e2e 小工具集合（纯 ctypes，零第三方依赖）。

为什么单独一个文件：M8 复测里要反复「点到按钮 → 在原生对话框里填路径 → 回车」，
之前散在几个一次性脚本里，每次都重写一遍 SendInput 的结构体。

要点（都是踩过的坑）：
- `SendInput` 的 INPUT 结构体在 x64 上必须是 40 字节（union 按 MOUSEINPUT 的 32 字节对齐），
  大小不对会直接返回 0，且不会报错。
- `keybd_event` 连发 down/up 会被 IFileDialog 吃成双字符，所以打字一律走
  `SendInput` + `KEYEVENTF_UNICODE`。
- 写剪贴板要给 `GlobalAlloc` 声明 `restype=c_void_p`，否则 64 位下句柄被截断成 0，
  后面 `memmove` 直接 access violation。

用法：
    from uia import click_client, set_clipboard, paste, enter, type_unicode
"""
import ctypes
import ctypes.wintypes as wt
import time

try:
    ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception:
    pass

user32 = ctypes.windll.user32
kernel32 = ctypes.windll.kernel32

KEYEVENTF_KEYUP = 0x0002
KEYEVENTF_UNICODE = 0x0004
INPUT_KEYBOARD = 1
VK_CTRL, VK_RETURN, VK_ESC, VK_A, VK_V = 0x11, 0x0D, 0x1B, 0x41, 0x56


class _KI(ctypes.Structure):
    _fields_ = [
        ('wVk', wt.WORD),
        ('wScan', wt.WORD),
        ('dwFlags', wt.DWORD),
        ('time', wt.DWORD),
        ('dwExtraInfo', ctypes.c_void_p),
    ]


class _INPUTunion(ctypes.Union):
    _fields_ = [('ki', _KI), ('pad', ctypes.c_byte * 32)]


class _INPUT(ctypes.Structure):
    _fields_ = [('type', wt.DWORD), ('u', _INPUTunion)]


assert ctypes.sizeof(_INPUT) == 40, 'INPUT 结构体大小不对，SendInput 会静默失败'


def _send(pairs):
    arr = (_INPUT * len(pairs))()
    for i, (vk, scan, flags) in enumerate(pairs):
        arr[i].type = INPUT_KEYBOARD
        arr[i].u.ki = _KI(wVk=vk, wScan=scan, dwFlags=flags, time=0, dwExtraInfo=None)
    n = user32.SendInput(len(pairs), arr, ctypes.sizeof(_INPUT))
    if n != len(pairs):
        raise RuntimeError(f'SendInput 只送进 {n}/{len(pairs)}')


def type_unicode(text, delay=0.02):
    """按字符发 KEYEVENTF_UNICODE，绕开键盘布局与 IME。"""
    buf = []
    for ch in text:
        buf.append((0, ord(ch), KEYEVENTF_UNICODE))
        buf.append((0, ord(ch), KEYEVENTF_UNICODE | KEYEVENTF_KEYUP))
    for i in range(0, len(buf), 20):
        _send(buf[i : i + 20])
        time.sleep(delay)


def enter():
    _send([(VK_RETURN, 0x1C, 0), (VK_RETURN, 0x1C, KEYEVENTF_KEYUP)])
    time.sleep(0.1)


def esc():
    _send([(VK_ESC, 0x01, 0), (VK_ESC, 0x01, KEYEVENTF_KEYUP)])
    time.sleep(0.1)


def combo(vk, mod=VK_CTRL):
    _send([(mod, 0, 0)])
    time.sleep(0.06)
    _send([(vk, 0, 0)])
    time.sleep(0.04)
    _send([(vk, 0, KEYEVENTF_KEYUP)])
    _send([(mod, 0, KEYEVENTF_KEYUP)])
    time.sleep(0.25)


def set_clipboard(text):
    """写 CF_UNICODETEXT 剪贴板。"""
    kernel32.GlobalAlloc.restype = ctypes.c_void_p
    kernel32.GlobalAlloc.argtypes = [wt.UINT, ctypes.c_size_t]
    kernel32.GlobalLock.restype = ctypes.c_void_p
    kernel32.GlobalLock.argtypes = [ctypes.c_void_p]
    kernel32.GlobalUnlock.argtypes = [ctypes.c_void_p]
    user32.SetClipboardData.restype = ctypes.c_void_p
    user32.SetClipboardData.argtypes = [wt.UINT, ctypes.c_void_p]

    data = (text + '\0').encode('utf-16-le')
    GMEM_MOVEABLE = 0x0002
    for _ in range(10):
        if user32.OpenClipboard(None):
            break
        time.sleep(0.05)
    else:
        raise RuntimeError('剪贴板打不开')
    user32.EmptyClipboard()
    h = kernel32.GlobalAlloc(GMEM_MOVEABLE, len(data))
    p = kernel32.GlobalLock(h)
    ctypes.memmove(p, data, len(data))
    kernel32.GlobalUnlock(h)
    user32.SetClipboardData(13, h)
    user32.CloseClipboard()


def paste():
    combo(VK_V)


def select_all():
    combo(VK_A)


def find_window(title, klass=None):
    return user32.FindWindowW(klass, title)


def client_origin(hwnd):
    p = wt.POINT(0, 0)
    user32.ClientToScreen(hwnd, ctypes.byref(p))
    return p.x, p.y


def click_client(hwnd, x, y, settle=0.45):
    """按客户区物理像素点击（与 shot.py 截图的坐标系一致）。"""
    ox, oy = client_origin(hwnd)
    user32.SetForegroundWindow(hwnd)
    time.sleep(0.15)
    user32.SetCursorPos(ox + x, oy + y)
    time.sleep(0.2)
    user32.mouse_event(0x0001, 0, 0, 0, 0)  # move：Chromium 要先有 hover 才更新命中测试
    time.sleep(0.12)
    user32.mouse_event(0x0002, 0, 0, 0, 0)
    time.sleep(0.06)
    user32.mouse_event(0x0004, 0, 0, 0, 0)
    time.sleep(settle)


def click_screen(x, y, settle=0.45):
    user32.SetCursorPos(x, y)
    time.sleep(0.2)
    user32.mouse_event(0x0001, 0, 0, 0, 0)
    time.sleep(0.12)
    user32.mouse_event(0x0002, 0, 0, 0, 0)
    time.sleep(0.06)
    user32.mouse_event(0x0004, 0, 0, 0, 0)
    time.sleep(settle)


def poke_dialog(title, path, klass='#32770'):
    """往文件夹选择对话框里塞路径：粘贴 → 回车（跳进目录）→ 回车（默认按钮）。"""
    dlg = find_window(title, klass)
    if not dlg:
        raise RuntimeError(f'找不到对话框：{title}')
    user32.SetForegroundWindow(dlg)
    time.sleep(0.5)
    set_clipboard(path)
    select_all()
    paste()
    time.sleep(0.4)
    enter()
    time.sleep(1.0)
    if find_window(title, klass):
        enter()
        time.sleep(0.8)
    return find_window(title, klass)
