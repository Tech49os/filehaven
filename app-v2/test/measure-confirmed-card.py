"""BUG-005 CDP 量测：已确认卡片文本块矩形 / 右侧已确认栏行高。

用法：
    python test/measure-confirmed-card.py before   # 修复前复测
    python test/measure-confirmed-card.py after    # 修复后验收

前提：应用带 --remote-debugging-port=9222 启动，dev server 在 5273。
所有选择器都是全路径（如 `.ws > .cols > .right > ul.done > li.drow`），
避免同名 .foot / .dname 选错元素。量测只问 getComputedStyle / getBoundingClientRect，
不按截图像素猜。长文件名 / 长路径 / 多徽章 / 10 条记录用 DOM 注入加压，
测完即还原（这些压力数据在 demo 数据集里没有现成的）。
"""
import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import cdp  # noqa: E402

PHASE = sys.argv[1] if len(sys.argv) > 1 else 'after'
WIDTHS = (1000, 1280, 1600)

# 全路径选择器（BUG-005 踩坑：同名 .foot / .dname 会选错元素）
SEL = {
    'card': '.ws > .cols > .mid > .card.done',
    'name': '.ws > .cols > .mid > .card.done > .hd > .nm > .nameline > .name',
    'meta': '.ws > .cols > .mid > .card.done > .hd > .nm > .meta',
    'state': '.ws > .cols > .mid > .card.done > .hd > .state',
    'pval': '.ws > .cols > .mid > .card.done > .pathrow > .pval',
    'result': '.ws > .cols > .mid > .card.done > .result',
    'dest': '.ws > .cols > .mid > .card.done > .result > .dest',
    'hint': '.ws > .cols > .mid > .card.done > .ft > .hint',
    'rows': '.ws > .cols > .right > ul.done > li.drow',
    'right': '.ws > .cols > .right',
    'drawer': '.ws > .top > .acts > button.drawer-btn',
}

MEASURE_JS = r"""(() => {
  const SEL = %s;
  const q = s => document.querySelector(s);
  const qa = s => [...document.querySelectorAll(s)];
  const rect = el => {
    if (!el) return null;
    const r = el.getBoundingClientRect();
    const f = n => +n.toFixed(2);
    return { l: f(r.left), t: f(r.top), r: f(r.right), b: f(r.bottom), w: f(r.width), h: f(r.height) };
  };
  const card = q(SEL.card);
  if (!card) return { error: 'no confirmed card in .mid', url: location.href, body: document.body.innerText.slice(0, 200) };

  // ---- 注入压力：长文件名 / 长路径 / 多徽章（测后还原） ----
  const saved = [];
  const save = (el, key) => saved.push([el, key, el.innerHTML]);
  const name = q(SEL.name), pval = q(SEL.pval), state = q(SEL.state);
  const result = q(SEL.result);
  if (name) { save(name, 'innerHTML'); name.textContent = '超长文件名压力测试' .repeat(12) + '最终版.docx'; }
  if (pval) { save(pval, 'innerHTML'); pval.textContent = 'C:/Users/kingb/Desktop/' + '很长的一级目录/'.repeat(10) + '最终文件.docx'; }
  const destB = result ? result.querySelector('b') : null;
  if (destB) { save(destB, 'innerHTML'); destB.textContent = 'D:/归档/'.repeat(3) + '很深很深的子目录/'.repeat(8) + '最终归档路径.docx'; }
  if (state) {
    const badge = state.querySelector('.badge.ok');
    if (badge) for (let i = 0; i < 6; i++) {
      const c = badge.cloneNode(true);
      c.setAttribute('data-bug005-stress', '1');
      c.textContent = '徽章' + (i + 1);
      state.appendChild(c);
    }
  }

  // ---- 量测文本块矩形 + computed style ----
  const blocks = {};
  for (const k of ['card', 'name', 'meta', 'state', 'pval', 'result', 'dest', 'hint']) {
    const el = q(SEL[k]);
    if (!el) { blocks[k] = null; continue; }
    const c = getComputedStyle(el);
    blocks[k] = {
      rect: rect(el),
      cs: {
        display: c.display, overflow: c.overflow, textOverflow: c.textOverflow,
        whiteSpace: c.whiteSpace, minWidth: c.minWidth, minHeight: c.minHeight,
        maxWidth: c.maxWidth, flex: c.flex, gridTemplateColumns: c.gridTemplateColumns,
        rowGap: c.rowGap, columnGap: c.columnGap, lineHeight: c.lineHeight,
        fontSize: c.fontSize, webkitLineClamp: c.webkitLineClamp,
      },
      clipped: el.scrollWidth > el.clientWidth + 1,
      title: el.getAttribute('title'),
    };
  }
  const overlap = (a, b) => {
    if (!a || !b) return null;
    const eps = 0.5;
    const w = Math.min(a.r, b.r) - Math.max(a.l, b.l) - eps;
    const h = Math.min(a.b, b.b) - Math.max(a.t, b.t) - eps;
    return (w > 0 && h > 0) ? { w: +w.toFixed(2), h: +h.toFixed(2) } : null;
  };
  const pairs = {};
  const keys = ['name', 'meta', 'state', 'pval', 'result', 'hint']
    .filter(k => blocks[k] && blocks[k].rect && blocks[k].rect.w > 0 && blocks[k].rect.h > 0);
  for (let i = 0; i < keys.length; i++)
    for (let j = i + 1; j < keys.length; j++)
      pairs[keys[i] + ' x ' + keys[j]] = overlap(blocks[keys[i]].rect, blocks[keys[j]].rect);

  // ---- 右侧已确认栏：2 条 / 10 条行高 ----
  const right = q(SEL.right);
  const rows0 = qa(SEL.rows);
  const rowTpl = rows0[0];
  const rowHeights = (n) => {
    while (qa(SEL.rows).length < n && rowTpl) {
      const c = rowTpl.cloneNode(true);
      c.setAttribute('data-bug005-stress', '1');
      rowTpl.parentNode.appendChild(c);
    }
    const hs = qa(SEL.rows).slice(0, n).map(el => +el.getBoundingClientRect().height.toFixed(2));
    return hs;
  };
  const h2 = rowTpl ? rowHeights(2) : [];
  const h10 = rowTpl ? rowHeights(10) : [];
  const rightInfo = right ? {
    clientHeight: right.clientHeight, scrollHeight: right.scrollHeight,
    overflowY: getComputedStyle(right).overflowY,
  } : null;

  // ---- 还原 DOM ----
  for (const [el, key, html] of saved) el[key] = html;
  for (const c of qa('[data-bug005-stress]')) c.remove();

  return {
    viewport: [innerWidth, innerHeight, devicePixelRatio],
    originalRowCount: rows0.length,
    blocks, pairs,
    right: { ...rightInfo, h2, h10,
      h2Spread: h2.length ? +(Math.max(...h2) - Math.min(...h2)).toFixed(2) : null,
      h10Spread: h10.length ? +(Math.max(...h10) - Math.min(...h10)).toFixed(2) : null },
  };
})()""" % json.dumps(SEL, ensure_ascii=False)


