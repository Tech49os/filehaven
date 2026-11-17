#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
钻采院项目 试点迁移（三段式：备份 → 复制校验 → 源文件入隔离区）

安全约束（硬编码，不可绕过）：
  1. 只处理 PLAN 中显式列出的文件，绝不目录通配
  2. 每个文件必须先备份成功，才允许复制
  3. 每个文件必须 SHA-256 与源一致，才允许把源移入隔离区
  4. 目标同名 -> 加序号，永不覆盖
  5. 源文件不删除，移入 .filemanager/trash/<批次>/ 可随时还原
  6. 任一文件失败 -> 该文件回滚，后续文件不处理
"""

import hashlib
import json
import os
import shutil
import sys
import time

sys.stdout.reconfigure(encoding="utf-8")

SRC_ROOT = r"C:\Users\kingb\Desktop"
ARCHIVE_ROOT = r"D:\文档归档"
META_DIR = os.path.join(ARCHIVE_ROOT, ".filemanager")

BATCH_ID = time.strftime("%Y%m%d-%H%M%S")
BACKUP_DIR = os.path.join(META_DIR, "backup", BATCH_ID)
TRASH_DIR = os.path.join(META_DIR, "trash", BATCH_ID)

# (源相对路径, 目标子目录, 目标文件名)  —— 全部显式声明，不做推断
PLAN = [
    (r"文件夹\钻采工程研究院+高温高压设备项目智能化提升方案.doc",
     r"钻采院项目\方案设计",
     "2026-04_钻采工程研究院+高温高压设备项目智能化提升方案.doc"),
    (r"钻采院项目\副本钻采院高温高压项目-工作量-成本20260519V3.1.xlsx",
     r"钻采院项目\成本报价",
     "2026-06_副本钻采院高温高压项目-工作量-成本20260519V3.1.xlsx"),
    (r"钻采院项目\钻采研究院智慧安全监管平台可行性研究报告9.29.doc",
     r"钻采院项目\方案设计",
     "2026-04_钻采研究院智慧安全监管平台可行性研究报告9.29.doc"),
    (r"钻采院项目\钻采研究院智慧安全监管平台可行性研究报告v1.0.doc",
     r"钻采院项目\方案设计",
     "2026-04_钻采研究院智慧安全监管平台可行性研究报告v1.0.doc"),
    (r"钻采院项目\钻采研究院智能化项目方案v2.0.docx",
     r"钻采院项目\方案设计",
     "2026-06_钻采研究院智能化项目方案v2.0.docx"),
    (r"钻采院项目\钻采院方案.docx",
     r"钻采院项目\方案设计",
     "2026-04_钻采院方案.docx"),
    (r"钻采院项目\钻采院设备清单.xls",
     r"钻采院项目",
     "2026-04_钻采院设备清单.xls"),
    (r"钻采院项目\钻采院高温高压项目-工作量-成本20260427V1.0.xlsx",
     r"钻采院项目\成本报价",
     "2026-04_钻采院高温高压项目-工作量-成本20260427V1.0.xlsx"),
    (r"钻采院项目\钻采院高温高压项目-工作量-成本20260519V2.0.xlsx",
     r"钻采院项目\成本报价",
     "2026-06_钻采院高温高压项目-工作量-成本20260519V2.0.xlsx"),
    (r"钻采院项目\钻采院高温高压项目-工作量-成本20260519V3.1.xlsx",
     r"钻采院项目\成本报价",
     "2026-06_钻采院高温高压项目-工作量-成本20260519V3.1.xlsx"),
]


def sha256(path, chunk=1 << 20):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        while True:
            b = f.read(chunk)
            if not b:
                break
            h.update(b)
    return h.hexdigest()


def unique_target(dir_, name):
    """同名加序号，永不覆盖"""
    target = os.path.join(dir_, name)
    if not os.path.exists(target):
        return target, False
    stem, ext = os.path.splitext(name)
    n = 1
    while True:
        cand = os.path.join(dir_, f"{stem}_{n}{ext}")
        if not os.path.exists(cand):
            return cand, True
        n += 1


def main():
    print("=" * 72)
    print(f"钻采院项目 试点迁移   批次 {BATCH_ID}")
    print(f"源: {SRC_ROOT}")
    print(f"目标: {ARCHIVE_ROOT}")
    print("=" * 72)

    # ---- 预检：源文件全部存在 ----
    missing = [p for p, _, _ in PLAN if not os.path.isfile(os.path.join(SRC_ROOT, p))]
    if missing:
        print("\n预检失败，以下源文件不存在，整批中止：")
        for m in missing:
            print("  ", m)
        return 1
    total = sum(os.path.getsize(os.path.join(SRC_ROOT, p)) for p, _, _ in PLAN)
    print(f"\n预检通过：{len(PLAN)} 个文件，合计 {total / 1024 / 1024:.2f} MB")

    # ---- 建目录 ----
    for d in (ARCHIVE_ROOT, BACKUP_DIR, TRASH_DIR,
              os.path.join(META_DIR, "ledger")):
        os.makedirs(d, exist_ok=True)

    records = []
    failed = 0

    for i, (rel, subdir, newname) in enumerate(PLAN, 1):
        src = os.path.join(SRC_ROOT, rel)
        size = os.path.getsize(src)
        print(f"\n[{i}/{len(PLAN)}] {rel}")
        print(f"        大小 {size:,} 字节")

        rec = {
            "seq": i,
            "src": src,
            "src_rel": rel,
            "size": size,
            "status": "pending",
        }

        try:
            # --- 第 1 段：备份 ---
            bak = os.path.join(BACKUP_DIR, rel)
            os.makedirs(os.path.dirname(bak), exist_ok=True)
            shutil.copy2(src, bak)
            src_hash = sha256(src)
            if sha256(bak) != src_hash:
                raise RuntimeError("备份哈希与源不一致")
            rec["backup"] = bak
            rec["sha256"] = src_hash
            print(f"        ✓ 备份完成  {src_hash[:16]}")

            # --- 第 2 段：复制到归档目录 ---
            dest_dir = os.path.join(ARCHIVE_ROOT, subdir)
            os.makedirs(dest_dir, exist_ok=True)
            dest, renamed = unique_target(dest_dir, newname)
            shutil.copy2(src, dest)
            dest_hash = sha256(dest)
            if dest_hash != src_hash:
                # 不一致 -> 立刻删掉这个坏的副本，本文件判失败
                try:
                    os.remove(dest)
                except OSError:
                    pass
                raise RuntimeError("复制后哈希不一致，已删除坏副本")
            rec["dest"] = dest
            rec["dest_rel"] = os.path.relpath(dest, ARCHIVE_ROOT)
            rec["renamed"] = renamed
            flag = "（同名冲突，已加序号）" if renamed else ""
            print(f"        ✓ 复制校验通过 {flag}")
            print(f"        → {rec['dest_rel']}")

            # --- 第 3 段：源文件移入隔离区 ---
            trash_path = os.path.join(TRASH_DIR, rel)
            os.makedirs(os.path.dirname(trash_path), exist_ok=True)
            shutil.copy2(src, trash_path)
            if sha256(trash_path) != src_hash:
                raise RuntimeError("隔离区副本哈希不一致，源文件保持原位")
            os.remove(src)
            rec["trash"] = trash_path
            rec["status"] = "migrated"
            print(f"        ✓ 源文件已移入隔离区（未删除，可还原）")

        except Exception as e:
            rec["status"] = "failed"
            rec["error"] = str(e)
            failed += 1
            print(f"        ✗ 失败：{e}")
            print("        后续文件不再处理，批次中止")
            records.append(rec)
            break

        records.append(rec)

    # ---- 写台账 ----
    ledger_path = os.path.join(META_DIR, "ledger", "migrations.jsonl")
    with open(ledger_path, "a", encoding="utf-8") as f:
        f.write(json.dumps({
            "batch": BATCH_ID,
            "time": time.strftime("%Y-%m-%d %H:%M:%S"),
            "archive_root": ARCHIVE_ROOT,
            "records": records,
        }, ensure_ascii=False) + "\n")

    ok = [r for r in records if r["status"] == "migrated"]
    print("\n" + "=" * 72)
    print(f"完成：成功 {len(ok)} 个 / 计划 {len(PLAN)} 个" + (f"，失败 {failed}" if failed else ""))
    print(f"台账: {ledger_path}")
    print(f"备份: {BACKUP_DIR}")
    print(f"隔离区: {TRASH_DIR}")
    print("=" * 72)
    return 0 if failed == 0 and len(ok) == len(PLAN) else 2


if __name__ == "__main__":
    sys.exit(main())
