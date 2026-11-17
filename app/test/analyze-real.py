# -*- coding: utf-8 -*-
"""从真实文件名+正文里归纳真实主题（只读分析）"""
import json, re, os, sys
from collections import Counter, defaultdict

sys.stdout.reconfigure(encoding="utf-8")
D = os.path.dirname(os.path.abspath(__file__))
data = json.load(open(os.path.join(D, "real-classify.json"), encoding="utf-8"))
inv = {f["rel"]: f for f in json.load(open(os.path.join(D, "inventory.json"), encoding="utf-8"))["files"]}

# 通用噪声词（在多数语境都出现，不构成主题）
STOP = set("""文件 文档 资料 报告 方案 情况 工作 项目 相关 关于 内容 说明 版本 更新 新建
副本 修改 最终 初版 草稿 完 的 和 与 及 对 为 在 是 了 年 月 日 版 汇总 清单 模板 表 记录 通知
附件 材料 情况汇报 汇报材料 说明文档 补充 整理 归档 下载 文件 夹""".split())

names = []
for f in data["details"]:
    nm = os.path.splitext(f["name"])[0]
    names.append(nm)

def grams(s):
    out = []
    for seg in re.findall(r"[\u4e00-\u9fff]+", s):
        for i in range(len(seg) - 1):
            out.append(seg[i:i+2])
        for i in range(len(seg) - 2):
            out.append(seg[i:i+3])
        for i in range(len(seg) - 3):
            out.append(seg[i:i+4])
    for w in re.findall(r"[a-zA-Z]{2,}", s):
        out.append(w.lower())
    return out

cnt = Counter()
for nm in names:
    for g in set(grams(nm)):
        if g in STOP or len(g) < 2:
            continue
        cnt[g] += 1

print("=" * 70)
print("【文件名高频主题词 Top60】（出现在 ≥4 个文件中）")
print("=" * 70)
shown = 0
for w, c in cnt.most_common(300):
    if c < 4:
        break
    if w in STOP:
        continue
    print(f"  {w:<8} {c:>3}", end="")
    shown += 1
    if shown % 5 == 0:
        print()
print("\n")

# 按已有文件夹看分布，判断用户自己的分类直觉
print("=" * 70)
print("【用户已有的文件夹 = 真实的分类雏形】")
print("=" * 70)
byfolder = defaultdict(list)
for rel, f in inv.items():
    top = rel.split("\\")[0] if "\\" in rel else "(根目录散落)"
    byfolder[top].append(f["name"])
for k in sorted(byfolder, key=lambda x: -len(byfolder[x])):
    print(f"\n  ▸ {k}  ({len(byfolder[k])} 个)")
    for n in byfolder[k][:4]:
        print(f"      {n[:64]}")

# 归档目录的内部结构（这是问题最集中的地方）
print("\n" + "=" * 70)
print("【归档\\ 内部结构 —— 同一批文件被按日期反复存档】")
print("=" * 70)
arch = defaultdict(list)
for rel in inv:
    if rel.startswith("归档\\"):
        parts = rel.split("\\")
        key = "\\".join(parts[1:3]) if len(parts) > 2 else parts[1]
        arch[key].append(parts[-1])
for k in sorted(arch):
    print(f"  {k:<28} {len(arch[k]):>3} 个")
