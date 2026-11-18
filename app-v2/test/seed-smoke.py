import sqlite3, os, time

db = os.path.join(os.environ['APPDATA'], 'com.filebutler.app', 'index.db')
bak = db + '.smoke-bak'
if not os.path.exists(bak):
    import shutil
    shutil.copy2(db, bak)

c = sqlite3.connect(db)
c.execute("PRAGMA journal_mode=WAL")
now = int(time.time() * 1000)
for t in ('assignments', 'files', 'archive_dirs', 'source_roots', 'batches'):
    c.execute(f"DELETE FROM {t}")

c.execute("INSERT INTO settings(key,value) VALUES('archive_root','D:/文档归档') "
          "ON CONFLICT(key) DO UPDATE SET value=excluded.value")
c.execute("INSERT INTO source_roots(path,enabled,added_at) VALUES('C:/Users/kingb/Desktop',1,?)", (now,))
root = c.execute("SELECT id FROM source_roots").fetchone()[0]


def mkdir(parent, name, rel, depth, origin='user'):
    cur = c.execute("INSERT INTO archive_dirs(parent_id,name,rel_path,depth,origin,created_at) "
                    "VALUES(?,?,?,?,?,?)", (parent, name, rel, depth, origin, now))
    return cur.lastrowid


mkdir(None, '安全检查', '安全检查', 1)
d2 = mkdir(None, '合同', '合同', 1)
mkdir(d2, '采购', '合同/采购', 2)

names = [
    '2026年第一季度安全生产检查通知.docx', '消防设施年度检测报告.pdf', '安全检查整改记录表.xlsx',
    '采购合同-办公用品-2026.docx', '供应商报价单.xlsx', '采购申请单2026-03.docx',
    '会议纪要-11月第2周.docx', '员工培训计划2026.xlsx', '公司简介PPT.pptx',
    '办公室平面图.drawio', '扫描件_20261112.pdf', '哈希名_a1b2c3d4.docx',
]
kind_of = {'docx': 'word', 'pdf': 'pdf', 'xlsx': 'excel', 'pptx': 'ppt', 'drawio': 'diagram'}
for i, n in enumerate(names):
    ext = n.rsplit('.', 1)[-1].lower()
    c.execute(
        """INSERT INTO files(source_root_id,abs_path,rel_path,name,ext,kind,size,mtime,
                             tag_level,status,defer_count,discovered_at,last_seen_at,missing)
           VALUES(?,?,?,?,?,?,?,?,'basic','pending_confirm',0,?,?,0)""",
        (root, 'C:/Users/kingb/Desktop/' + n, 'C:/Users/kingb/Desktop/' + n, n, ext,
         kind_of[ext], 1024 * (i + 5), now - i * 86400000, now, now),
    )

c.commit()
print('dirs', c.execute("SELECT COUNT(*) FROM archive_dirs").fetchone()[0],
      'files', c.execute("SELECT COUNT(*) FROM files").fetchone()[0],
      'root', root)
c.close()
