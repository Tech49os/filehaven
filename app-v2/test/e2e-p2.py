"""P2 端到端验证（隔离库，不碰用户真实索引库）。

跑法（与 P1 同一套手法）：
    export FILEBUTLER_DATA_DIR=D:/filemanager/.e2e2-data
    export WEBVIEW2_USER_DATA_FOLDER=D:/filemanager/.e2e2-webview
    export WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223
    python test/e2e-p2.py seed      # 应用启动前跑
    <启动 exe>
    python test/e2e-p2.py verify

覆盖 P2 的五块：
  M2 深度标签    —— summary 显示、标签等级、队列进度在设置页
  M3 增量与监听  —— 丢一份新文件进源区，侧栏要出「+N 份新文件」；last_scan_at 有写入
  M4 待定区闭环  —— 列出 / 主推直接指定 / 捞回 / 移出
  M5 视觉收口    —— --c-text-3 对比度 ≥4.5、120 字文件名不换行、路径中间省略
  迁移守卫       —— 迁走源文件后，监听不许把它判成 missing（T3.5 真集成）
"""
import json
import os
import shutil
import sqlite3
import sys
import time
import zipfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import cdp  # noqa: E402

SRC = Path('D:/filemanager/.e2e2-src')
ARC = Path('D:/filemanager/.e2e2-archive')
DATA = Path(os.environ.get('FILEBUTLER_DATA_DIR', 'D:/filemanager/.e2e2-data'))

LONG_NAME = (
    '2026年度智能工厂平台安全生产大检查暨消防设施专项验收工作情况汇报'
    '（第三次修订版）最终定稿v3.docx'
)

# (文件名, 状态, tag_level, summary)
SEED_DOCS = [
    ('安全检查通知.docx', 'parked', 'deep', '2026 年度安全检查通知，要求各部门 11 月底前自查上报。'),
    ('采购合同-服务器.docx', 'parked', 'basic', None),
    ('培训签到表.docx', 'parked', 'basic', None),
    ('消防验收报告.docx', 'confirmed', 'deep', '消防设施专项验收结论与整改清单。'),
    ('月度例会纪要.docx', 'labeled', 'deep', '九月份例会：生产、质量、安全三条线的待办。'),
    (LONG_NAME, 'labeled', 'basic', None),
]


def make_docx(p: Path, text: str):
    with zipfile.ZipFile(p, 'w', zipfile.ZIP_DEFLATED) as z:
        z.writestr(
            'word/document.xml',
            '<w:document xmlns:w="x"><w:body><w:p><w:r><w:t>'
            + text
            + '</w:t></w:r></w:p></w:body></w:document>',
        )


