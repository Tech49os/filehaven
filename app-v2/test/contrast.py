#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""文字对比度校验（P2 T5.1）

P1 的 --c-text-3 是 #aeaeb2，在 #f5f5f7 上只有 2.03:1 —— 11~12px 的提示字
（文件大小、路径、推荐理由、状态文字）在真实屏幕上基本看不清。这条一直挂在
P1 的已知缺陷里，P2 收尾时按 WCAG AA（小字 4.5:1）重定。

为什么不走 CDP 量 getComputedStyle：
  1) 对比度是纯色值计算，量一次就等于算一次，起窗口反而引入变量；
  2) 脚本要在 CI/提交前都能跑，不能依赖 WebView2 起来；
  3) 真实渲染的颜色就是 tokens.css 里那几个变量（组件禁止硬编码色值）。
于是直接解析 tokens.css，对三个实际用到的底色各算一遍。

用法：python test/contrast.py [--min 4.5]
退出码：0 = 全过；1 = 有项不达标（可直接接进 CI）
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

TOKENS = Path(__file__).resolve().parent.parent / "src" / "styles" / "tokens.css"

# 文字实际会落在哪些底色上（都是不透明底色，见 tokens.css 的注释）
BACKDROPS = {
    "bg": "#f5f5f7",
    "sidebar": "#f2f2f7",
    "white": "#ffffff",
}

# 必须达标的前景色。基准取最差的那个底色（窗口背景/侧栏都比白底深）
CHECKED = ["--c-text", "--c-text-2", "--c-text-3"]

# P3 新页面的专项组合（T5.3）：文字色 × 它实际会落的底色，逐对算。
# indigo 是标签 chip（topic）的字色，落 --c-indigo-bg 上；
SPECIAL_PAIRS = [
    ("--c-indigo(topic chip 字)", "--c-indigo", "--c-indigo-bg"),
]

# 已知例外（只报告不拦截）：主按钮白字落在 #007aff（iOS 蓝，设计文档定死的品牌色）上是 4.02:1。
# 白字已是该底色的对比度上限，要过 4.5 只能加深品牌色——那是全项目 7 处一致的既有视觉决策，
# 不在 P3 范围内动。按 WCAG 大字标准（粗体 14pt ≈ 18.7px）主按钮文字也未达，如实记录。
KNOWN_EXCEPTIONS = [
    ("primary 按钮白字(已知例外)", "#ffffff", "--c-primary"),
]


def expand(name: str, css: str) -> str:
    """token 名解析成不透明色；字面 hex 原样返回。rgba 叠白底换算。"""
    if name.startswith("#"):
        return name
    m = re.search(rf"{re.escape(name)}\s*:\s*rgba?\(([^)]+)\)", css)
    if not m:
        return hex_of(name, css)
    parts = [p.strip() for p in m.group(1).split(",")]
    r, g, b = (int(float(p)) for p in parts[:3])
    a = float(parts[3])
    # 叠白底：c' = c*a + 255*(1-a)
    bg = 255
    mix = lambda c: round(c * a + bg * (1 - a))  # noqa: E731
    return f"#{mix(r):02x}{mix(g):02x}{mix(b):02x}"


def hex_of(name: str, css: str) -> str:
    m = re.search(rf"{re.escape(name)}\s*:\s*(#[0-9a-fA-F]{{6}})", css)
    if not m:
        raise SystemExit(f"tokens.css 里找不到 {name}（硬编码色值或改名了？）")
    return m.group(1)


def luminance(hex_color: str) -> float:
    h = hex_color.lstrip("#")
    channels = [int(h[i : i + 2], 16) / 255 for i in (0, 2, 4)]

    def lin(c: float) -> float:
        return c / 12.92 if c <= 0.03928 else ((c + 0.055) / 1.055) ** 2.4

    r, g, b = (lin(c) for c in channels)
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


def ratio(fg: str, bg: str) -> float:
    a, b = luminance(fg), luminance(bg)
    hi, lo = max(a, b), min(a, b)
    return (hi + 0.05) / (lo + 0.05)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--min", type=float, default=4.5, help="AA 小字门槛，默认 4.5")
    args = ap.parse_args()

    css = TOKENS.read_text(encoding="utf-8")
    bad = []
    print(f"{'token':<12}{'color':<10}" + "".join(f"{k:>9}" for k in BACKDROPS))
    for name in CHECKED:
        fg = hex_of(name, css)
        cells = []
        for bg in BACKDROPS.values():
            r = ratio(fg, bg)
            cells.append(f"{r:>9.2f}")
            if r < args.min:
                bad.append((name, fg, bg, r))
        print(f"{name:<12}{fg:<10}" + "".join(cells))

    if bad:
        print(f"\n不达标（< {args.min}:1）：")
        for name, fg, bg, r in bad:
            print(f"  {name} {fg} on {bg} = {r:.2f}")
        return 1

    # P3 新页面专项组合
    print("\n-- P3 新页面组合 --")
    for label, fg_name, bg_name in SPECIAL_PAIRS:
        fg = expand(fg_name, css)
        bg = expand(bg_name, css)
        r = ratio(fg, bg)
        print(f"{label:<26}{fg} on {bg} = {r:.2f}")
        if r < args.min:
            bad.append((label, fg, bg, r))

    if bad:
        print(f"\n不达标（< {args.min}:1）：")
        for item in bad:
            print(f"  {item[0]} {item[1]} on {item[2]} = {item[3]:.2f}")
        return 1

    # 已知例外：只报告，不拦截
    print("\n-- 已知例外（只报告，不拦截）--")
    for label, fg_name, bg_name in KNOWN_EXCEPTIONS:
        fg = expand(fg_name, css)
        bg = expand(bg_name, css)
        print(f"{label:<26}{fg} on {bg} = {ratio(fg, bg):.2f}")

    print(f"\n全部 ≥ {args.min}:1（WCAG AA 小字；例外见上）")
    return 0


if __name__ == "__main__":
    sys.exit(main())
