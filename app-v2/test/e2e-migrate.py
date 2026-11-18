"""M7 端到端手测（T7.7）：10 份真实文件走完「确认 → 预览 → 迁移 → 撤销」，逐字节核对。

用法（在 app-v2 下）：
  python test/e2e-migrate.py seed             # 造语料 + 写库（10 份标成 confirmed）
  python test/e2e-migrate.py snap             # 记下迁移前每份的 sha256
  python test/e2e-migrate.py verify-migrated  # 迁移后：源目录空、归档齐、隔离区齐、台账齐
  python test/e2e-migrate.py verify-undone    # 撤销后：逐字节还原、归档副本删除、隔离区空

只依赖标准库；库路径与应用一致（%APPDATA%/com.filebutler.app/index.db）。
"""
import hashlib
import json
import os
import shutil
import sqlite3
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
SRC = os.path.join(HERE, "e2e", "src")
ARC = os.path.join(HERE, "e2e", "archive")
SNAP = os.path.join(HERE, "e2e", "sha-before.json")
DB = os.path.join(os.environ["APPDATA"], "com.filebutler.app", "index.db")

# 目录 → 文件（名字, 内容生成器）
PLAN = {
    "安全检查": [
        ("安全检查通知.docx", b"notice-" * 300),
        ("消防检测报告.pdf", b"pdf-" * 500),
        ("整改记录表.xlsx", b"xlsx-" * 200),
        ("安全问题清单.docx", b"list-" * 120),
    ],
    "合同/采购": [
        ("采购合同-办公用品.docx", b"contract-" * 400),
        ("供应商报价单.xlsx", b"quote-" * 250),
        ("采购申请单.docx", b"apply-" * 150),
    ],
    "汇报材料": [
        ("季度汇报.pptx", b"ppt-" * 800),
        ("经营分析.pdf", b"report-" * 600),
        ("周会纪要.docx", b"minutes-" * 90),
    ],
}
# 归档区里预先放一份同名文件：迁移必须错开成 _2，绝不覆盖
PRE_EXIST = ("安全检查/安全检查通知.docx", b"PRE-EXISTING-DO-NOT-TOUCH")