def seed():
    if SRC.exists():
        shutil.rmtree(SRC, ignore_errors=True)
    if ARC.exists():
        shutil.rmtree(ARC, ignore_errors=True)
    SRC.mkdir(parents=True, exist_ok=True)
    ARC.mkdir(parents=True, exist_ok=True)
    # 故意埋在两层目录里：卡片上「位置」那一栏才会真的需要中间省略
    docs_dir = SRC / '2026年度综合管理资料' / '安全与消防'
    docs_dir.mkdir(parents=True, exist_ok=True)
    for name, _, _, summary in SEED_DOCS:
        make_docx(docs_dir / name, summary or '本文件用于 P2 端到端验证，正文略。')

    DATA.mkdir(parents=True, exist_ok=True)
    db = DATA / 'index.db'
    conn = sqlite3.connect(db)
    conn.executescript(Path('src-tauri/src/schema.sql').read_text(encoding='utf-8'))
    now = int(time.time() * 1000)
    setk = 'INSERT OR REPLACE INTO settings(key, value) VALUES(?,?)'
    conn.execute(setk, ('archive_root', str(ARC)))
    conn.execute(setk, ('ollama_model', 'qwen3.5:4b'))
    # 深度解读关掉：e2e 不该依赖模型在跑；summary 由 seed 直接写进库
    conn.execute(setk, ('deep_tag_enabled', '0'))
    conn.execute(setk, ('watch_enabled', '1'))
    conn.execute(
        'INSERT OR IGNORE INTO source_roots(path, enabled, added_at) VALUES(?,1,?)',
        (str(SRC), now),
    )
    root_id = conn.execute('SELECT id FROM source_roots LIMIT 1').fetchone()[0]

    for name, status, level, summary in SEED_DOCS:
        abs_path = str(docs_dir / name)  # 与 scan::meta_of 一致（Windows 下是反斜杠），否则会被当成两份
        conn.execute(
            'INSERT OR IGNORE INTO files'
            '(source_root_id, abs_path, rel_path, name, ext, kind, size, mtime,'
            ' tag_level, status, defer_count, summary, discovered_at, last_seen_at, missing)'
            ' VALUES(?,?,?,?,?,?,?,?,?,?,0,?,?,?,0)',
            (
                root_id,
                abs_path,
                name,
                name,
                'docx',
                'word',
                (docs_dir / name).stat().st_size,
                now - 86_400_000,
                level,
                status,
                summary,
                now,
                now,
            ),
        )

    # 两个目录：一个与「安全检查」沾边，一个远一些 —— 待定区要能算出主推
    for rel in ['安全环保/检查报告', '人事培训']:
        conn.execute(
            'INSERT OR IGNORE INTO archive_dirs(name, parent_id, rel_path, depth, origin, created_at)'
            ' VALUES(?,NULL,?,?,?,?)',
            (rel.split('/')[-1], rel, rel.count('/') + 1, 'user', now),
        )

    # 给「安全环保/检查报告」塞两份已确认文件：主推要算得出来（单测里靠这个）
    dir_id = conn.execute(
        "SELECT id FROM archive_dirs WHERE rel_path='安全环保/检查报告'"
    ).fetchone()[0]
    hist = SRC / '2025年归档'
    hist.mkdir(parents=True, exist_ok=True)
    for i, nm in enumerate(['2025年度安全检查报告.docx', '消防检查记录.docx']):
        make_docx(hist / nm, '历史安全检查记录，用于证明主推算得出来、迁移搬得走。')
        abs_path = str(hist / nm)
        conn.execute(
            'INSERT OR IGNORE INTO files'
            '(source_root_id, abs_path, rel_path, name, ext, kind, size, mtime,'
            ' tag_level, status, defer_count, discovered_at, last_seen_at, missing)'
            ' VALUES(?,?,?,?,?,?,?,?,?,?,0,?,?,0)',
            (root_id, abs_path, nm, nm, 'docx', 'word', 1024, now, 'basic', 'confirmed', now, now),
        )
        fid = conn.execute('SELECT id FROM files WHERE abs_path=?', (abs_path,)).fetchone()[0]
        conn.execute(
            'INSERT INTO assignments(file_id, batch_id, target_dir_id, origin, confidence,'
            ' reason, decision, decided_at) VALUES(?,NULL,?,?,?,?,?,?)',
            (fid, dir_id, 'user_picked', None, '验证用历史数据', 'accepted', now),
        )

    conn.commit()
    conn.close()
    print(f'seeded db={db} src={SRC} ({len(SEED_DOCS)} docs) archive={ARC}')


def check(label, cond, detail=''):
    print(('  PASS  ' if cond else '  FAIL  ') + label + (f'  <<{detail}>>' if detail and not cond else ''))
    return bool(cond)


def ratio(hex_a: str, hex_b: str) -> float:
    def lum(h):
        h = h.strip().lstrip('#')
        r, g, b = (int(h[i:i + 2], 16) / 255 for i in (0, 2, 4))
        f = lambda c: c / 12.92 if c <= 0.03928 else ((c + 0.055) / 1.055) ** 2.4  # noqa: E731
        return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b)

    a, b = lum(hex_a), lum(hex_b)
    hi, lo = max(a, b), min(a, b)
    return (hi + 0.05) / (lo + 0.05)


