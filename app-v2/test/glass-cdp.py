# -*- coding: utf-8 -*-
"""BUG-011 CDP 量测：材质计算样式 / 对比度 / 三档截图 / 命中测试 / 粗帧率。
运行：python test/glass-cdp.py
"""
import base64
import io
import json
import os
import re
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from cdp import Page  # noqa: E402
from PIL import Image  # noqa: E402

SHOT_DIR = os.path.abspath(os.path.join(
    os.path.dirname(os.path.abspath(__file__)), '..', '..', 'bugfix'))


def srgb(c):
    c = c / 255.0
    return c / 12.92 if c <= 0.03928 else ((c + 0.055) / 1.055) ** 2.4


def lum(rgb):
    r, g, b = rgb[:3]
    return 0.2126 * srgb(r) + 0.7152 * srgb(g) + 0.0722 * srgb(b)


def ratio(a, b):
    la, lb = lum(a), lum(b)
    hi, lo = max(la, lb), min(la, lb)
    return (hi + 0.05) / (lo + 0.05)


def parse_color(s):
    m = re.search(r'rgba?\(([^)]+)\)', s or '')
    if not m:
        m = re.search(r'#([0-9a-fA-F]{6})', s or '')
        if m:
            h = m.group(1)
            return tuple(int(h[i:i + 2], 16) for i in (0, 2, 4)) + (1.0,)
        return None
    parts = [p.strip() for p in m.group(1).split(',')]
    rgb = tuple(float(p) for p in parts[:3])
    a = float(parts[3]) if len(parts) > 3 else 1.0
    return rgb + (a,)


def composite(fg, bg):
    """fg/bg 均为 (r,g,b,a)，返回叠好后的不透明 rgb。"""
    a = fg[3]
    return tuple(round(fg[i] * a + bg[i] * (1 - a)) for i in range(3))


