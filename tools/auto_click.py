#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
自动化端到端验证（仅用于开发验证，不参与产品运行）

流程：定位窗口 → 把路径写进剪贴板 → 点击指定坐标 → Ctrl+V → 回车
用途：验证"选择目录 → 扫描"这条真实链路，不涉及任何写/删操作。
"""

import ctypes
import ctypes.wintypes as wt
import sys
import time

user32 = ctypes.windll.user32
kernel32 = ctypes.windll.kernel32

# 64 位下必须显式声明签名，否则句柄被截断成 32 位
kernel32.GlobalAlloc.restype = wt.HGLOBAL
kernel32.GlobalAlloc.argtypes = [wt.UINT, ctypes.c_size_t]
kernel32.GlobalLock.restype = ctypes.c_void_p
kernel32.GlobalLock.argtypes = [wt.HGLOBAL]
kernel32.GlobalUnlock.argtypes = [wt.HGLOBAL]
user32.SetClipboardData.restype = wt.HANDLE
user32.SetClipboardData.argtypes = [wt.UINT, wt.HANDLE]

CF_UNICODETEXT = 13
GMEM_MOVEABLE = 0x0002


def find_window(title):
    hwnd = user32.FindWindowW(None, title)
    if not hwnd:
        raise SystemExit(f"找不到窗口: {title}")
    return hwnd


def to_clipboard(text):
    data = text.encode("utf-16-le") + b"\x00\x00"
    if not user32.OpenClipboard(0):
        raise SystemExit("打不开剪贴板")
    user32.EmptyClipboard()
    h = kernel32.GlobalAlloc(GMEM_MOVEABLE, len(data))
    p = kernel32.GlobalLock(h)
    ctypes.memmove(p, data, len(data))
    kernel32.GlobalUnlock(h)
    user32.SetClipboardData(CF_UNICODETEXT, h)
    user32.CloseClipboard()


def click(x, y):
    user32.SetCursorPos(int(x), int(y))
    time.sleep(0.15)
    user32.mouse_event(0x0002, 0, 0, 0, 0)  # LEFTDOWN
    time.sleep(0.05)
    user32.mouse_event(0x0004, 0, 0, 0, 0)  # LEFTUP


def key(vk, down=True):
    user32.keybd_event(vk, 0, 0 if down else 2, 0)


def ctrl_v():
    key(0x11)
    time.sleep(0.05)
    key(0x56)
    key(0x56, False)
    key(0x11, False)


def enter():
    key(0x0D)
    key(0x0D, False)


def main():
    title = sys.argv[1] if len(sys.argv) > 1 else "文件管家"
    path = sys.argv[2] if len(sys.argv) > 2 else r"C:\Users\kingb\Desktop"
    bx = int(sys.argv[3]) if len(sys.argv) > 3 else 970
    by = int(sys.argv[4]) if len(sys.argv) > 4 else 746

    hwnd = find_window(title)
    user32.ShowWindow(hwnd, 9)
    user32.SetForegroundWindow(hwnd)
    time.sleep(0.8)

    rect = wt.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(rect))
    print(f"窗口位置 {rect.left},{rect.top}  尺寸 {rect.right-rect.left}x{rect.bottom-rect.top}")

    to_clipboard(path)
    print(f"剪贴板已写入: {path}")

    sx, sy = rect.left + bx, rect.top + by
    print(f"点击窗口内坐标 ({bx},{by}) → 屏幕 ({sx},{sy})")
    click(sx, sy)

    time.sleep(1.8)  # 等原生对话框出现
    ctrl_v()
    time.sleep(0.5)
    enter()
    print("已粘贴路径并回车，等待扫描…")
    time.sleep(6.0)


if __name__ == "__main__":
    main()