def js_click(p, label, tag='button'):
    """精确点某个按钮：按文字找 button，scrollIntoView 后直接 .click()。

    不能用 cdp.click_text —— 它的选择器里带了 `li`，待定区一行本身就是一个 li，
    「主推」会被整行抢走，点行中心等于什么都没点（本轮就是这么假红的）。
    """
    js = (
        "((label)=>{const bs=[...document.querySelectorAll('%s')];"
        "const t=e=>(e.textContent||'').replace(/\\s+/g,'');"
        "const b=bs.find(x=>t(x).includes(label.replace(/\\s+/g,'')));"
        "if(!b)return 'none';b.scrollIntoView({block:'center'});b.click();return 'ok';})(%s)"
        % (tag, json.dumps(label))
    )
    return p.eval(js) == 'ok'


def click_until_gone(p, label, text, tries=3):
    """点一个按钮直到目标行消失。

    为什么要重试：动作按钮在上一笔写库 + 刷计数期间是 disabled 的，
    这时点下去是空点（Vue 不会触发 handler）。给几次机会比放宽断言更实在。
    """
    for _ in range(tries):
        js_click(p, label)
        if wait_gone(p, text, 3.0):
            return True
    return False


def wait_gone(p, text, timeout=8.0):
    """等某段文字从页面上消失。UI 的「点完 → 后端写完 → 刷计数 → 重渲染」是异步的，
    点完立刻断言会把正常行为判成失败。"""
    deadline = time.time() + timeout
    while time.time() < deadline:
        if text not in p.text():
            return True
        time.sleep(0.3)
    return False


def goto(p, label):
    p.click_text(label, 'button', exact=False)
    time.sleep(0.7)


def go_workspace(p):
    if '/workspace' not in p.url():
        p.eval("location.hash = '#/workspace'")
        time.sleep(1.0)


