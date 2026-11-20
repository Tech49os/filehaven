"""启动增量 + 10 分钟节流的真机验证（P2 验收 6）。

这一条必须**启两次应用**才能验，所以单独成脚本，由 run-e2e.py 的 `p2r` 阶段调用：

    第一次启动：源区里已有 5 份文件 → 10s 内应全部入池（files 表 5 行）
    立刻第二次启动：`last_scan_at` 不许变（10 分钟内不重扫）

跑法（一般不用手敲，run-e2e 会带 exe 路径调进来）：
    python test/e2e-p2-restart.py <exe 路径>
"""
import os
import sqlite3
import subprocess
import sys
import time
import zipfile
from pathlib import Path

SRC = Path('D:/filemanager/.e2e2-src-restart')
ARC = Path('D:/filemanager/.e2e2-archive-restart')
DATA = Path(os.environ.get('FILEBUTLER_DATA_DIR', 'D:/filemanager/.e2e-p2r-data'))

N = 5


def make_docx(p: Path, text: str):
    with zipfile.ZipFile(p, 'w', zipfile.ZIP_DEFLATED) as z:
        z.writestr(
            'word/document.xml',
            '<w:document xmlns:w="x"><w:body><w:p><w:r><w:t>' + text + '</w:t></w:r></w:p></w:body></w:document>',
        )


def check(label, cond, detail=''):
    print(('  PASS  ' if cond else '  FAIL  ') + label + (f'  <<{detail}>>' if detail and not cond else ''))
    return bool(cond)


def seed():
    SRC.mkdir(parents=True, exist_ok=True)
    ARC.mkdir(parents=True, exist_ok=True)
    sub = SRC / '待归档资料'
    sub.mkdir(exist_ok=True)
    for i in range(N):
        make_docx(sub / f'启动增量验收-{i + 1}.docx', f'第 {i + 1} 份，用于验证启动时自动补新。')

    DATA.mkdir(parents=True, exist_ok=True)
    db = DATA / 'index.db'
    if db.exists():
        db.unlink()
    conn = sqlite3.connect(db)
    conn.executescript(Path('src-tauri/src/schema.sql').read_text(encoding='utf-8'))
    now = int(time.time() * 1000)
    conn.execute('INSERT OR REPLACE INTO settings(key, value) VALUES(?,?)', ('archive_root', str(ARC)))
    conn.execute('INSERT OR REPLACE INTO settings(key, value) VALUES(?,?)', ('deep_tag_enabled', '0'))
    conn.execute('INSERT OR REPLACE INTO settings(key, value) VALUES(?,?)', ('watch_enabled', '1'))
    conn.execute(
        'INSERT OR IGNORE INTO source_roots(path, enabled, added_at) VALUES(?,1,?)',
        (str(SRC), now),
    )
    conn.commit()
    conn.close()
    print(f'seeded db={db} src={SRC}（{N} 份，零索引行）')


def q(sql, *args):
    conn = sqlite3.connect(f'file:{DATA / "index.db"}?mode=ro', uri=True)
    try:
        return conn.execute(sql, args).fetchone()
    finally:
        conn.close()


def launch(exe: str, env) -> subprocess.Popen:
    p = subprocess.Popen([exe], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(3)
    return p


def kill(p: subprocess.Popen):
    subprocess.run(['taskkill', '/F', '/PID', str(p.pid)], capture_output=True)
    time.sleep(1.5)


def main() -> int:
    exe = sys.argv[1]
    env = dict(os.environ)
    env['FILEBUTLER_DATA_DIR'] = str(DATA)
    env['WEBVIEW2_USER_DATA_FOLDER'] = str(DATA) + '-wv2'

    seed()
    ok = True

    # ---- 第一次启动：5 份新文件应当自动进池
    t0 = time.time()
    p = launch(exe, env)
    rows, elapsed = 0, 0.0
    try:
        deadline = time.time() + 30
        while time.time() < deadline:
            try:
                rows = q('SELECT COUNT(*) FROM files')[0]
            except sqlite3.Error:
                rows = 0
            if rows >= N:
                elapsed = time.time() - t0
                break
            time.sleep(0.5)
    finally:
        kill(p)
    ok &= check(f'启动后 10s 内 {N} 份全部入池', rows >= N and elapsed <= 10.5, f'{rows} 行 / {elapsed:.1f}s')

    first = q("SELECT value FROM settings WHERE key='last_scan_at'")
    ok &= check('第一次启动写了 last_scan_at', first is not None, str(first))

    # ---- 立刻第二次启动：10 分钟节流，不许再扫
    p = launch(exe, env)
    try:
        time.sleep(8)
    finally:
        kill(p)
    second = q("SELECT value FROM settings WHERE key='last_scan_at'")
    ok &= check(
        '10 分钟内二次启动不重扫（last_scan_at 没变）',
        first is not None and second is not None and first[0] == second[0],
        f'{first} → {second}',
    )
    # 节流跳过时也不该多插行（同一份文件被扫两遍会产生重复）
    rows2 = q('SELECT COUNT(*) FROM files')[0]
    ok &= check('二次启动没有重复插行', rows2 == rows, f'{rows} → {rows2}')

    print('\nRESULT:', 'ALL PASS' if ok else 'HAS FAILURE')
    return 0 if ok else 1


if __name__ == '__main__':
    sys.exit(main())