def main():
    p = Page()
    out = {}

    def set_glass(on):
        p.eval('localStorage.setItem("filebutler.glassEnabled","%s"); true' % ('1' if on else '0'))
        p.call('Page.reload', {'ignoreCache': True})
        time.sleep(2.2)

    def styles(on):
        js = """(() => {
          const pick = sel => { const e=document.querySelector(sel); if(!e) return null;
            const cs=getComputedStyle(e), r=e.getBoundingClientRect();
            const host=e.closest('.glass-title,.glass-side,.glass-panel,.glass-float');
            return {sel, bg:cs.backgroundColor, bf:cs.backdropFilter||cs.webkitBackdropFilter,
                    color:cs.color, layer:host ? (host.className||'').split(' ').filter(x=>x.indexOf('glass-')===0)[0] : null,
                    rect:{x:r.x,y:r.y,w:r.width,h:r.height},
                    z:cs.zIndex, pos:cs.position}; };
          return JSON.stringify([pick('.glass-title'),pick('.glass-side'),pick('.glass-panel'),
                                 pick('.glass-frame'),pick('.title'),pick('.group-title'),
                                 pick('.side .nav'),pick('.side .p1'),pick('.side .p2'),
                                 pick('.side .mtext'),pick('.main .hint'),pick('.main .name'),
                                 pick('.main .meta')].filter(Boolean));
        })()"""
        return json.loads(p.eval(js))

    # ---------- 材质开 ----------
    set_glass(True)
    out['glassOn'] = styles(True)
    out['htmlClassOn'] = p.eval('document.documentElement.className')

    # 底图渐变止点：从 .glass-frame 的 computed backgroundImage 里解析
    bgimg = p.eval("getComputedStyle(document.querySelector('.glass-frame')).backgroundImage")
    stops = []
    for m in re.finditer(r'rgba?\(([^)]+)\)', bgimg or ''):
        c = parse_color('rgba(' + m.group(1) + ')')
        if c and c[3] > 0:
            stops.append(composite(c, (255, 255, 255, 1)))
    for m in re.finditer(r'#([0-9a-fA-F]{6})', bgimg or ''):
        h = m.group(1)
        stops.append(tuple(int(h[i:i + 2], 16) for i in (0, 2, 4)))
    if not stops:
        stops = [(255, 255, 255)]
    out['backdropStops'] = stops

    # 对比度：每个正文元素 × 它所在材质层（alpha 合成底图止点），取最坏值
    checks = []
    layers = {x['sel'].lstrip('.'): x for x in out['glassOn'] if x['sel'] in
              ('.glass-title', '.glass-side', '.glass-panel', '.glass-float')}
    for t in out['glassOn']:
        lay_name = t.get('layer')
        if not lay_name or lay_name not in layers:
            continue
        lay = parse_color(layers[lay_name]['bg'])
        fg = parse_color(t['color'])
        if not lay or not fg:
            continue
        worst = min((ratio(fg[:3], composite(lay, b)) for b in stops), default=0)
        checks.append({
            'layer': lay_name, 'text': t['sel'], 'fg': t['color'],
            'layerBg': layers[lay_name]['bg'], 'worstRatio': round(worst, 2),
        })
    out['contrastOn'] = checks

    # ---------- 浮层：CDP 合成 Ctrl+K 打开命令面板并量测 ----------
    p.eval("window.dispatchEvent(new KeyboardEvent('keydown',"
           "{key:'k',ctrlKey:true,bubbles:true,cancelable:true})); true")
    time.sleep(0.5)
    fl = p.eval("""(() => { const e=document.querySelector('.glass-float'); if(!e) return null;
      const cs=getComputedStyle(e); return JSON.stringify({bg:cs.backgroundColor,
        bf:cs.backdropFilter||cs.webkitBackdropFilter, shadow:cs.boxShadow,
        textBacking:!!document.querySelector('.glass-float .glass-text')}); })()""")
    out['floatPanel'] = json.loads(fl) if fl else None
    p.eval("window.dispatchEvent(new KeyboardEvent('keydown',"
           "{key:'Escape',bubbles:true,cancelable:true})); true")
    time.sleep(0.3)

    # ---------- 命中测试（按钮不被材质层遮挡）----------
    hit = p.eval("""(() => {
      const out=[];
      for (const sel of ['.wb.close','.nav','.btn']) {
        for (const e of [...document.querySelectorAll(sel)].slice(0,4)) {
          const r=e.getBoundingClientRect(); if(!r.width||!r.height) continue;
          const x=r.x+r.width/2, y=r.y+r.height/2;
          const top=document.elementFromPoint(x,y);
          out.push({sel, hit:!!top && (top===e || e.contains(top) || top.contains(e)),
                    top: top ? (top.className||top.tagName).toString().slice(0,40) : null,
                    rect:{x:+x.toFixed(1),y:+y.toFixed(1)}});
        }
      }
      return JSON.stringify(out);
    })()""")
    out['hitTest'] = json.loads(hit)

    # ---------- 三档窗口截图 + 尺寸量测 ----------
    shots = []
    for w, h in [(1000, 700), (1440, 900), (1600, 900)]:
        p.call('Emulation.setDeviceMetricsOverride', {
            'width': w, 'height': h, 'deviceScaleFactor': 1, 'mobile': False,
        })
        time.sleep(0.8)
        geom = p.eval("""(() => {
          const q=s=>{const e=document.querySelector(s); if(!e)return null; const r=e.getBoundingClientRect();
            return {x:+r.x.toFixed(1),y:+r.y.toFixed(1),w:+r.width.toFixed(1),h:+r.height.toFixed(1)};};
          return JSON.stringify({win:q('.glass-frame'),title:q('.glass-title'),
            side:q('.glass-side'),main:q('.glass-panel'),close:q('.wb.close'),
            docW:document.documentElement.scrollWidth,docH:document.documentElement.scrollHeight});
        })()""")
        path = os.path.join(SHOT_DIR, 'BUG-011-shot-%dx%d.png' % (w, h))
        os.makedirs(SHOT_DIR, exist_ok=True)
        p.screenshot(path)
        entry = {'size': '%dx%d' % (w, h), 'path': path,
                 'bytes': os.path.getsize(path), 'geom': json.loads(geom)}
        if w == 1440:
            # 真机截图取色：在文字右侧（或左侧）的空白处采一小块中位数，
            # 直接对「材质 + 模糊 + 渐变底图」合成后的像素算 WCAG 对比度。
            img = Image.open(path).convert('RGB')
            samples = []
            for t in out['glassOn']:
                r = t.get('rect')
                if not r or not t.get('layer') or r['w'] < 4 or r['h'] < 4:
                    continue
                fg = parse_color(t['color'])
                if not fg:
                    continue
                host = layers.get(t['layer'])
                hr = host['rect'] if host else None
                x = r['x'] + r['w'] + 8
                if hr and x > hr['x'] + hr['w'] - 3:
                    x = r['x'] - 8
                y = r['y'] + r['h'] / 2
                if x < 2 or y < 2 or x >= img.width - 2 or y >= img.height - 2:
                    continue
                px = []
                for dx in range(-3, 4):
                    for dy in range(-3, 4):
                        px.append(img.getpixel((int(x + dx), int(y + dy))))
                bg = tuple(sorted(c[i] for c in px)[len(px) // 2] for i in range(3))
                samples.append({'text': t['sel'], 'fg': t['color'], 'bg': list(bg),
                                'ratio': round(ratio(fg[:3], bg), 2)})
            entry['pixelContrast'] = samples
        shots.append(entry)
    p.call('Emulation.clearDeviceMetricsOverride')
    out['shots'] = shots

    # ---------- 粗帧率（rAF 计数，非阻断）----------
    fps = p.eval("""new Promise(res => {
      let n=0; const t0=performance.now();
      const tick=()=>{ n++; if(performance.now()-t0<1200) requestAnimationFrame(tick);
        else res(JSON.stringify({frames:n, ms:Math.round(performance.now()-t0),
          fps:+(n/((performance.now()-t0)/1000)).toFixed(1)})); };
      requestAnimationFrame(tick);
    })""", await_promise=True)
    out['perf'] = json.loads(fps)

    # ---------- 材质关：回退实色 ----------
    set_glass(False)
    out['glassOff'] = styles(False)
    out['htmlClassOff'] = p.eval('document.documentElement.className')
    off_path = os.path.join(SHOT_DIR, 'BUG-011-shot-off-solid.png')
    p.screenshot(off_path)
    out['offShot'] = off_path

    # 恢复默认开
    set_glass(True)
    print(json.dumps(out, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    main()