def verify():
    cdp.wait_port(30)
    p = cdp.Page()
    deadline = time.time() + 40
    while time.time() < deadline:
        if '资料库' in p.text() or '引导' in p.text():
            break
        time.sleep(0.5)
    time.sleep(1.2)
    ok = True

    # ------------------------------------------------ M3：启动增量扫过一遍
    db = DATA / 'index.db'
    conn = sqlite3.connect(f'file:{db}?mode=ro', uri=True)
    row = conn.execute("SELECT value FROM settings WHERE key='last_scan_at'").fetchone()
    ok &= check('启动增量扫描留下了 last_scan_at（10 分钟节流靠它）', row is not None)
    conn.close()

    # ------------------------------------------------ M4：待定区
    go_workspace(p)
    txt = p.text()
    ok &= check('侧栏有待定区入口且角标为 3', '待定区' in txt and '3' in txt, txt[:200])

    goto(p, '待定区')
    txt = p.text()
    ok &= check('待定区列出 3 份', txt.count('parked') >= 0 and '检查通知' in txt, txt[:400])
    ok &= check('读过的显示 summary', '11 月底前自查上报' in txt, txt[:400])
    ok &= check('没读过的写「未解读」，不留空占位', '未解读' in txt, txt[:400])
    ok &= check('主推目录与理由都在', '安全环保/检查报告' in txt, txt[:400])

    # 主推 = 直接指定：行消失 + 待迁移数 +1
    before = p.text()
    ok &= check('找到「主推」按钮', js_click(p, '主推'))
    time.sleep(1.6)
    after = p.text()
    ok &= check(
        '点主推后该行消失',
        after.count('安全检查通知.docx') < before.count('安全检查通知.docx')
        or wait_gone(p, '安全检查通知.docx'),
        p.text()[:300],
    )

    # 捞回 / 移出
    ok &= check('捞回后行消失', click_until_gone(p, '捞回', '采购合同-服务器.docx'), p.text()[:300])
    ok &= check('移出后行消失', click_until_gone(p, '移出', '培训签到表.docx'), p.text()[:300])
    time.sleep(0.6)
    last = p.text()
    ok &= check('全部处理完回到中性空态', '这里现在是空的' in last, last[:300])
    ok &= check('空态不催办（没有「待处理」字样）', '待处理' not in last and '未处理' not in last, last[:300])

    # ------------------------------------------------ M2：卡片上的 summary 与标签等级
    go_workspace(p)
    goto(p, '整理台')
    txt = p.text()
    ok &= check('整理台卡片显示深度解读的一句说明', '九月份例会' in txt or '消防设施专项验收' in txt, txt[:500])

    # ------------------------------------------------ M5：对比度与截断
    css = p.eval(
        "getComputedStyle(document.documentElement).getPropertyValue('--c-text-3').trim()"
    )
    ok &= check('--c-text-3 换成了更深的值', css.lower() == '#6e6e73', str(css))
    if css.startswith('#'):
        r = ratio(css, '#f5f5f7')
        ok &= check(f'--c-text-3 对窗口背景 {r:.2f}:1 ≥ 4.5（WCAG AA 小字）', r >= 4.5, f'{r:.2f}')

    h = p.eval(
        "(()=>{const e=document.querySelector('.name');"
        "return e?e.getBoundingClientRect().height:0})()"
    )
    ok &= check('120 字文件名仍是单行（高度 < 26px）', 0 < float(h) < 26, f'{h}px')

    path_txt = p.eval(
        "(()=>{const e=document.querySelector('.pval');return e?e.textContent:''})()"
    )
    ok &= check('长路径中间省略（含 …）', '…' in str(path_txt), str(path_txt))

    # ------------------------------------------------ M3：监听真的在盯
    drop = SRC / '刚丢进来的新文件.docx'
    make_docx(drop, '这是运行期丢进源区的文件，监听应当几秒内发现。')
    found = False
    deadline = time.time() + 15
    while time.time() < deadline:
        if '份新文件' in p.text():
            found = True
            break
        time.sleep(0.5)
    ok &= check('丢一份新文件进源区，侧栏 15s 内出「+N 份新文件」', found, p.text()[:300])

    # ------------------------------------------------ 迁移守卫（T3.5 真集成）
    goto(p, '待迁移')
    if '应用迁移' in p.text():
        js_click(p, '应用迁移')
        time.sleep(1.0)
        if '确认迁移' in p.text():
            js_click(p, '确认迁移')
        deadline = time.time() + 40
        while time.time() < deadline:
            if '迁移完成' in p.text() or '无可迁移' in p.text():
                break
            time.sleep(1.0)
        conn = sqlite3.connect(f'file:{db}?mode=ro', uri=True)
        miss = conn.execute("SELECT COUNT(*) FROM files WHERE status='missing'").fetchone()[0]
        migrated = conn.execute(
            "SELECT COUNT(*) FROM files WHERE status='migrated'"
        ).fetchone()[0]
        conn.close()
        ok &= check('迁移真的搬走了文件', migrated > 0, str(migrated))
        ok &= check('监听没把刚迁移的文件判成 missing（T3.5）', miss == 0, str(miss))
    else:
        ok &= check('待迁移页有可迁移内容', False, p.text()[:300])

    # ------------------------------------------------ 设置页：深度解读 + 新文件监听
    goto(p, '设置')
    txt = p.text()
    ok &= check('设置页有「深度解读」卡片', '深度解读' in txt, txt[:300])
    ok &= check('深度解读显示完成/剩余', '完成' in txt and '还剩' in txt, txt[:300])
    ok &= check('设置页有「新文件监听」卡片', '新文件监听' in txt, txt[:300])
    ok &= check('监听开关显示当前状态', '正在盯着源区' in txt, txt[:300])
    if '暂停监听' in txt:
        js_click(p, '暂停监听')
        time.sleep(1.2)
        ok &= check('点「暂停监听」后变成已关闭', '已关闭' in p.text(), p.text()[:300])

    print('\nRESULT:', 'ALL PASS' if ok else 'HAS FAILURE')
    return 0 if ok else 1


if __name__ == '__main__':
    cmd = sys.argv[1] if len(sys.argv) > 1 else 'verify'
    if cmd == 'seed':
        seed()
    elif cmd == 'verify':
        sys.exit(verify())
    else:
        raise SystemExit(f'未知子命令：{cmd}')
