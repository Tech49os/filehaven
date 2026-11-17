# -*- coding: utf-8 -*-
"""
真实目录只读盘点脚本（绝不移动/删除/写入被扫描目录）
输出 inventory.json 供后续分类测试使用
"""
import os
import sys
import json
import hashlib
from collections import Counter, defaultdict
from datetime import datetime

sys.stdout.reconfigure(encoding="utf-8")

TARGETS = [
    r"C:\Users\kingb\Desktop",
]

SKIP_DIRS = {".git", "node_modules", "__pycache__", ".filemanager"}
SKIP_EXT = {".lnk", ".ini", ".exe", ".url", ".tmp", ".bkp"}
MAX_DEPTH = 3

OFFICE = {".doc", ".docx", ".xls", ".xlsx", ".ppt", ".pptx", ".csv"}
DOCS = {".pdf", ".txt", ".md"}
DIAGRAM = {".drawio", ".xml", ".vsdx", ".mm", ".xmind"}
IMAGE = {".jpg", ".jpeg", ".png", ".gif", ".bmp", ".webp", ".svg"}
ARCHIVE = {".zip", ".rar", ".7z", ".tar", ".gz"}


def category_of(ext):
    e = ext.lower()
    if e in OFFICE:
        return "Office文档"
    if e in DOCS:
        return "PDF/文本"
    if e in DIAGRAM:
        return "图表/架构"
    if e in IMAGE:
        return "图片"
    if e in ARCHIVE:
        return "压缩包"
    return "其他"


def human(n):
    for u in ["B", "KB", "MB", "GB"]:
        if n < 1024:
            return f"{n:.0f}{u}"
        n /= 1024
    return f"{n:.1f}TB"


def main():
    files = []
    dirs_seen = 0
    for root_target in TARGETS:
        base = os.path.abspath(root_target)
        base_depth = base.rstrip("\\").count("\\")
        for cur, dirnames, filenames in os.walk(base):
            depth = cur.rstrip("\\").count("\\") - base_depth
            dirnames[:] = [d for d in dirnames if d not in SKIP_DIRS and depth < MAX_DEPTH]
            if depth == 0:
                dirs_seen = len(dirnames)
            for fn in filenames:
                p = os.path.join(cur, fn)
                try:
                    st = os.stat(p)
                except OSError:
                    continue
                ext = os.path.splitext(fn)[1].lower()
                rel = os.path.relpath(p, base)
                files.append({
                    "path": p,
                    "rel": rel,
                    "name": fn,
                    "ext": ext.lstrip("."),
                    "size": st.st_size,
                    "size_h": human(st.st_size),
                    "mtime": datetime.fromtimestamp(st.st_mtime).strftime("%Y-%m-%d"),
                    "mtime_ts": int(st.st_mtime),
                    "depth": rel.count("\\"),
                    "folder": os.path.dirname(rel) or "(根)",
                    "kind": category_of(ext),
                    "skipped_ext": ext in SKIP_EXT,
                })

    real = [f for f in files if not f["skipped_ext"]]
    print("=" * 72)
    print(f"扫描根目录: {TARGETS[0]}")
    print(f"一级子目录数: {dirs_seen}   文件总数: {len(files)}   有效候选: {len(real)}")
    print("=" * 72)

    print("\n【按类型分布】")
    for k, v in Counter(f["kind"] for f in real).most_common():
        sz = sum(f["size"] for f in real if f["kind"] == k)
        print(f"  {k:<10} {v:>4} 个   {human(sz):>9}")

    print("\n【按扩展名 Top12】")
    for k, v in Counter(f["ext"] or "(无)" for f in real).most_common(12):
        print(f"  .{k:<10} {v:>4}")

    print("\n【散落在根目录的文件（未归入任何文件夹）】")
    root_files = [f for f in real if f["depth"] == 0]
    print(f"  共 {len(root_files)} 个")
    for f in sorted(root_files, key=lambda x: -x["size"])[:20]:
        print(f"    {f['size_h']:>8}  {f['mtime']}  {f['name'][:58]}")

    print("\n【一级文件夹（已有组织）】")
    folders = Counter(f["folder"].split("\\")[0] for f in real if f["depth"] > 0)
    for k, v in folders.most_common(20):
        print(f"  {k[:40]:<42} {v:>3} 个文件")

    print("\n【时间跨度】")
    years = Counter(f["mtime"][:4] for f in real)
    for k in sorted(years):
        print(f"  {k}: {years[k]:>3} 个")
    if real:
        oldest = min(real, key=lambda x: x["mtime_ts"])
        newest = max(real, key=lambda x: x["mtime_ts"])
        print(f"  最早: {oldest['mtime']} {oldest['name'][:40]}")
        print(f"  最新: {newest['mtime']} {newest['name'][:40]}")

    print("\n【同名文件（不同目录下重名）】")
    byname = defaultdict(list)
    for f in real:
        byname[f["name"].lower()].append(f)
    dups = {k: v for k, v in byname.items() if len(v) > 1}
    if dups:
        for k, v in list(dups.items())[:10]:
            print(f"  {k[:50]}  ×{len(v)}")
            for x in v:
                print(f"      {x['size_h']:>8}  {x['rel']}")
    else:
        print("  无")

    print("\n【大文件 Top10】")
    for f in sorted(real, key=lambda x: -x["size"])[:10]:
        print(f"  {f['size_h']:>9}  {f['mtime']}  {f['rel'][:60]}")

    # 内容指纹：找出真正内容重复的文件（同大小同哈希）
    print("\n【内容完全重复的文件（MD5）】")
    by_size = defaultdict(list)
    for f in real:
        if f["size"] > 1024:
            by_size[f["size"]].append(f)
    hash_groups = defaultdict(list)
    for sz, group in by_size.items():
        if len(group) < 2:
            continue
        for f in group:
            try:
                with open(f["path"], "rb") as fh:
                    hash_groups[hashlib.md5(fh.read()).hexdigest()].append(f)
            except OSError:
                pass
    dup_hash = {k: v for k, v in hash_groups.items() if len(v) > 1}
    if dup_hash:
        wasted = sum(v[0]["size"] * (len(v) - 1) for v in dup_hash.values())
        print(f"  {len(dup_hash)} 组重复，浪费约 {human(wasted)}")
        for k, v in list(dup_hash.items())[:6]:
            print(f"    ×{len(v)} {v[0]['size_h']}  {v[0]['name'][:44]}")
    else:
        print("  无")

    out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "inventory.json")
    with open(out, "w", encoding="utf-8") as fh:
        json.dump({"target": TARGETS[0], "files": real}, fh, ensure_ascii=False, indent=1)
    print(f"\n清单已写出: {out}  ({len(real)} 条)")


if __name__ == "__main__":
    main()
