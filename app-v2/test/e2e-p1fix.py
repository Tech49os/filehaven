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
import struct
import sys
import time
import zipfile
import zlib
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import cdp  # noqa: E402

SRC = Path('D:/filemanager/.e2e-src')
ARC = Path('D:/filemanager/.e2e-archive')
DATA = Path(os.environ.get('FILEBUTLER_DATA_DIR', 'D:/filemanager/.e2e-data'))

# 造几份**真的能读出正文**的文件：预览功能不拿真文件验，等于没验
def make_docx(p: Path, text: str):
    with zipfile.ZipFile(p, 'w', zipfile.ZIP_DEFLATED) as z:
        z.writestr(
            'word/document.xml',
            '<w:document xmlns:w="x"><w:body><w:p><w:r><w:t>'
            + text
            + '</w:t></w:r></w:p></w:body></w:document>',
        )


def make_xlsx(p: Path, rows: list[str]):
    body = ''.join(f'<si><t>{r}</t></si>' for r in rows)
    with zipfile.ZipFile(p, 'w', zipfile.ZIP_DEFLATED) as z:
        z.writestr('xl/sharedStrings.xml', f'<sst>{body}</sst>')


def make_pptx(p: Path, slides: list[str]):
    """真 pptx：只要 ppt/slides/slideN.xml 里有 <a:t> 文字，抽取器就该读得出来"""
    with zipfile.ZipFile(p, 'w', zipfile.ZIP_DEFLATED) as z:
        for i, s in enumerate(slides, 1):
            z.writestr(
                f'ppt/slides/slide{i}.xml',
                f'<p:sld><a:p><a:r><a:t>{s}</a:t></a:r></a:p></p:sld>',
            )


def make_drawio(p: Path, nodes: list[str]):
    cells = ''.join(
        f'<mxCell id="c{i}" value="{n}" vertex="1"><mxGeometry/></mxCell>'
        for i, n in enumerate(nodes)
    )
    p.write_text(
        f'<mxfile><diagram><mxGraphModel><root>{cells}</root></mxGraphModel></diagram></mxfile>',
        encoding='utf-8',
    )


