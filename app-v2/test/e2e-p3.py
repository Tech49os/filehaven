"""P3 端到端验证（隔离库，不碰用户真实索引库）。

跑法（与 P1/P2 同一套手法，run-e2e.py 的 p3 段会自动喂环境变量）：
    python test/e2e-p3.py seed      # 应用启动前跑
    <启动 exe>
    python test/e2e-p3.py verify

覆盖 P3 的六块（T5.2）：
  浏览          —— 树/虚拟视图/未归类/目录内列表/详情条（archived_path 生效）
  检索          —— Ctrl+K 面板 / 高级筛选（AND·OR）/ 目录内筛选
  标签          —— 卡片 chips / 加用户标签 / 删标签 / 设置页词表
  AI 说明       —— 深读关着时「深度解读已关闭」+「去开启」（不依赖模型的确定行为）
  图片解读开关  —— 设置页开关行 + 计数 + 开关写库
  老数据回填    —— migration_items 反推 archived_path / archive_dir_id
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

SRC = Path('D:/filemanager/.e2e3-src')
ARC = Path('D:/filemanager/.e2e3-archive')
DATA = Path(os.environ.get('FILEBUTLER_DATA_DIR', 'D:/filemanager/.e2e3-data'))


def make_docx(p: Path, text: str):
    with zipfile.ZipFile(p, 'w', zipfile.ZIP_DEFLATED) as z:
        z.writestr(
            'word/document.xml',
            '<w:document xmlns:w="x"><w:body><w:p><w:r><w:t>'
            + text
            + '</w:t></w:r></w:p></w:body></w:document>',
        )


def seed():
    for d in (SRC, ARC, DATA):
        shutil.rmtree(d, ignore_errors=True)
        d.mkdir(parents=True, exist_ok=True)
    db = DATA / 'index.db'
    conn = sqlite3.connect(db)
    conn.executescript(Path('src-tauri/src/schema.sql').read_text(encoding='utf-8'))
    now = int(time.time() * 1000)
    setk = 'INSERT OR REPLACE INTO settings(key, value) VALUES(?,?)'
    conn.execute(setk, ('archive_root', str(ARC)))
    conn.execute(setk, ('deep_tag_enabled', '0'))  # AI 说明走「已关闭」分支，不依赖模型
    conn.execute(setk, ('deep_vision_enabled', '0'))  # 图片解读关着：设置页要显示「（已关）」
    conn.execute(setk, ('watch_enabled', '0'))
    conn.execute('INSERT OR IGNORE INTO source_roots(path, enabled, added_at) VALUES(?,1,?)', (str(SRC), now))
    root_id = conn.execute('SELECT id FROM source_roots LIMIT 1').fetchone()[0]

    def add_file(name, rel, text, status, level='basic', summary=None, missing=0):
        p = SRC / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        make_docx(p, text)
        conn.execute(
            'INSERT INTO files'
            '(source_root_id, abs_path, rel_path, name, ext, kind, size, mtime,'
            ' tag_level, status, defer_count, summary, discovered_at, last_seen_at, missing)'
            ' VALUES(?,?,?,?,?,?,?,?,?,?,0,?,?,?,?)',
            (
                root_id, str(p), rel, name, 'docx', 'word',
                p.stat().st_size, now, level, status, summary, now, now, missing,
            ),
        )
        return conn.execute('SELECT id FROM files WHERE abs_path=?', (str(p),)).fetchone()[0]

    # 整理池（待整理视图）
    fid_pool = add_file('项目验收报告.docx', '项目验收报告.docx', '项目验收结论与遗留问题清单。', 'labeled', 'deep', '项目验收结论与遗留问题清单。')
    fid_parked = add_file('神秘材料.docx', '神秘材料.docx', '看不出用途的材料。', 'parked')

    # 归档目录 + 已迁移文件（带标签）：浏览/检索的主语料
    d1 = conn.execute(
        'INSERT INTO archive_dirs(name, parent_id, rel_path, depth, origin, created_at)'
        " VALUES('检查报告',NULL,'安全环保/检查报告',2,'user',?)",
        (now,),
    ).lastrowid
    conn.execute(
        'INSERT INTO archive_dirs(name, parent_id, rel_path, depth, origin, created_at)'
        " VALUES('安全环保',NULL,'安全环保',1,'user',?)",
        (now,),
    )
    m1 = ARC / '安全环保' / '检查报告'
    m1.mkdir(parents=True, exist_ok=True)
    migrated = []
    for nm, txt in [
        ('2025年度安全检查报告.docx', '历史安全检查报告正文。'),
        ('消防检查记录.docx', '消防设施检查记录正文。'),
    ]:
        p = m1 / nm
        make_docx(p, txt)
        fid = conn.execute(
            'INSERT INTO files'
            '(source_root_id, abs_path, rel_path, name, ext, kind, size, mtime,'
            ' tag_level, status, defer_count, summary, discovered_at, last_seen_at, missing,'
            ' archived_path, archive_dir_id)'
            ' VALUES(?,?,?,?,?,?,?,?,?,?,0,?,?,?,0,?,?)',
            (
                root_id, str(SRC / '旧路径' / nm), '旧路径/' + nm, nm, 'docx', 'word',
                p.stat().st_size, now, 'deep', 'migrated', txt, now, now, str(p), d1,
            ),
        ).lastrowid
        migrated.append((fid, nm, str(p)))

    # 已迁移但目录被删过：archived_path 有、archive_dir_id NULL → 浏览页归「未归类」
    orphan = ARC / '无处安放'
    orphan.mkdir(parents=True, exist_ok=True)
    p = orphan / '无人认领报告.docx'
    make_docx(p, '目录被删掉的那份报告。')
    fid_orphan = conn.execute(
        'INSERT INTO files'
        '(source_root_id, abs_path, rel_path, name, ext, kind, size, mtime,'
        ' tag_level, status, defer_count, summary, discovered_at, last_seen_at, missing,'
        ' archived_path, archive_dir_id)'
        ' VALUES(?,?,?,?,?,?,?,?,?,?,0,?,?,?,0,?,NULL)',
        (
            root_id, str(SRC / '旧路径2' / p.name), '旧路径2/' + p.name, p.name, 'docx', 'word',
            p.stat().st_size, now, 'deep', 'migrated', '目录被删掉的那份报告。', now, now, str(p),
        ),
    ).lastrowid

    # 标签：安全检查（topic，贴在两份迁移件+池内件）、消防（topic 只贴一份）、重点跟进（user）
    tags = {}
    for name, kind in [('安全检查', 'topic'), ('消防', 'topic'), ('重点跟进', 'user')]:
        tid = conn.execute(
            'INSERT OR IGNORE INTO tags(name, kind) VALUES(?,?)', (name, kind)
        ).lastrowid
        tags[name] = tid
    def tag_file(fid, *names):
        for n in names:
            conn.execute('INSERT INTO file_tags(file_id, tag_id, confidence) VALUES(?,?,0.9)', (fid, tags[n]))
    tag_file(migrated[0][0], '安全检查', '消防', '重点跟进')   # 同时具备 AND 的那一份
    tag_file(migrated[1][0], '安全检查')
    tag_file(fid_pool, '安全检查')
    conn.commit()
    conn.close()
    print(f'seeded db={db} src={SRC} archive={ARC} tags={list(tags)}')


# ---------------------------------------------------------------- verify

def check(label, cond, detail=''):
    print(('  PASS  ' if cond else '  FAIL  ') + label + (f'  <<{detail}>>' if detail and not cond else ''))
    return bool(cond)


def js_ctxmenu(p, selector, label):
    """按文字找元素并派发 contextmenu 事件（右键菜单）。"""
    js = (
        "((sel,label)=>{const es=[...document.querySelectorAll(sel)];"
        "const t=e=>(e.textContent||'').replace(/\\s+/g,'');"
        "const e=es.filter(x=>t(x).includes(label)).sort((a,b)=>a.textContent.length-b.textContent.length)[0];"
        "if(!e)return 'none';"
        "const r=e.getBoundingClientRect();"
        "e.dispatchEvent(new MouseEvent('contextmenu',{bubbles:true,cancelable:true,"
        "clientX:Math.round(r.left+6),clientY:Math.round(r.top+6)}));return 'ok';}"
        ")(%s,%s)" % (json.dumps(selector), json.dumps(label))
    )
    return p.eval(js) == 'ok'


def js_click(p, selector, label):
    """按文字在 selector 圈定的范围内找元素并 .click()。

    取**最小**匹配（textContent 最短的）：容器元素的 textContent 也包含子元素文字，
    先匹配到容器的话 click 打在空气上（第一轮就是这么假红的）。
    """
    js = (
        "((sel,label)=>{const es=[...document.querySelectorAll(sel)];"
        "const t=e=>(e.textContent||'').replace(/\\s+/g,'');"
        "const hits=es.filter(x=>t(x).includes(label.replace(/\\s+/g,'')));"
        "if(!hits.length)return 'none';"
        "const e=hits.sort((a,b)=>a.textContent.length-b.textContent.length)[0];"
        "e.scrollIntoView({block:'center'});e.click();return 'ok';})(%s,%s)"
        % (json.dumps(selector), json.dumps(label))
    )
    return p.eval(js) == 'ok'


def wait_text(p, text, timeout=8.0):
    deadline = time.time() + timeout
    while time.time() < deadline:
        if text in p.text():
            return True
        time.sleep(0.3)
    return False


def wait_gone(p, text, timeout=6.0):
    deadline = time.time() + timeout
    while time.time() < deadline:
        if text not in p.text():
            return True
        time.sleep(0.3)
    return False


def go(p, hash_route):
    if hash_route not in p.url():
        p.eval(f"location.hash = '{hash_route}'")
        time.sleep(1.0)


def verify():
    cdp.wait_port(30)
    p = cdp.Page()
    # 应用默认窗宽 1160 ≤ 1240，useNarrow 会把左树收进抽屉（T5.3 的预期行为）——
    # e2e 覆盖成宽视口，按完整三栏布局测
    p.call(
        'Page.setDeviceMetricsOverride',
        {'width': 1440, 'height': 900, 'deviceScaleFactor': 0, 'mobile': False},
    )
    deadline = time.time() + 40
    while time.time() < deadline:
        t = p.text()
        if '资料库' in t or '浏览' in t:
            break
        time.sleep(0.5)
    time.sleep(1.2)
    ok = True
    txt = p.text()

    # ------------------------------------------------ 浏览：树 + 虚拟视图
    go(p, '#/browser')
    time.sleep(0.8)
    txt = p.text()
    ok &= check('树有待整理虚拟视图', '待整理' in txt, txt[:300])
    ok &= check('树有待定区虚拟视图', '待定区' in txt, txt[:300])
    ok &= check('树有已建目录（安全环保）', '安全环保' in txt, txt[:300])
    ok &= check('未归类分组可见（老数据目录被删）', '未归类' in txt, txt[:300])

    # 点「待整理」虚拟视图 → 池内文件
    ok &= check('点「待整理」', js_click(p, '.vrow, .tree .row', '待整理'))
    time.sleep(0.8)
    ok &= check('待整理视图列出池内文件', wait_text(p, '项目验收报告', 4.0), p.text()[:300])

    # 点目录 → 只列该目录的文件
    ok &= check('点「检查报告」目录', js_click(p, '.tree .row', '检查报告'))
    time.sleep(0.8)
    txt = p.text()
    ok &= check('目录视图列出迁移件', wait_text(p, '2025年度安全检查报告', 4.0), txt[:300])
    ok &= check('其他目录的文件不混进来', '无人认领报告' not in txt, txt[:300])

    # 点行 → 详情条：archived_path 生效（路径指向归档区，不是源区旧路径）
    ok &= check('点迁移件行', js_click(p, '.list .row', '2025年度安全检查报告'))
    time.sleep(0.8)
    txt = p.text()
    ok &= check(
        '详情条显示归档后的真实路径（archived_path）',
        str(ARC).replace('\\', '/') in txt.replace('\\', '/'),
        txt[:400],
    )
    ok &= check('详情条不再指向源区旧路径', '旧路径' not in txt, txt[:400])

    # 未归类分组里能找到孤儿迁移件
    ok &= check('点「未归类」', js_click(p, '.vrow, .tree .row', '未归类'))
    time.sleep(0.8)
    ok &= check('未归类列出孤儿迁移件', '无人认领报告' in p.text(), p.text()[:300])

    # ------------------------------------------------ 检索：目录内筛选 + Ctrl+K + 高级
    ok &= check('点回「检查报告」', js_click(p, '.tree .row', '检查报告'))
    time.sleep(0.8)
    p.eval('const i=document.querySelector(\'input[placeholder*="目录里找"]\'); if(i) i.focus()')
    time.sleep(0.3)
    p.insert_text('消防')
    ok &= check(
        '目录内筛选：输入「消防」只剩消防件',
        wait_gone(p, '2025年度安全检查报告', 4.0) and wait_text(p, '消防检查记录', 3.0),
        p.text()[:300],
    )

    # Ctrl+K 面板
    p.eval("window.dispatchEvent(new KeyboardEvent('keydown',{key:'k',ctrlKey:true,bubbles:true}))")
    time.sleep(0.8)
    ok &= check('Ctrl+K 打开检索面板', wait_text(p, 'Esc 关闭', 4.0), p.text()[-200:])
    p.insert_text('验收')
    time.sleep(0.6)
    ok &= check('面板出结果', wait_text(p, '项目验收报告', 4.0), p.text()[-200:])
    p.press('Escape')
    time.sleep(0.5)
    ok &= check('Esc 关掉面板', wait_gone(p, 'Esc 关闭', 3.0), p.text()[-200:])

    # 高级筛选：AND / OR / URL 预置
    go(p, '#/search')
    time.sleep(0.8)
    ok &= check('高级筛选页有标签筛选器', '安全检查' in p.text(), p.text()[:300])
    ok &= check('勾「安全检查」', js_click(p, 'button.fchip', '安全检查'))
    time.sleep(0.6)
    ok &= check('勾「消防」', js_click(p, 'button.fchip', '消防'))
    time.sleep(1.2)
    txt = p.text()
    ok &= check('AND：两标签交集只剩一份', '2025年度安全检查报告' in txt and '消防检查记录' not in txt, txt[:400])
    ok &= check('切到 OR', js_click(p, '.mode button', '任一'))
    time.sleep(1.2)
    txt = p.text()
    ok &= check('OR：任一标签两份都在', '2025年度安全检查报告' in txt and '消防检查记录' in txt, txt[:400])
    p.eval('location.hash = "#/search?tag=1"')
    time.sleep(1.2)
    txt = p.text()
    ok &= check('URL ?tag= 预置生效（已选条件）', '已选' in txt, txt[:300])

    # ------------------------------------------------ 标签：卡片 chips + 加/删
    go(p, '#/workspace')
    time.sleep(1.0)
    if '没有待整理' not in p.text():
        js_click(p, 'button', '开批')
        time.sleep(1.5)
    txt = p.text()
    ok &= check('卡片显示标签 chips', '安全检查' in txt, txt[:400])
    ok &= check('点「+」加标签', js_click(p, 'button.add', '+'))
    time.sleep(0.5)
    p.insert_text('现场核查')
    p.press('Enter')
    time.sleep(0.8)
    ok &= check('新标签出现', wait_text(p, '现场核查', 5.0), p.text()[:400])
    # 删掉它：现场核查 chip 自己的 ×（别点到别的 chip）
    removed = p.eval(
        "(()=>{const cs=[...document.querySelectorAll('.chip')];"
        "const c=cs.find(x=>(x.textContent||'').includes('现场核查'));"
        "if(!c)return 'no-chip';"
        "const x=c.querySelector('.x');"
        "if(!x)return 'no-x';x.click();return 'ok';})()"
    )
    ok &= check('点现场核查的 ×', removed == 'ok', str(removed))
    ok &= check('删除标签生效', wait_gone(p, '现场核查', 4.0), p.text()[:300])

    # ------------------------------------------------ 设置页词表 + 图片解读开关
    go(p, '#/settings')
    time.sleep(1.0)
    txt = p.text()
    ok &= check('词表列出标签与份数', '安全检查' in txt and ('份' in txt), txt[:400])
    ok &= check('图片解读开关行存在', '图片解读' in txt, txt[:300])
    ok &= check('关着时明说（已关）', '（已关）' in txt, txt[:300])
    ok &= check('计数亮出来', '已读' in txt and '跳过' in txt, txt[:300])
    ok &= check('点「打开」', js_click(p, 'button', '打开'))
    time.sleep(1.0)
    ok &= check('打开后（已关）消失', wait_gone(p, '（已关）', 4.0), p.text()[:300])
    js_click(p, 'button', '关掉')
    time.sleep(0.8)

    # ------------------------------------------------ 待定区 AI 说明（深读关着）
    go(p, '#/parked')
    time.sleep(1.0)
    txt = p.text()
    ok &= check('待定区列出 parked 件', '神秘材料' in txt, txt[:300])
    ok &= check('深读关着：明说「深度解读已关闭」', '深度解读已关闭' in txt, txt[:400])
    ok &= check('给「去开启」入口', '去开启' in txt, txt[:300])
    ok &= check('点「去开启」跳设置', js_click(p, 'button', '去开启'))
    time.sleep(1.0)
    ok &= check('落到设置页', '/settings' in p.url(), p.url())

    # ------------------------------------------------ 老数据回填（回填在启动时已跑）
    db = DATA / 'index.db'
    conn = sqlite3.connect(f'file:{db}?mode=ro', uri=True)
    rows = conn.execute(
        'SELECT archived_path, archive_dir_id FROM files WHERE status="migrated"'
    ).fetchall()
    has_path = sum(1 for r in rows if r[0])
    conn.close()
    ok &= check('迁移件 archived_path 全部落库', has_path == len(rows) and len(rows) >= 3, f'{has_path}/{len(rows)}')
    ok &= check('有目录的件回填了 archive_dir_id', any(r[1] is not None for r in rows), str(rows))

    # ------------------------------------------------ P4 M1：重新归类（归档区内二次迁移）
    go(p, '#/browser')
    time.sleep(0.8)
    ok &= check('点「检查报告」目录（recat 前置）', js_click(p, '.tree .row', '检查报告'))
    time.sleep(0.8)
    # 右键迁移件 → 菜单出第四项 → 菜单内选目标目录
    ok &= check('右键出上下文菜单', js_ctxmenu(p, '.list .row', '2025年度安全检查报告'))
    time.sleep(0.5)
    ok &= check('菜单有「重新归类…」', js_click(p, '.menu .mi', '重新归类'))
    time.sleep(0.5)
    ok &= check('候选不含当前目录，出「安全环保」', js_click(p, '.menu .mi', '安全环保'))
    time.sleep(1.5)
    # 磁盘真挪了 + 台账是 recategorize 批次（「安全环保」目录 rel_path 就是一层）
    moved = ARC / '安全环保' / '2025年度安全检查报告.docx'
    ok &= check('文件真挪到新目录（磁盘）', moved.is_file(), str(moved))
    ok &= check(
        '旧位置空出来',
        not (ARC / '安全环保' / '检查报告' / '2025年度安全检查报告.docx').exists(),
        '',
    )
    db2 = sqlite3.connect(f'file:{DATA / "index.db"}?mode=ro', uri=True)
    row = db2.execute(
        "SELECT archived_path, archive_dir_id FROM files WHERE name='2025年度安全检查报告.docx'"
    ).fetchone()
    kinds = db2.execute(
        "SELECT kind FROM migrations ORDER BY id DESC LIMIT 1"
    ).fetchone()
    db2.close()
    ok &= check(
        'archived_path 指向新目录',
        bool(row and (row[0] or '').replace('/', '\\').endswith('安全环保\\2025年度安全检查报告.docx')),
        str(row),
    )
    ok &= check('台账记成 recategorize 批次', bool(kinds and kinds[0] == 'recategorize'), str(kinds))
    # 浏览页即时生效：新目录下能看到它
    ok &= check('点「安全环保」目录', js_click(p, '.tree .row', '安全环保'))
    time.sleep(0.8)
    ok &= check('浏览页新目录列出该文件', wait_text(p, '2025年度安全检查报告', 4.0), p.text()[:300])

    print(f'\n==== e2e-p3 结果: {"ALL PASS" if ok else "有 FAIL"}')
    return 0 if ok else 1


if __name__ == '__main__':
    if sys.argv[1:] and sys.argv[1] == 'seed':
        seed()
        sys.exit(0)
    sys.exit(verify())
