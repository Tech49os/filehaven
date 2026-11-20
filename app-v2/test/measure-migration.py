"""待迁移页「文件多了排版错乱」的量化验证（CDP，隔离库）。

跑法：python test/measure-migration.py
    - 造 6 个目录 / 42 份已确认文件（其中 12 份带改名）
    - 开一个独立数据目录的实例，进「待迁移」
    - 量三件事：
      1) .groups 有没有被压扁（clientHeight 与 scrollHeight 差多少）
      2) 每一行的体积/状态列左边界是不是同一个像素（列有没有错位）
      3) 底部固定条有没有压住最后几行（rect 相交）
    - 顶/底各截一张图，人眼复核
"""
import os
import sqlite3
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import cdp  # noqa: E402

APP = 'D:/filemanager/发布/文件管家_v0.3.0-alpha.5.exe'
STAMP = time.strftime('%H%M%S')
DATA = Path(f'D:/filemanager/.e2e-data-{STAMP}')
# 自己的语料目录：42 份造出来的文件不能倒进 e2e 共用的 .e2e-src，
# 否则 e2e 里「扫到 13 份」那条会被这堆垃圾文件搞挂
MG_SRC = Path(f'D:/filemanager/.e2e-src-mg-{STAMP}')

DIRS = [
    '安全与消防/检查报告',
    '安全与消防/演练记录',
    '采购合同/2026年',
    '财务报表/月度',
    '生产智能项目/工作汇报',
    '人事培训/课件',
]
PER_DIR = 7


def make_docx(p: Path, text: str):
    import zipfile
    with zipfile.ZipFile(p, 'w', zipfile.ZIP_DEFLATED) as z:
        z.writestr(
            'word/document.xml',
            '<w:document xmlns:w="x"><w:body><w:p><w:r><w:t>'
            + text
            + '</w:t></w:r></w:p></w:body></w:document>',
        )


def fill(db: Path):
    conn = sqlite3.connect(db)
    now = int(time.time() * 1000)
    ids = []
    for rel in DIRS:
        name = rel.rsplit('/', 1)[-1]
        depth = rel.count('/') + 1
        conn.execute(
            'INSERT OR IGNORE INTO archive_dirs(name, parent_id, rel_path, depth, origin, created_at) '
            'VALUES(?,NULL,?,?,?,?)',
            (name, rel, depth, 'user', now),
        )
        ids.append(conn.execute('SELECT id FROM archive_dirs WHERE rel_path=?', (rel,)).fetchone()[0])

    src = MG_SRC
    src.mkdir(parents=True, exist_ok=True)
    conn.execute('UPDATE source_roots SET path=?', (str(src),))
    n = 0
    for gi, dir_id in enumerate(ids):
        for k in range(PER_DIR):
            n += 1
            name = f'{"IMG_%04d" % n if n % 4 == 0 else "工作记录%02d" % n}.docx'
            abs_path = src / name
            # 必须是真能在盘上对上的文件：库里 size/mtime 和磁盘不一致时，
            # 预览会把它们判成「标注后被改」直接拦下来，就测不到列表排版了
            make_docx(abs_path, f'第 {n} 份巡检记录')
            st = abs_path.stat()
            size, mtime = int(st.st_size), int(st.st_mtime * 1000)
            conn.execute(
                'INSERT OR IGNORE INTO files'
                '(source_root_id, abs_path, rel_path, name, ext, kind, size, mtime, status,'
                ' defer_count, target_name, discovered_at, last_seen_at)'
                ' VALUES(1,?,?,?,?,?,?,?,?,0,?,?,?)',
                (
                    str(abs_path),
                    str(src),
                    name,
                    'docx',
                    'word',
                    size,
                    mtime,
                    'confirmed',
                    # 每 4 份里挑一份改过名，专门撞「多一格」这个坑
                    (f'配电箱巡检记录_{n:02d}.docx' if n % 4 == 0 else None),
                    mtime,
                    mtime,
                ),
            )
            file_id = conn.execute('SELECT id FROM files WHERE abs_path=?', (str(abs_path),)).fetchone()[0]
            conn.execute(
                'INSERT INTO assignments(file_id, batch_id, target_dir_id, origin, confidence, reason,'
                ' decision, decided_at) VALUES(?,NULL,?,?,?,?,?,?)',
                (file_id, dir_id, 'recommended', 0.9, '按关键词匹配', 'accepted', now),
            )
    conn.commit()
    conn.close()
    print(f'注入 {n} 份已确认文件 / {len(DIRS)} 个目录')