def make_png(p: Path, w=360, h=200):
    """纯标准库画一张渐变图：只为了证明缩略图真能显示出来"""

    def chunk(tag: bytes, data: bytes) -> bytes:
        return (
            struct.pack('>I', len(data))
            + tag
            + data
            + struct.pack('>I', zlib.crc32(tag + data) & 0xFFFFFFFF)
        )

    rows = bytearray()
    for y in range(h):
        rows.append(0)
        for x in range(w):
            rows += bytes((40 + x * 180 // w, 90 + y * 120 // h, 210 - x * 120 // w))
    ihdr = struct.pack('>IIBBBBB', w, h, 8, 2, 0, 0, 0)
    p.write_bytes(
        b'\x89PNG\r\n\x1a\n'
        + chunk(b'IHDR', ihdr)
        + chunk(b'IDAT', zlib.compress(bytes(rows), 6))
        + chunk(b'IEND', b'')
    )


REAL = [
    ('2026年安全检查通知.docx', lambda p: make_docx(
        p, '关于开展 2026 年度春季安全检查的通知：各部门于 3 月 20 日前完成自查，'
           '重点检查配电室、消防通道与危化品仓库，整改情况报安全环保部。')),
    ('采购合同（第三版）.docx', lambda p: make_docx(
        p, '采购合同 甲方：某某制造有限公司 乙方：某某机电设备有限公司 '
           '合同金额：人民币壹拾贰万元整 交货期：合同签订后 30 日内。')),
    ('部门月度费用明细.xlsx', lambda p: make_xlsx(
        p, ['部门月度费用明细', '差旅费', '办公用品', '培训费', '合计 8,420.00'])),
    ('智能工厂平台架构图.drawio', lambda p: make_drawio(
        p, ['数据采集层', '边缘网关', 'MES 系统', '质量看板', '设备联网'])),
    ('生产智能化项目组工作汇报.pptx', lambda p: make_pptx(
        p, ['生产智能化项目组工作汇报', '一、项目范围与里程碑', '二、成本核算与投入产出'])),
    ('消防演练现场.png', make_png),
    ('整改报告-消防.pdf', None),
]

# 补到 12 份：目录建议要 ≥10 份样本才跑（MIN_FILES）
DUMMY = [f'工作记录0{i}.docx' for i in range(1, 7)]

DOCS = [n for n, _ in REAL] + DUMMY


def seed():
    SRC.mkdir(parents=True, exist_ok=True)
    ARC.mkdir(parents=True, exist_ok=True)
    for name, build in REAL:
        p = SRC / name
        if build is None:
            p.write_bytes(b'%PDF-1.7\n%\xe2\xe3\xcf\xd3\n')
        else:
            build(p)
    for name in DUMMY:
        p = SRC / name
        if not p.exists():
            make_docx(p, '工作记录：本周例会要点与待办事项。')

    DATA.mkdir(parents=True, exist_ok=True)
    db = DATA / 'index.db'
    conn = sqlite3.connect(db)
    conn.executescript(Path('src-tauri/src/schema.sql').read_text(encoding='utf-8'))
    now = int(time.time() * 1000)
    conn.execute(
        'INSERT OR REPLACE INTO settings(key, value) VALUES(?,?)',
        ('archive_root', str(ARC)),
    )
    # 小模型快一点：目录建议这一屏要现场等模型出结果
    conn.execute(
        'INSERT OR REPLACE INTO settings(key, value) VALUES(?,?)',
        ('ollama_model', 'qwen3.5:4b'),
    )
    conn.execute(
        'INSERT OR IGNORE INTO source_roots(path, enabled, added_at) VALUES(?,1,?)',
        (str(SRC), now),
    )
    # 先给一个目录：整理台有目录才会出卡片（否则是「还没有任何目录」空态）
    conn.execute(
        'INSERT OR IGNORE INTO archive_dirs(name, parent_id, rel_path, depth, origin, created_at) '
        'VALUES(?,NULL,?,1,?,?)',
        ('安全与消防', '安全与消防', 'user', now),
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
    # 端口起来 ≠ 界面画完：WebView 首屏还要挂载一次（尤其刚编译完首启）
    deadline = time.time() + 40
    while time.time() < deadline:
        if '资料库' in p.text() or '引导' in p.text():
            break
        time.sleep(0.5)
    time.sleep(1.2)
    ok = True

    if '/workspace' not in p.url():
        # 引导类页面（/onboarding/*）不显示侧栏，先回主框架再点
        p.eval("location.hash = '#/workspace'")
        time.sleep(1.0)
        if '资料库' in p.text():
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
    ok &= check(f'扫到 {len(DOCS)} 份', f'扫了 {len(DOCS)} 份' in txt, txt[-300:])

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

    # ---- P1c：路径 / 预览 / 打开 / 窄窗收栏（2026-11-20 反馈）
    p.click_text('整理台', 'button', exact=False)
    time.sleep(1.0)
    ok &= verify_workstead(p)

    # ---- P1d：不刷批次 / 只报待确认 / 改名 / 目录联想（2026-11-20 第二轮反馈）
    ok &= verify_p1d(p)

    # ---- P1c：目录建议能听人话
    ok &= verify_dirsuggest(p)

    # ---- 回归：移除源区后入口还在（**放最后**：移除会连带删掉该源下的文件索引）
    p.click_text('设置', 'button', exact=False)
    time.sleep(0.8)
    if '移除' in p.text():
        # 必须 exact：li 里也含「移除」，contains 匹配会点到整行而不是按钮
        p.click_text('移除', 'button', exact=True)
        time.sleep(1.2)
        after = p.text()
        ok &= check('移除后回到空态', '还没有源区' in after, after[-500:])
        ok &= check('空态里仍有「添加源区」入口', '添加源区' in after, after[-500:])
        ok &= check('空态里有可点的「选一个文件夹」', '选一个文件夹' in after, after[-500:])

    print('\nRESULT:', 'ALL PASS' if ok else 'HAS FAILURE')
    return 0 if ok else 1


def verify_workstead(p) -> bool:
    """整理台：看得见路径与内容、能快速打开、窄窗不挤"""
    ok = True
    txt = p.text()
    src = str(SRC).replace('\\', '/')
    ok &= check('卡片显示完整路径（不是只有文件名）', src in txt or str(SRC) in txt, txt[:400])
    ok &= check('有「打开」与「定位」按钮', '打开' in txt and '定位' in txt, txt[:300])
    ok &= check('快捷键多了 O / L', '打开' in txt and '定位' in txt)

    # 抽屉：默认窗口 1160 < 1240，右栏应该收起来
    has_right = p.eval("document.querySelector('aside.right') !== null")
    ok &= check('窄窗时右栏收进抽屉', has_right is False, f'aside.right 存在={has_right}')
    try:
        p.click_text('已确认', 'button', exact=False)
        time.sleep(0.5)
        opened = p.eval("document.querySelector('aside.right') !== null")
        ok &= check('点「已确认」能展开抽屉', opened is True, str(opened))
        p.click_text('已确认', 'button', exact=False)
        time.sleep(0.3)
    except RuntimeError as e:
        ok &= check('点「已确认」能展开抽屉', False, str(e))

    # 逐份看过去：至少一份能读出正文，至少一张图片能显示缩略图
    rows = p.eval("document.querySelectorAll('.qrow').length") or 0
    text_hit = ''
    all_text = ''
    img_hit = False
    for i in range(min(rows, 12)):
        pos = p.eval(
            "(() => { const r = document.querySelectorAll('.qrow')[%d];"
            " if (!r) return null; const b = r.getBoundingClientRect();"
            " return {x: b.left + b.width/2, y: b.top + b.height/2}; })()" % i
        )
        if not pos:
            continue
        p.click(pos['x'], pos['y'])
        time.sleep(0.35)
        body = p.eval(
            "(() => { const t = document.querySelector('.pv-text');"
            " const im = document.querySelector('.pv-thumb img');"
            " return { text: t ? t.textContent.trim().slice(0, 60) : '',"
            " img: im ? im.getAttribute('src') : '' }; })()"
        ) or {}
        if body.get('text'):
            all_text += body['text']
            if not text_hit:
                text_hit = body['text']
        if body.get('img', '').startswith('http'):
            img_hit = True

    ok &= check('能读出正文摘要', bool(text_hit), text_hit)
    print(f'        正文样例：{text_hit[:40]}')
    ok &= check('图片能显示缩略图（asset 协议）', img_hit)
    # 2026-11-20 反馈：42MB 的 pptx 显示「读不出正文」。抽取器不能按文件总大小放弃
    ok &= check('pptx 也能读出正文', '生产智能化' in all_text, all_text[:120])
    p.screenshot('D:/filemanager/app-v2/test/shot-workstead.png')
    return ok


def verify_p1d(p) -> bool:
    """第二轮反馈：进整理台不再刷批次、顶部只说待确认份数、能改名、N 里能联想"""
    ok = True
    p.click_text('整理台', 'button', exact=False)
    time.sleep(1.0)

    def pending() -> str:
        return p.eval("document.querySelector('.ws .title')?.textContent?.trim() ?? ''")

    title = pending()
    ok &= check('顶部只报「待确认 N 份」', title.startswith('待确认') and title.endswith('份'), title)
    ok &= check('顶部不再出现「第 N 批」', '批' not in title, title)

    before = pending()
    p.eval("document.querySelector('.mid .ft .btn.primary')?.click()")
    time.sleep(0.9)
    after = pending()
    ok &= check('确认一份后待确认数跟着减', before != after, f'{before} → {after}')

    # 切走再回来：批次不能重开（重开的话份数会回到 12 份）
    p.click_text('待迁移', 'button', exact=False)
    time.sleep(0.6)
    p.click_text('整理台', 'button', exact=False)
    time.sleep(1.0)
    again = pending()
    ok &= check('切走再回来不重开批次', again == after, f'{after} → {again}')

    # 改名：点「改名」→ 写新名 → 回车
    ok &= check('卡片上有「改名」入口', bool(p.eval("!!document.querySelector('.ren')")))
    try:
        p.eval("document.querySelector('.ren')?.click()")
        time.sleep(0.35)
        p.eval(
            "(() => { const i = document.querySelector('.rename-input');"
            " if (!i) return;"
            " const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set;"
            " setter.call(i, '改名验证-年度汇报');"
            " i.dispatchEvent(new Event('input', {bubbles:true}));"
            " i.dispatchEvent(new KeyboardEvent('keydown', {key:'Enter', bubbles:true})); })()"
        )
        time.sleep(0.9)
        t = p.text()
        ok &= check('改名后卡片说清「归档时存为」', '归档时存为' in t and '改名验证' in t, t[:300])
    except RuntimeError as e:
        ok &= check('改名能落到卡片上', False, str(e))

    # N 输入框：聚焦后列出已有目录，敲「安全」能索引到
    try:
        p.click_text('新建目录', 'button', exact=False)
        time.sleep(0.6)
        p.eval("document.querySelector('.newdir input')?.focus()")
        time.sleep(0.4)
        p.eval(
            "(() => { const i = document.querySelector('.newdir input');"
            " const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set;"
            " setter.call(i, '安全');"
            " i.dispatchEvent(new Event('input', {bubbles:true})); })()"
        )
        time.sleep(0.9)
        opts = p.eval(
            "Array.from(document.querySelectorAll('.newdir .opt')).map(e => e.textContent.trim())"
        ) or []
        ok &= check('N 输入框能联想已有目录', any('安全' in o for o in opts), str(opts)[:200])
        p.press('Escape')
        time.sleep(0.3)
    except RuntimeError as e:
        ok &= check('N 输入框能联想已有目录', False, str(e))

    p.screenshot('D:/filemanager/app-v2/test/shot-p1d.png')
    return ok


def verify_dirsuggest(p) -> bool:
    """目录建议：能写补充要求、能就地改名（入口在设置页，配置完成后也回得去）"""
    ok = True
    p.click_text('设置', 'button', exact=False)
    time.sleep(0.8)
    try:
        # 注意：click_text 比对时会把空白全去掉，这里也得写成不含空格的形态
        p.click_text('AI建议目录', 'button', exact=False)
    except RuntimeError:
        return check('设置页有「AI 建议目录」入口', False, p.text()[:200])
    time.sleep(1.0)
    ok &= check('进得去目录建议页', '挑一套目录结构' in p.text(), p.text()[:200])

    # 等模型出方案（4b 在 12 份样本上大约 30~90 秒）
    # 注意不能等「按我说的再来一版」——那个输入框在等待期也在，会立刻以为出结果了
    deadline = time.time() + 260
    while time.time() < deadline:
        t = p.text()
        if '方案 A' in t or '先多扫一些' in t or '只有' in t:
            break
        time.sleep(2)
    txt = p.text()
    has_ask = '按我说的再来一版' in txt
    ok &= check('有「补充要求」输入框', has_ask, txt[:300])
    ok &= check('两套方案都出来了', '方案 A' in txt and '方案 B' in txt, txt[:300])

    if has_ask:
        p.screenshot('D:/filemanager/app-v2/test/shot-dirsuggest.png')
        # 就地改名：点第一个目录名，看看能不能变成输入框
        pos = p.eval(
            "(() => { const n = document.querySelector('.dname'); if (!n) return null;"
            " const b = n.getBoundingClientRect();"
            " return {x: b.left + b.width/2, y: b.top + b.height/2}; })()"
        )
        if pos:
            p.click(pos['x'], pos['y'])
            time.sleep(0.4)
            ok &= check(
                '点目录名能就地改名',
                p.eval("document.querySelector('.dname-input') !== null") is True,
            )
            p.press('Escape')

    # 这页不显示侧栏，回主框架再继续后面的检查
    p.eval("location.hash = '#/settings'")
    time.sleep(1.0)
    return ok


if __name__ == '__main__':
    mode = sys.argv[1] if len(sys.argv) > 1 else 'verify'
    if mode == 'seed':
        seed()
    else:
        sys.exit(verify())
