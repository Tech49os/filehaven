# -*- coding: utf-8 -*-
"""诊断脚本：验证「原始目录路径」作为分类证据的实际价值（只读，不改源码）

回答三个问题：
  1. 现有分类里，多少文件的结论和「原始目录名」冲突？
  2. 未判出工作域的 96 份，靠目录名能救回多少？
  3. 目录名可不可信（是否存在骗人的目录）？
"""
import json
import re
import collections
from pathlib import Path

HERE = Path(__file__).parent
SRC = HERE.parent / 'src' / 'core' / 'classifier.ts'
DATA = HERE / 'real-classify.json'


def load_ws_lexicon():
    """从 classifier.ts 抽出工作域词表（name -> [(word, strength)]）"""
    txt = SRC.read_text(encoding='utf-8')
    block = txt.split('export const WORKSTREAM_LEXICON')[1]
    block = block.split('\n/** 二级')[0]
    out = []
    for seg in re.split(r"\n  \{\n    name: '", block)[1:]:
        name = seg.split("'", 1)[0]
        words = [(w, int(s)) for s, w in re.findall(r"c([123])\('([^']+)'\)", seg)]
        out.append((name, words))
    return out


LEX = load_ws_lexicon()
print('工作域词表：', [(n, len(w)) for n, w in LEX])


def dir_segments(rel: str):
    parts = [p for p in rel.replace('/', '\\').split('\\') if p]
    return parts[:-1]  # 去掉文件名


def score_dirs(segs):
    """目录段（不含文件名）命中的工作域得分"""
    text = '\\'.join(segs).lower()
    res = []
    for name, words in LEX:
        core = [w for w, s in words if s == 3 and w in text]
        strong = [w for w, s in words if s == 2 and w in text]
        if core or len(strong) >= 2:
            res.append((name, core, strong))
    return res


det = json.loads(DATA.read_text(encoding='utf-8'))['details']

# ---- 1. 目录信号 vs 现有结论 ----
agree = conflict = rescue = 0
only_dir = []       # 文件名/正文判不出，但目录能判出
dir_conflicts = []  # 目录和现有结论不一致
for x in det:
    segs = dir_segments(x['rel'])
    if not segs:
        continue
    hits = score_dirs(segs)
    if not hits:
        continue
    dir_ws = hits[0][0]
    if not x['ws']:
        rescue += 1
        only_dir.append((x, dir_ws, hits[0][1] or hits[0][2]))
    elif x['ws'] == dir_ws:
        agree += 1
    else:
        conflict += 1
        dir_conflicts.append((x, dir_ws))

print('\n【1】目录段命中工作域词典的文件')
print(f'  与现有结论一致 : {agree}')
print(f'  与现有结论冲突 : {conflict}')
print(f'  现有判不出、目录能判 : {rescue}')

print('\n【2】目录能救回的样例（文件名/正文判不出工作域）')
for x, w, ev in only_dir[:25]:
    print(f"  {x['rel'][:58]:60s} -> 目录指向「{w}」 证据{ev}")

print('\n【3】目录与结论冲突的样例（检查目录是否可信）')
for x, w in dir_conflicts[:25]:
    print(f"  {x['rel'][:52]:54s} 现判「{x['ws']}」/目录「{w}」")

# ---- 4. 目录一级的「纯度」：有多少一级目录可以直接当工作域 ----
print('\n【4】一级目录纯度（现有结论为参照）')
by_top = collections.defaultdict(list)
for x in det:
    segs = dir_segments(x['rel'])
    by_top[segs[0] if segs else '<根>'].append(x['ws'] or '未判出')
for k, v in sorted(by_top.items(), key=lambda kv: -len(kv[1])):
    c = collections.Counter(v)
    top, n = c.most_common(1)[0]
    print(f'  {len(v):4d}  {k[:20]:22s} 主导={top}:{n}  纯度={n/len(v):.0%}')