def sha(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def norm(p):
    return p.replace("\\", "/")


def all_files(root):
    out = []
    for dirpath, _dirs, names in os.walk(root):
        for n in names:
            out.append(os.path.join(dirpath, n))
    return out


def cmd_seed():
    for d in (SRC, ARC):
        if os.path.isdir(d):
            shutil.rmtree(d)
    os.makedirs(SRC, exist_ok=True)

    # 归档区：目录先不存在，让预览显示「（新建目录）」；只预置一份同名文件
    pre_path = os.path.join(ARC, *PRE_EXIST[0].split("/"))
    os.makedirs(os.path.dirname(pre_path), exist_ok=True)
    with open(pre_path, "wb") as f:
        f.write(PRE_EXIST[1])

    os.makedirs(os.path.dirname(DB), exist_ok=True)
    bak = DB + ".e2e-bak"
    if os.path.exists(DB) and not os.path.exists(bak):
        shutil.copy2(DB, bak)
    c = sqlite3.connect(DB)
    c.execute("PRAGMA journal_mode=WAL")
    for t in ("migration_items", "migrations", "assignments", "files", "archive_dirs", "source_roots", "batches"):
        c.execute(f"DELETE FROM {t}")

    now = int(time.time() * 1000)
    c.execute("INSERT INTO settings(key,value) VALUES('archive_root',?) "
              "ON CONFLICT(key) DO UPDATE SET value=excluded.value", (norm(ARC),))
    c.execute("INSERT INTO source_roots(path,enabled,added_at) VALUES(?,1,?)", (norm(SRC), now))
    root_id = c.execute("SELECT id FROM source_roots").fetchone()[0]

    dir_ids = {}
    for rel in PLAN:
        parent, cur = None, ""
        for seg in rel.split("/"):
            cur = f"{cur}/{seg}" if cur else seg
            row = c.execute("SELECT id FROM archive_dirs WHERE rel_path=?", (cur,)).fetchone()
            if row:
                parent = row[0]
                continue
            parent = c.execute(
                "INSERT INTO archive_dirs(parent_id,name,rel_path,depth,origin,created_at) "
                "VALUES(?,?,?,?, 'user', ?)", (parent, seg, cur, cur.count("/") + 1, now)
            ).lastrowid
        dir_ids[rel] = parent

    n = 0
    for rel, items in PLAN.items():
        for name, body in items:
            p = os.path.join(SRC, name)
            with open(p, "wb") as f:
                f.write(body)
            st = os.stat(p)
            ext = name.rsplit(".", 1)[-1].lower()
            kind = {"docx": "word", "pdf": "pdf", "xlsx": "excel", "pptx": "ppt"}.get(ext, "other")
            fid = c.execute(
                """INSERT INTO files(source_root_id,abs_path,rel_path,name,ext,kind,size,mtime,
                                     tag_level,status,defer_count,discovered_at,last_seen_at,missing)
                   VALUES(?,?,?,?,?,?,?,?, 'basic','confirmed',0,?,?,0)""",
                (root_id, norm(p), norm(p), name, ext, kind, st.st_size,
                 int(st.st_mtime * 1000), now, now),
            ).lastrowid
            c.execute(
                """INSERT INTO assignments(file_id,batch_id,target_dir_id,origin,confidence,reason,
                                           decision,decided_at)
                   VALUES(?,NULL,?, 'user_picked',NULL,?, 'accepted',?)""",
                (fid, dir_ids[rel], f"端到端手测：确认进 {rel}", now),
            )
            n += 1
    c.commit()
    c.close()
    print(f"seed ok：源区 {n} 份 → {norm(SRC)}\n归档区 {norm(ARC)}（预置 1 份同名）")


def cmd_snap():
    data = {norm(p): sha(p) for p in all_files(SRC)}
    with open(SNAP, "w", encoding="utf-8") as f:
        json.dump(data, f, ensure_ascii=False, indent=1)
    print(f"snap ok：{len(data)} 份")


def cmd_verify_migrated():
    snap = json.load(open(SNAP, encoding="utf-8"))
    left = all_files(SRC)
    assert not left, f"源目录还有 {len(left)} 份：{left[:3]}"
    q = os.path.join(ARC, ".filebutler", "quarantine")
    qf = all_files(q)
    assert len(qf) == 10, f"隔离区应有 10 份，实际 {len(qf)}"
    arch = [p for p in all_files(ARC) if ".filebutler" not in norm(p)]
    assert len(arch) == 11, f"归档区应有 11 份（10 + 预置 1），实际 {len(arch)}"
    # 逐份核对内容：归档副本必须与快照一致
    got = {}
    for p in arch:
        got.setdefault(sha(p), []).append(norm(p))
    missing = [k for k in snap.values() if k not in got]
    assert not missing, f"归档区缺 {len(missing)} 份内容（sha 对不上）"
    pre = os.path.join(ARC, *PRE_EXIST[0].split("/"))
    assert open(pre, "rb").read() == PRE_EXIST[1], "预置的同名文件被覆盖了！"
    renamed = [p for p in arch if "安全检查通知_2" in os.path.basename(p)]
    assert renamed, "同名文件没有错开成 _2"

    c = sqlite3.connect(DB)
    st = dict(c.execute("SELECT status, COUNT(*) FROM files GROUP BY status").fetchall())
    assert st.get("migrated") == 10, f"状态不对：{st}"
    mig = c.execute("SELECT id,status,file_count,finished_at FROM migrations ORDER BY id DESC").fetchone()
    assert mig and mig[1] == "done" and mig[2] == 10 and mig[3], f"迁移台账不对：{mig}"
    items = c.execute("SELECT COUNT(*), SUM(result='ok') FROM migration_items WHERE migration_id=?",
                      (mig[0],)).fetchone()
    assert items[0] == 10 and items[1] == 10, f"台账明细不对：{items}"
    c.close()
    print(f"verify-migrated ok：归档 11 / 隔离区 10 / 台账 10 ok / 状态 migrated=10 / 迁移 #{mig[0]}")
    return mig[0]


def cmd_verify_undone():
    snap = json.load(open(SNAP, encoding="utf-8"))
    back = {norm(p): sha(p) for p in all_files(SRC)}
    assert len(back) == 10, f"源目录应有 10 份，实际 {len(back)}"
    for p, h in snap.items():
        assert back.get(p) == h, f"逐字节不一致：{p}"
    arch = [p for p in all_files(ARC) if ".filebutler" not in norm(p)]
    assert len(arch) == 1, f"归档区应只剩预置的 1 份，实际 {len(arch)}：{[os.path.basename(x) for x in arch]}"
    pre = os.path.join(ARC, *PRE_EXIST[0].split("/"))
    assert open(pre, "rb").read() == PRE_EXIST[1], "预置文件被动了"
    q = all_files(os.path.join(ARC, ".filebutler", "quarantine"))
    assert not q, f"隔离区应为空，实际 {len(q)} 份"
    c = sqlite3.connect(DB)
    st = dict(c.execute("SELECT status, COUNT(*) FROM files GROUP BY status").fetchall())
    assert st.get("confirmed") == 10, f"状态不对：{st}"
    mig = c.execute("SELECT status FROM migrations ORDER BY id DESC").fetchone()
    assert mig and mig[0] == "undone", f"迁移状态应为 undone：{mig}"
    c.close()
    print("verify-undone ok：10 份逐字节还原 / 归档副本已删 / 隔离区空 / 状态回 confirmed / 迁移标记 undone")


if __name__ == "__main__":
    cmd = sys.argv[1] if len(sys.argv) > 1 else ""
    {"seed": cmd_seed, "snap": cmd_snap,
     "verify-migrated": cmd_verify_migrated, "verify-undone": cmd_verify_undone}.get(
        cmd, lambda: print("用法：seed | snap | verify-migrated | verify-undone")
    )()
