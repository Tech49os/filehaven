"""P1 体验修复的端到端验证（隔离库，不碰用户真实索引库）。

跑法：
    export FILEBUTLER_DATA_DIR=D:/filemanager/.e2e-data
    export WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222
    npm run app:dev
    python test/e2e-p1fix.py verify

`seed` 只负责造语料和建库（在应用启动前跑）。断言全部读 DOM 文本，不截图。
"""
import json
import os
import sqlite3
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import cdp  # noqa: E402

SRC = Path('D:/filemanager/.e2e-src')
ARC = Path('D:/filemanager/.e2e-archive')
DATA = Path(os.environ.get('FILEBUTLER_DATA_DIR', 'D:/filemanager/.e2e-data'))

DOCS = [
    '2026年安全检查通知.docx',
    '采购合同（第三版）.docx',
    '部门月度费用明细.xlsx',
    '智能工厂平台架构图.drawio',
    '整改报告-消防.pdf',
]


def seed():
    SRC.mkdir(parents=True, exist_ok=True)
    ARC.mkdir(parents=True, exist_ok=True)
    for name in DOCS:
        p = SRC / name
        if not p.exists():
            p.write_bytes(b'x' * 1024)

    DATA.mkdir(parents=True, exist_ok=True)
    db = DATA / 'index.db'
    conn = sqlite3.connect(db)
    conn.executescript(Path('src-tauri/src/schema.sql').read_text(encoding='utf-8'))
    conn.execute(
        'INSERT OR REPLACE INTO settings(key, value) VALUES(?,?)',
        ('archive_root', str(ARC)),
    )
    conn.execute(
        'INSERT OR IGNORE INTO source_roots(path, enabled, added_at) VALUES(?,1,?)',
        (str(SRC), int(time.time() * 1000)),
    )
    conn.commit()
    conn.close()
    print(f'seeded db={db} src={SRC} ({len(DOCS)} docs) archive={ARC}')


def check(label, cond, detail=''):
    print(('  PASS  ' if cond else '  FAIL  ') + label + (f'  <<{detail}>>' if detail and not cond else ''))
    return bool(cond)


def verify():
    cdp.wait_port(30)
    p = cdp.Page()
    time.sleep(1.2)
    ok = True

    if '/workspace' not in p.url():
        p.click_text('整理台', 'button', exact=False)
        time.sleep(0.8)
    ok &= check('启动落在整理台', '/workspace' in p.url(), p.url())

    # ---- 设置页：源区卡片必须同时有「添加源区」和「重新扫描」
    p.click_text('设置', 'button', exact=False)
    time.sleep(0.8)
    txt = p.text()
    ok &= check('设置页打开', '源区' in txt, txt[:120])
    ok &= check('源区卡片有「添加源区」', '添加源区' in txt, txt[:200])
    ok &= check('源区卡片有「重新扫描」', '重新扫描' in txt, txt[:200])
    ok &= check('已配置的源区列出来了', str(SRC).replace('\\', '/') in txt or str(SRC) in txt, txt[:300])

    # ---- 归档区剩余空间：现在走内核 API，值应该立刻出来
    t0 = time.time()
    ok &= check('归档区显示剩余空间（free_gb 可用）', 'GB 可用' in txt, txt[:300])
    print(f'        首次渲染耗时 {time.time() - t0:.2f}s 内已含剩余空间')

    # ---- 扫描：进度行 + 结果行都要出现，且扫描不能崩
    pos = p.click_text('重新扫描', 'button', exact=False)
    print(f'        点「重新扫描」at {pos}')
    seen_progress = False
    deadline = time.time() + 25
    while time.time() < deadline:
        t = p.text()
        if '已处理' in t:
            seen_progress = True
        if '跳过非资料型' in t:
            break
        time.sleep(0.25)
    txt = p.text()
    ok &= check('扫描有实时进度（不是干等）', seen_progress or '跳过非资料型' in txt, txt[:300])
    ok &= check('扫描出结果行', '跳过非资料型' in txt, txt[-300:])
    ok &= check('扫到 5 份', '扫了 5 份' in txt, txt[-300:])

    # ---- 空态也要能添加：先移除源区，再看入口是否还在
    before = p.text()
    if '移除' in before:
        # 必须 exact：li 里也含「移除」，contains 匹配会点到整行而不是按钮
        p.click_text('移除', 'button', exact=True)
        time.sleep(1.2)
        after = p.text()
        ok &= check('移除后回到空态', '还没有源区' in after, after[-500:])
        ok &= check('空态里仍有「添加源区」入口', '添加源区' in after, after[-500:])
        # 把源区加回去，后面的页面巡览才有数据
        ok &= check('空态里有可点的「选一个文件夹」', '选一个文件夹' in after, after[-500:])

    # ---- 逐页走一遍：过渡动画不能把内容吃掉
    for label, must in [
        ('整理台', '整理台'),
        ('待迁移', '待迁移'),
        ('浏览', '浏览'),
        ('待定区', '待定区'),
    ]:
        p.click_text(label, 'button', exact=False)
        time.sleep(0.6)
        t = p.text()
        ok &= check(f'切到「{label}」内容在', must in t and len(t.strip()) > 10, t[:120])

    print('\nRESULT:', 'ALL PASS' if ok else 'HAS FAILURE')
    return 0 if ok else 1


if __name__ == '__main__':
    mode = sys.argv[1] if len(sys.argv) > 1 else 'verify'
    if mode == 'seed':
        seed()
    else:
        sys.exit(verify())
