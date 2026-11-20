#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""把真实索引库导出成「留一法」评测集（纯标准库）。

用法：
    python test/export-eval.py [源 index.db] [输出 json]

默认源：`%APPDATA%/com.filebutler.app/index.db`；默认输出 `test/fixtures/eval-cases.json`。

为什么要留一法
--------------
评测要回答的是「新来一份文件，程序能不能推荐到它真实的目录」。
如果把 case 自己也算进目录的「已确认文件」里，那 familyMatch / kindBias 就等于
拿着答案算分 —— 分数虚高，改前改后都没有可比性。所以：

* `dirs[].fileNames` / `dirs[].kinds` 里**不摘** case（同一份 JSON 给多个 case 用），
  摘除由 Rust 侧 `--eval` 在算这一条时做，见 `main.rs::run_eval_cli`；
* case 只带 `expectDir`（真实落点）+ 文件自身元信息，不带任何目录线索。

真库是 WAL 模式：只拷 index.db 会漏掉未 checkpoint 的最新写入，
所以 db / -wal / -shm 三个文件一起拷到临时目录再打开。
"""

import json
import os
import shutil
import sqlite3
import sys
import tempfile

DEFAULT_DB = os.path.join(
    os.environ.get("APPDATA", ""), "com.filebutler.app", "index.db"
)
HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_OUT = os.path.join(HERE, "fixtures", "eval-cases.json")

# 门槛（计划 §4 T0.2）：不够就在产出里写 insufficient，前端/报告据此降级
MIN_CASES = 40
MIN_DIRS = 5


def open_copy(src):
    """拷一份（含 WAL）到临时目录再打开，绝不碰用户真实索引库。"""
    tmp = tempfile.mkdtemp(prefix="fb-eval-")
    base = os.path.basename(src)
    for suf in ("", "-wal", "-shm"):
        s = src + suf
        if os.path.exists(s):
            shutil.copy2(s, os.path.join(tmp, base + suf))
    return sqlite3.connect(os.path.join(tmp, base)), tmp


def main():
    src = sys.argv[1] if len(sys.argv) > 1 else DEFAULT_DB
    out = sys.argv[2] if len(sys.argv) > 2 else DEFAULT_OUT
    if not os.path.exists(src):
        print(f"找不到索引库：{src}")
        return 1

    conn, tmp = open_copy(src)
    try:
        # 目录全量：评测必须知道「池子里一共有哪些目录可选」，否则 top3 无意义
        dirs = []
        for did, name, rel in conn.execute(
            "SELECT id, name, rel_path FROM archive_dirs ORDER BY depth, id"
        ):
            dirs.append(
                {"id": did, "name": name, "relPath": rel, "fileNames": [], "kinds": []}
            )
        by_id = {d["id"]: d for d in dirs}

        # 每个目录的已确认文件（含已迁移）：只认最后一次判定的落点
        rows = conn.execute(
            """
            SELECT a.target_dir_id, f.name, f.kind
              FROM assignments a
              JOIN files f ON f.id = a.file_id
             WHERE a.target_dir_id IS NOT NULL
               AND f.status IN ('confirmed','migrated')
               AND a.id = (SELECT MAX(a2.id) FROM assignments a2 WHERE a2.file_id = a.file_id)
            """
        ).fetchall()
        for did, name, kind in rows:
            d = by_id.get(did)
            if d is None:
                continue
            d["fileNames"].append(name)
            d["kinds"].append(kind)

        # 目录的 topic 标签（P2 之前几乎为空，但留好通道）
        tag_rows = conn.execute(
            """
            SELECT a.target_dir_id, t.name
              FROM assignments a
              JOIN files f ON f.id = a.file_id
              JOIN file_tags ft ON ft.file_id = f.id
              JOIN tags t ON t.id = ft.tag_id AND t.kind = 'topic'
             WHERE a.target_dir_id IS NOT NULL
               AND f.status IN ('confirmed','migrated')
            """
        ).fetchall()
        tags_by_dir = {}
        for did, tname in tag_rows:
            tags_by_dir.setdefault(did, [])
            if tname not in tags_by_dir[did]:
                tags_by_dir[did].append(tname)

        cases = []
        for did, name, kind, mtime in conn.execute(
            """
            SELECT a.target_dir_id, f.name, f.kind, f.mtime
              FROM assignments a
              JOIN files f ON f.id = a.file_id
             WHERE a.target_dir_id IS NOT NULL
               AND f.status IN ('confirmed','migrated')
               AND a.id = (SELECT MAX(a2.id) FROM assignments a2 WHERE a2.file_id = a.file_id)
             ORDER BY f.mtime DESC
            """
        ):
            d = by_id.get(did)
            if d is None:
                continue
            cases.append(
                {
                    "fileName": name,
                    "kind": kind,
                    "mtime": mtime,
                    "expectDir": d["relPath"],
                    "hasDeepTags": False,
                }
            )

        for d in dirs:
            d["topicTags"] = tags_by_dir.get(d["id"], [])

        payload = {
            "source": src,
            "dirs": dirs,
            "cases": cases,
            "insufficient": len(cases) < MIN_CASES or len(dirs) < MIN_DIRS,
            "note": (
                f"留一集：{len(cases)} 份 / {len(dirs)} 个目录。"
                f"门槛 {MIN_CASES} 份 / {MIN_DIRS} 目录，"
                + ("不足，结论仅供参考。" if len(cases) < MIN_CASES or len(dirs) < MIN_DIRS
                   else "达标。")
            ),
        }
    finally:
        conn.close()
        shutil.rmtree(tmp, ignore_errors=True)

    os.makedirs(os.path.dirname(out), exist_ok=True)
    with open(out, "w", encoding="utf-8") as f:
        json.dump(payload, f, ensure_ascii=False, indent=1)

    used = len({c["expectDir"] for c in cases})
    print(f"cases={len(cases)} dirs={len(dirs)} 命中目录数={used} -> {out}")
    if payload["insufficient"]:
        print("!! 样本不足门槛，评测结论仅供参考")
    return 0


if __name__ == "__main__":
    sys.exit(main())
