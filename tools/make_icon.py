#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""生成应用图标 icons/icon.ico（多尺寸 32 位 DIB），零第三方依赖。"""

import math
import os
import struct

SIZES = [16, 24, 32, 48, 64, 128, 256]
SS = 4  # 每像素 4x4 超采样


def in_round_rect(x, y, x0, y0, x1, y1, r):
    if x < x0 or x > x1 or y < y0 or y > y1:
        return False
    cx = min(max(x, x0 + r), x1 - r)
    cy = min(max(y, y0 + r), y1 - r)
    return (x - cx) ** 2 + (y - cy) ** 2 <= r * r


def render(size):
    """返回 size*size 的 (r,g,b,a) 列表，行优先自上而下"""
    S = float(size)
    px = []
    for j in range(size):
        row = []
        for i in range(size):
            acc_a = 0.0
            acc_r = acc_g = acc_b = 0.0
            for sy in range(SS):
                for sx in range(SS):
                    x = i + (sx + 0.5) / SS
                    y = j + (sy + 0.5) / SS
                    u, v = x / S, y / S

                    # 外层圆角方：留 3% 边距，圆角 22%
                    if not in_round_rect(u, v, 0.03, 0.03, 0.97, 0.97, 0.22):
                        continue

                    # 垂直渐变 Apple 蓝
                    t = (v - 0.03) / 0.94
                    t = min(1.0, max(0.0, t))
                    r0, g0, b0 = 0x2E, 0x9C, 0xFF
                    r1, g1, b1 = 0x00, 0x5A, 0xD6
                    cr = r0 + (r1 - r0) * t
                    cg = g0 + (g1 - g0) * t
                    cb = b0 + (b1 - b0) * t

                    # 白色文件夹图形
                    white = False
                    # 上方标签
                    if in_round_rect(u, v, 0.26, 0.30, 0.50, 0.40, 0.045):
                        white = True
                    # 主体
                    if in_round_rect(u, v, 0.22, 0.37, 0.78, 0.71, 0.055):
                        white = True
                    # 顶部一道浅色分隔（把主体上沿压深一点，形成层次）
                    if white:
                        # 主体上沿 1.5% 高度的深色，模拟开口
                        if 0.37 <= v <= 0.395 and u >= 0.22 and u <= 0.78:
                            white = False
                            cr, cg, cb = cr, cg, cb
                    if white:
                        cr = cg = cb = 0xFF

                    acc_a += 1.0
                    acc_r += cr
                    acc_g += cg
                    acc_b += cb

            n = SS * SS
            if acc_a <= 0:
                row.append((0, 0, 0, 0))
            else:
                row.append((
                    int(round(acc_r / acc_a)),
                    int(round(acc_g / acc_a)),
                    int(round(acc_b / acc_a)),
                    int(round(255 * acc_a / n)),
                ))
        px.append(row)
    return px


def bmp_dib(px, size):
    """打包成 ICO 用的 32 位 DIB（BITMAPINFOHEADER + BGRA 倒序 + AND 掩码）"""
    hdr = struct.pack(
        "<IiiHHIIiiII",
        40, size, size * 2, 1, 32, 0, 0, 0, 0, 0, 0,
    )
    xor = bytearray()
    for j in range(size - 1, -1, -1):  # 自下而上
        for i in range(size):
            r, g, b, a = px[j][i]
            xor += bytes((b, g, r, a))
    # AND 掩码：1 表示透明
    row_bytes = ((size + 31) // 32) * 4
    and_mask = bytearray()
    for j in range(size - 1, -1, -1):
        bits = bytearray(row_bytes)
        for i in range(size):
            if px[j][i][3] == 0:
                bits[i // 8] |= 0x80 >> (i % 8)
        and_mask += bits
    img = hdr + bytes(xor) + bytes(and_mask)
    return img


def main():
    out_dir = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "src-tauri", "icons")
    out_dir = os.path.normpath(out_dir)
    os.makedirs(out_dir, exist_ok=True)

    images = []
    for s in SIZES:
        print(f"  渲染 {s}x{s} …")
        images.append((s, bmp_dib(render(s), s)))

    count = len(images)
    header = struct.pack("<HHH", 0, 1, count)
    offset = 6 + 16 * count
    entries = b""
    data = b""
    for s, img in images:
        w = 0 if s >= 256 else s
        entries += struct.pack("<BBBBHHII", w, w, 0, 0, 1, 32, len(img), offset)
        data += img
        offset += len(img)

    ico = header + entries + data
    path = os.path.join(out_dir, "icon.ico")
    with open(path, "wb") as f:
        f.write(ico)
    print(f"已生成 {path}  ({len(ico):,} 字节，{count} 个尺寸)")


if __name__ == "__main__":
    main()
