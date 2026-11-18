"""M8 验收复测用的造数据 / 核对脚本（只依赖标准库）。

用法（在 app-v2 下）：
  python test/seed-m8.py reset            # 清库 → 写入源区/归档区/12 份真实文件（queued），不建目录
  python test/seed-m8.py state            # 打印各表计数与关键状态，供核对
  python test/seed-m8.py verify-kill 5    # 强杀后核对：5 条 assignments 在、其余回池

库路径与应用一致：%APPDATA%/com.filebutler.app/index.db
第一次改动前会把库备份成 index.db.m8-bak（不覆盖已有备份）。
"""
import json
import os
import shutil
import sqlite3
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
SRC = os.path.join(HERE, "e2e", "m8", "src")
ARC = os.path.join(HERE, "e2e", "m8", "archive")
DB = os.path.join(os.environ["APPDATA"], "com.filebutler.app", "index.db")

NAMES = [
    ("2026年第一季度安全生产检查通知.docx", "word"),
    ("消防设施年度检测报告.pdf", "pdf"),
    ("安全检查整改记录表.xlsx", "excel"),
    ("采购合同-办公用品-2026.docx", "word"),
    ("供应商报价单202511.xlsx", "excel"),
    ("采购申请单2026-03.docx", "word"),
    ("11月第2周会议纪要.docx", "word"),
    ("员工安全培训计划2026.xlsx", "excel"),
    ("安全生产月汇报.pptx", "ppt"),
    ("隐患排查治理清单.xlsx", "excel"),
    ("应急演练方案.docx", "word"),
    ("安全检查制度汇编.pdf", "pdf"),
]


def norm(p):
    return p.replace("\\", "/")


def connect():
    os.makedirs(os.path.dirname(DB), exist_ok=True)
    bak = DB + ".m8-bak"
    if os.path.exists(DB) and not os.path.exists(bak):
        shutil.copy2(DB, bak)
        print(f"已备份原库 → {bak}")
    c = sqlite3.connect(DB)
    c.execute("PRAGMA journal_mode=WAL")
    return c


def cmd_reset():
    os.makedirs(SRC, exist_ok=True)
    os.makedirs(ARC, exist_ok=True)
    c = connect()
    for t in (
        "migration_items", "migrations", "assignments", "files", "batches",
        "file_tags", "tags", "archive_dirs", "source_roots", "settings",
    ):
        c.execute(f"DELETE FROM {t}")

    now = int(time.time() * 1000)
    c.execute("INSERT INTO settings(key,value) VALUES('archive_root',?)", (norm(ARC),))
    c.execute(
        "INSERT INTO source_roots(path,enabled,added_at) VALUES(?,1,?)", (norm(SRC), now)
    )
    root_id = c.execute("SELECT id FROM source_roots").fetchone()[0]

    for i, (name, kind) in enumerate(NAMES):
        p = os.path.join(SRC, name)
        if not os.path.exists(p):
            with open(p, "wb") as f:
                f.write((f"占位内容-{i}\n" * 40).encode("utf-8"))
        st = os.stat(p)
        c.execute(
            """INSERT INTO files(source_root_id,abs_path,rel_path,name,ext,kind,size,mtime,
                                 tag_level,status,defer_count,discovered_at,last_seen_at,missing)
               VALUES(?,?,?,?,?,?,?,?, 'basic','queued',0,?,?,0)""",
            (
                root_id, norm(p), norm(p), name, name.rsplit(".", 1)[-1].lower(), kind,
                st.st_size, int(st.st_mtime * 1000), now, now,
            ),
        )
    c.commit()
    counts = dict(c.execute("SELECT status, COUNT(*) FROM files GROUP BY status").fetchall())
    dirs = c.execute("SELECT COUNT(*) FROM archive_dirs").fetchone()[0]
    print(f"reset ok：files={counts} archive_dirs={dirs} src={norm(SRC)} arc={norm(ARC)}")
    c.close()


def cmd_state():
    c = sqlite3.connect(DB)
    for t in ("settings", "source_roots", "archive_dirs", "files", "assignments", "batches",
              "migrations", "migration_items"):
        n = c.execute(f"SELECT COUNT(*) FROM {t}").fetchone()[0]
        print(f"{t:18} {n}")
    print("status:", dict(c.execute("SELECT status, COUNT(*) FROM files GROUP BY status").fetchall()))
    print("assignments(decision):", dict(
        c.execute("SELECT decision, COUNT(*) FROM assignments GROUP BY decision").fetchall()))
    print("archive_dirs:", c.execute("SELECT rel_path FROM archive_dirs ORDER BY rel_path").fetchall())
    c.close()


def cmd_verify_kill(expect):
    expect = int(expect)
    c = sqlite3.connect(DB)
    acc = c.execute(
        "SELECT COUNT(*) FROM assignments WHERE decision='accepted'").fetchone()[0]
    conf = c.execute("SELECT COUNT(*) FROM files WHERE status='confirmed'").fetchone()[0]
    queued = c.execute(
        "SELECT COUNT(*) FROM files WHERE status IN ('queued','labeled','pending_confirm')"
    ).fetchone()[0]
    rows = c.execute(
        """SELECT f.name, a.decision, a.target_dir_id, f.status
           FROM assignments a JOIN files f ON f.id=a.file_id ORDER BY a.id"""
    ).fetchall()
    c.close()
    print(json.dumps(
        {"accepted": acc, "confirmed": conf, "pool": queued, "rows": rows},
        ensure_ascii=False, indent=1))
    assert acc == expect, f"accepted 应为 {expect}，实际 {acc}"
    assert conf == expect, f"confirmed 应为 {expect}，实际 {conf}"
    print(f"verify-kill ok：{acc} 条已确认落库，池里还有 {queued} 份")


if __name__ == "__main__":
    cmd = sys.argv[1] if len(sys.argv) > 1 else ""
    if cmd == "reset":
        cmd_reset()
    elif cmd == "state":
        cmd_state()
    elif cmd == "verify-kill":
        cmd_verify_kill(sys.argv[2] if len(sys.argv) > 2 else "5")
    else:
        print("用法：reset | state | verify-kill <n>")
