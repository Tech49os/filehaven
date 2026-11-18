"""把路径打进 Windows 文件夹选择对话框（rfd 用的 IFileDialog）并确认。

用法：python test/typedir.py "选择要整理的文件夹" "D:\\some\\dir"

踩过的坑：
1. `keybd_event` 连发 down/up 会被这个对话框吃成双字符（`DD: : //ffii...`），
   所以改用 `SendInput` + `KEYEVENTF_UNICODE`，一个字符一对事件，绕开键盘布局。
2. 回车确认后对话框会关闭并把焦点还给宿主窗口，此时**再按一次回车**可能重新打开选择框
   （宿主按钮仍有焦点）。所以确认后只判一次「窗口是否还在」，不再补回车。
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
VK_RETURN = 0x0D
KEYEVENTF_KEYUP = 0x0002
KEYEVENTF_UNICODE = 0x0004
INPUT_KEYBOARD = 1
ULONG_PTR = ctypes.c_void_p


class KEYBDINPUT(ctypes.Structure):
    _fields_ = [
        ('wVk', wt.WORD),
        ('wScan', wt.WORD),
        ('dwFlags', wt.DWORD),
        ('time', wt.DWORD),
        ('dwExtraInfo', ULONG_PTR),
    ]


class _INPUTunion(ctypes.Union):
    # MOUSEINPUT 在 x64 上就是 32 字节，这样 INPUT 合计 40 —— 结构体大小不对 SendInput 会直接返回 0
    _fields_ = [('ki', KEYBDINPUT), ('padding', ctypes.c_byte * 32)]


class INPUT(ctypes.Structure):
    _fields_ = [('type', wt.DWORD), ('u', _INPUTunion)]


def send(ki_list):
    arr = (INPUT * len(ki_list))()
    for i, ki in enumerate(ki_list):
        arr[i].type = INPUT_KEYBOARD
        arr[i].u.ki = ki
    n = user32.SendInput(len(ki_list), arr, ctypes.sizeof(INPUT))
    if n != len(ki_list):
        raise SystemExit(f'SendInput 只送进 {n}/{len(ki_list)}')


def type_text(s):
    events = []
    for ch in s:
        for flags in (KEYEVENTF_UNICODE, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP):
            events.append(KEYBDINPUT(wVk=0, wScan=ord(ch), dwFlags=flags, time=0, dwExtraInfo=None))
    # 分批送，避免单次 SendInput 数组过大
    for i in range(0, len(events), 20):
        send(events[i : i + 20])
        time.sleep(0.02)


def enter():
    down = KEYBDINPUT(wVk=VK_RETURN, wScan=0x1C, dwFlags=0, time=0, dwExtraInfo=None)
    up = KEYBDINPUT(wVk=VK_RETURN, wScan=0x1C, dwFlags=KEYEVENTF_KEYUP, time=0, dwExtraInfo=None)
    send([down, up])


def main():
    title, path = sys.argv[1], sys.argv[2]
    dlg = user32.FindWindowW('#32770', title)
    if not dlg:
        raise SystemExit(f'找不到对话框：{title}')
    user32.SetForegroundWindow(dlg)
    time.sleep(0.6)
    # 先清空输入框里可能已有的内容
    type_text(path)
    time.sleep(0.4)
    enter()
    time.sleep(1.2)
    if user32.FindWindowW('#32770', title):
        # 第一次回车只是「跳进目录」，再回车才点默认按钮
        enter()
        time.sleep(0.8)
    left = user32.FindWindowW('#32770', title)
    print('dialog gone' if not left else f'dialog still open: {hex(left)}')


if __name__ == '__main__':
    main()