def click_selector(p, selector):
    pos = p.eval("""(() => {
      const el = document.querySelector(%s);
      if (!el) return null;
      el.scrollIntoView({ block: 'center' });
      const r = el.getBoundingClientRect();
      return [r.left + r.width / 2, r.top + r.height / 2];
    })()""" % json.dumps(selector))
    if not pos:
        return False
    p.click(pos[0], pos[1])
    return True


def main():
    p = cdp.Page()
    p.eval("location.hash = '#/workspace'")
    time.sleep(1.5)

    # 让 .mid 里显示一张 confirmed 卡片：优先当前已是 done；否则确认一份再点回队列
    state = p.eval("""(() => ({
      done: !!document.querySelector('.ws > .cols > .mid > .card.done'),
      pending: document.querySelectorAll('.ws > .cols > .left > .queue > .qrow:not(.done)').length,
      confirmedRows: document.querySelectorAll('.ws > .cols > .right > ul.done > li.drow').length,
    }))()""")
    if not state['done'] and state['pending'] > 0:
        click_selector(p, '.ws > .cols > .mid > .card > .ft > .btn.primary')
        time.sleep(1.0)
        click_selector(p, '.ws > .cols > .left > .queue > .qrow.done')
        time.sleep(0.5)
    elif not state['done'] and state['confirmedRows'] > 0:
        click_selector(p, '.ws > .cols > .left > .queue > .qrow.done')
        time.sleep(0.5)

    report = {'phase': PHASE, 'widths': {}, 'date': time.strftime('%Y-%m-%d %H:%M:%S')}
    for width in WIDTHS:
        p.call('Emulation.setDeviceMetricsOverride', {
            'width': width, 'height': 900, 'deviceScaleFactor': 1, 'mobile': False,
        })
        time.sleep(0.4)
        # 窄窗 (<1240) 右栏收进抽屉，先打开才能量行高
        opened = False
        if not p.eval('!!document.querySelector(%s)' % json.dumps(SEL['right'])):
            if click_selector(p, SEL['drawer']):
                opened = True
                time.sleep(0.4)
        report['widths'][str(width)] = p.eval(MEASURE_JS)
        if opened:
            click_selector(p, SEL['drawer'])
            time.sleep(0.2)
    p.call('Emulation.clearDeviceMetricsOverride')
    print(json.dumps(report, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    main()