def measure(p) -> dict:
    return p.eval(
        """(() => {
      const mg = document.querySelector('.scroll');
      const g = document.querySelector('.groups');
      // 侧栏底部那块也叫 .foot，必须先框定在待迁移页自己的固定条上
      const foot = document.querySelector('.mg > .foot');
      const rows = Array.from(document.querySelectorAll('.frow'));
      const lefts = (sel) => Array.from(new Set(rows.map(r => {
        const e = r.querySelector(sel);
        return e ? Math.round(e.getBoundingClientRect().left) : -1;
      })));
      const sizes = Array.from(new Set(rows.map(r => {
        const e = r.querySelector('.fsize');
        return e ? Math.round(e.getBoundingClientRect().width) : -1;
      })));
      const fr = foot ? foot.getBoundingClientRect() : null;
      const scRect = mg ? mg.getBoundingClientRect() : null;
      const lastRow = rows[rows.length - 1];
      const lr = lastRow ? lastRow.getBoundingClientRect() : null;
      return {
        rows: rows.length,
        mgClient: mg ? Math.round(mg.clientHeight) : -1,
        mgScroll: mg ? Math.round(mg.scrollHeight) : -1,
        groupsClient: g ? Math.round(g.clientHeight) : -1,
        groupsScroll: g ? Math.round(g.scrollHeight) : -1,
        fsizeLefts: lefts('.fsize'),
        fstLefts: lefts('.fst'),
        fsizeWidths: sizes,
        // 底部固定条顶边 vs 最后一行底边：正常应该 footer.top >= lastRow.bottom
        footTop: fr ? Math.round(fr.top) : -1,
        mgBottom: mg ? Math.round(mg.getBoundingClientRect().bottom) : -1,
        scrollBottom: scRect ? Math.round(scRect.bottom) : -1,
        lastRowBottom: lr ? Math.round(lr.bottom) : -1,
        footOverlap: !!(fr && lr && lr.bottom > fr.top + 1),
      };
    })()"""
    )


def main():
    try:
        p = Path(APP)
        if not p.exists():
            raise SystemExit(f'找不到 {APP}')
        env = dict(os.environ)
        env['FILEBUTLER_DATA_DIR'] = str(DATA)
        env['WEBVIEW2_USER_DATA_FOLDER'] = f'D:/filemanager/.e2e-wv2-{STAMP}'
        env['WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS'] = '--remote-debugging-port=9222'

        subprocess.run([sys.executable, 'test/e2e-p1fix.py', 'seed'], check=True, env=env, cwd='.')
        fill(DATA / 'index.db')

        proc = subprocess.Popen([APP], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            cdp.wait_port(40)
            pg = cdp.Page()
            deadline = time.time() + 40
            while time.time() < deadline:
                if '资料库' in pg.text() or '整理台' in pg.text():
                    break
                time.sleep(0.5)
            time.sleep(1.0)
            pg.eval("location.hash = '#/migration'")
            time.sleep(2.5)

            top = measure(pg)
            print('顶部:', top)
            pg.screenshot('D:/filemanager/app-v2/test/shot-mg-top.png')

            # 滚到底再看一次：压扁/重叠如果存在，到底部最明显
            pg.eval("document.querySelector('.scroll').scrollTop = 99999")
            time.sleep(0.8)
            bottom = measure(pg)
            print('底部:', bottom)
            pg.screenshot('D:/filemanager/app-v2/test/shot-mg-bottom.png')

            flat = top['groupsClient'] >= top['groupsScroll'] - 1
            aligned = (
                len(bottom['fsizeLefts']) == 1
                and len(bottom['fstLefts']) == 1
                and len(bottom['fsizeWidths']) == 1
            )
            print()
            print(('  PASS  ' if top['rows'] >= 40 else '  FAIL  ') + f"渲染出 {top['rows']} 行")
            print(('  PASS  ' if flat else '  FAIL  ')
                  + f"分组没被压扁（client {top['groupsClient']} / scroll {top['groupsScroll']}）")
            print(('  PASS  ' if aligned else '  FAIL  ')
                  + f"体积/状态列对齐（fsize@{bottom['fsizeLefts']} fst@{bottom['fstLefts']} 宽{top['fsizeWidths']}）")
            print(('  PASS  ' if not bottom['footOverlap'] else '  FAIL  ')
                  + f"底部固定条没压住列表（foot.top {bottom['footTop']} vs 末行底 {bottom['lastRowBottom']}）")
            print(('  PASS  ' if bottom['footTop'] >= bottom['mgBottom'] else '  FAIL  ')
                  + f"固定条在滚动区下方（foot.top {bottom['footTop']} vs 滚动区底 {bottom['mgBottom']}）")
            print(('  PASS  ' if bottom['mgScroll'] > bottom['mgClient'] else '  FAIL  ')
                  + f"整页可滚动（{bottom['mgClient']} / {bottom['mgScroll']}）")
        finally:
            subprocess.run(['taskkill', '/F', '/PID', str(proc.pid)], capture_output=True)
    finally:
        pass


if __name__ == '__main__':
    main()
