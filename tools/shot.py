#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""按窗口标题截屏（纯 ctypes + GDI），输出 32 位 BMP。"""

import ctypes
import ctypes.wintypes as wt
import struct
import sys

user32 = ctypes.windll.user32
gdi32 = ctypes.windll.gdi32

SRCCOPY = 0x00CC0020
DIB_RGB_COLORS = 0


class BITMAPINFOHEADER(ctypes.Structure):
    _fields_ = [
        ("biSize", wt.DWORD),
        ("biWidth", wt.LONG),
        ("biHeight", wt.LONG),
        ("biPlanes", wt.WORD),
        ("biBitCount", wt.WORD),
        ("biCompression", wt.DWORD),
        ("biSizeImage", wt.DWORD),
        ("biXPelsPerMeter", wt.LONG),
        ("biYPelsPerMeter", wt.LONG),
        ("biClrUsed", wt.DWORD),
        ("biClrImportant", wt.DWORD),
    ]


class BITMAPINFO(ctypes.Structure):
    _fields_ = [("bmiHeader", BITMAPINFOHEADER), ("bmiColors", wt.DWORD * 3)]


def main():
    title = sys.argv[1] if len(sys.argv) > 1 else "文件管家"
    out = sys.argv[2] if len(sys.argv) > 2 else "shot.bmp"

    hwnd = user32.FindWindowW(None, title)
    if not hwnd:
        print(f"找不到窗口: {title}")
        return 1

    user32.ShowWindow(hwnd, 9)  # SW_RESTORE
    user32.SetForegroundWindow(hwnd)
    ctypes.windll.kernel32.Sleep(900)

    rect = wt.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(rect))
    w = rect.right - rect.left
    h = rect.bottom - rect.top
    if w <= 0 or h <= 0:
        print("窗口尺寸异常")
        return 1

    screen = user32.GetDC(0)
    mem = gdi32.CreateCompatibleDC(screen)
    bmp = gdi32.CreateCompatibleBitmap(screen, w, h)
    gdi32.SelectObject(mem, bmp)
    ok = gdi32.BitBlt(mem, 0, 0, w, h, screen, rect.left, rect.top, SRCCOPY)
    if not ok:
        print("BitBlt 失败")
        return 1

    bi = BITMAPINFO()
    bi.bmiHeader.biSize = ctypes.sizeof(BITMAPINFOHEADER)
    bi.bmiHeader.biWidth = w
    bi.bmiHeader.biHeight = -h  # 负值 = 自上而下
    bi.bmiHeader.biPlanes = 1
    bi.bmiHeader.biBitCount = 32
    bi.bmiHeader.biCompression = 0
    bi.bmiHeader.biSizeImage = w * h * 4

    buf = ctypes.create_string_buffer(w * h * 4)
    lines = gdi32.GetDIBits(mem, bmp, 0, h, buf, ctypes.byref(bi), DIB_RGB_COLORS)
    if lines == 0:
        print("GetDIBits 失败")
        return 1

    # BMP 文件头（32bpp 无需调色板）
    data_size = w * h * 4
    file_hdr = struct.pack("<2sIHHI", b"BM", 14 + 40 + data_size, 0, 0, 14 + 40)
    info_hdr = struct.pack(
        "<IiiHHIIiiII", 40, w, -h, 1, 32, 0, data_size, 0, 0, 0, 0
    )
    with open(out, "wb") as f:
        f.write(file_hdr + info_hdr + buf.raw)

    gdi32.DeleteObject(bmp)
    gdi32.DeleteDC(mem)
    user32.ReleaseDC(0, screen)
    print(f"已保存 {out}  ({w}x{h})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
