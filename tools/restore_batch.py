#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
迁移批次还原（撤销）

把某个批次的文件从隔离区还原回原始位置，全程哈希校验，绝不覆盖既有文件。

用法:
    python tools/restore_batch.py --list                列出所有批次
    python tools/restore_batch.py --dry-run             预览最新批次还原计划
    python tools/restore_batch.py --batch 20261116-183624
    python tools/restore_batch.py --latest --keep-archive

默认行为（完整还原 = 回到迁移前的状态）:
    1. 隔离区文件 -> 复制回原始位置（已存在同名文件则加序号，不覆盖）
    2. 校验哈希
    3. 校验通过后删除归档副本，并清空隔离区里这一份
    4. 追加还原记录到台账

--keep-archive: 只还原到源位置，保留归档副本（会产生两份）
"""

import argparse
import hashlib
import json
import os
import shutil
import sys
import time

sys.stdout.reconfigure(encoding="utf-8")

ARCHIVE_ROOT = r"D:\文档归档"
META_DIR = os.path.join(ARCHIVE_ROOT, ".filemanager")
LEDGER = os.path.join(META_DIR, "ledger", "migrations.jsonl")


def sha256(path, chunk=1 << 20):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for b in iter(lambda: f.read(chunk), b""):
            h.update(b)
    return h.hexdigest()


def load_batches():
    if not os.path.isfile(LEDGER):
        return []
    out = []
    with open(LEDGER, encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                d = json.loads(line)
            except json.JSONDecodeError:
                continue
            # 还原记录本身也写在同一个台账里，跳过
            if d.get("type") == "restore":
                continue
            out.append(d)
    return out


def unique_target(dir_, name):
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
    ap = argparse.ArgumentParser()
    ap.add_argument("--list", action="store_true", help="列出所有批次")
    ap.add_argument("--batch", help="批次号")
    ap.add_argument("--latest", action="store_true", help="使用最新批次")
    ap.add_argument("--dry-run", action="store_true", help="只预览，不动文件")
    ap.add_argument("--keep-archive", action="store_true", help="保留归档副本")
    args = ap.parse_args()

    batches = load_batches()

    if args.list:
        if not batches:
            print("台账里还没有批次")
            return 0
        print(f"{'批次':<20}{'时间':<21}{'文件数':<8}状态")
        print("-" * 62)
        for b in batches:
            recs = b.get("records", [])
            ok = len([r for r in recs if r.get("status") == "migrated"])
            print(f"{b['batch']:<20}{b.get('time',''):<21}{ok:<8}已迁移")
        return 0

    if not batches:
        print("台账里没有可还原的批次")
        return 1

    if args.batch:
        target = next((b for b in batches if b["batch"] == args.batch), None)
        if not target:
            print(f"找不到批次 {args.batch}")
            return 1
    elif args.latest:
        target = batches[-1]
    else:
        print("请指定 --batch 或 --latest（用 --list 查看）")
        return 1

    mode = "预览（不修改任何文件）" if args.dry_run else "执行还原"
    print("=" * 72)
    print(f"批次 {target['batch']}  ·  {mode}")
    print(f"归档根: {ARCHIVE_ROOT}")
    print("=" * 72)

    done = failed = 0
    records = []

    for r in target["records"]:
        if r.get("status") != "migrated":
            continue
        src = r["src"]            # 原始位置（桌面）
        trash = r["trash"]        # 隔离区原件
        dest = r["dest"]          # 归档成品
        name = os.path.basename(src)
        print(f"\n{name}")

        if not os.path.isfile(trash):
            print(f"  ✗ 隔离区文件不存在，跳过: {trash}")
            failed += 1
            continue

        want = r["sha256"]
        got = sha256(trash)
        if got != want:
            print(f"  ✗ 隔离区文件哈希不符（期望 {want[:16]}… 实际 {got[:16]}…），跳过")
            failed += 1
            continue

        os.makedirs(os.path.dirname(src), exist_ok=True)
        final, renamed = unique_target(os.path.dirname(src), name)
        if args.dry_run:
            print(f"  将还原到: {final}" + ("（同名，加序号）" if renamed else ""))
            print(f"  将删除归档副本: {dest}" if not args.keep_archive else "  保留归档副本")
            done += 1
            continue

        shutil.copy2(trash, final)
        if sha256(final) != want:
            print("  ✗ 还原后哈希不一致，已删除坏副本，归档副本未动")
            try:
                os.remove(final)
            except OSError:
                pass
            failed += 1
            continue
        print(f"  ✓ 已还原到 {final}")

        if not args.keep_archive:
            try:
                os.remove(dest)
                print(f"  ✓ 归档副本已删除")
            except OSError as e:
                print(f"  ! 归档副本删除失败: {e}")
            try:
                os.remove(trash)
            except OSError:
                pass

        done += 1
        records.append({
            "seq": r["seq"], "restored_to": final,
            "removed_archive": None if args.keep_archive else dest,
        })

    if not args.dry_run and records:
        with open(LEDGER, "a", encoding="utf-8") as f:
            f.write(json.dumps({
                "type": "restore",
                "batch": target["batch"],
                "time": time.strftime("%Y-%m-%d %H:%M:%S"),
                "keep_archive": args.keep_archive,
                "records": records,
            }, ensure_ascii=False) + "\n")

    print("\n" + "=" * 72)
    print(f"{'可还原' if args.dry_run else '已还原'} {done} 个 / 失败 {failed} 个")
    print("=" * 72)
    return 0 if failed == 0 else 2


if __name__ == "__main__":
    sys.exit(main())
