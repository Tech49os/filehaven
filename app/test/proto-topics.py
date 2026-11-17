# -*- coding: utf-8 -*-
"""原型验证：从文件名「涌现」事项专名（零配置，不预设任何行业词表）

目标：验证「按专名分组」能不能在你 450 份真实文件上，分出有意义的「事项」，
而不是像 TF-IDF 全文聚类那样分出一堆「合同类/汇报类」。

只内置语言级通用词（方案/汇报/附件/副本/v1 这类），绝不内置「钻采院/AI大赛」
这种行业词——那些必须靠文件自身涌现。
"""
import json
import re
import collections
from pathlib import Path

HERE = Path(__file__).parent
det = json.loads((HERE / 'real-classify.json').read_text(encoding='utf-8'))['details']

# ── 语言级通用词：文档性质词、版本词、公文套话。与行业无关，任何中文语料都适用
STOP = set("""
方案 汇报 报告 合同 协议 清单 报价 成本 工作量 模板 附件 副本 修改 意见
最终 最终版 初版 草稿 评审 申请 审批 单 表 图 手册 规范 办法 制度 计划 总结
纪要 议程 提纲 说明 情况 说明 材料 文档 文件 内容 要求 通知 名单 汇总 梳理
汇总表 统计 统计表 登记 记录 签到 封面 目录 正文 附录 参考 参考资料
技术 工作 项目 建设 管理 服务 系统 平台 开发 设计 需求 支撑 提升 优化
版本 版 初稿 完 已 新 旧 本 期 月 年 日 第 部分 相关 有关 关于
xx 某某 测试 示例 样例 范例 下载 归档 文件夹 新建 未命名 无标题
""".split())

NOISE = re.compile(
    r'[（(]\s*\d+\s*[)）]'          # (1) (2)
    r'|副本|最终版|最终|最新|修改版|修订版|定稿'
    r'|[Vv]\s?\d+(\.\d+)*'           # V1.0 v2
    r'|20\d{6}|20\d{2}[-_.年]\d{1,2}([-_.月]\d{1,2})?'  # 日期
    r'|\d{2,}'                       # 裸数字
)

SPLIT = re.compile(r'[-_–—\s+·、，,：:；;！!？?\[\]【】《》“”"\'’()（）]+')


def clean(name: str) -> str:
    base = re.sub(r'\.[A-Za-z0-9]{1,5}$', '', name)
    base = NOISE.sub('|', base)
    return base


def segments(name: str):
    """切成候选片段：整段 + 段内 2..8 字 n-gram"""
    out = set()
    for seg in SPLIT.split(clean(name)):
        seg = seg.strip()
        # 去掉纯停用词包住的边角
        if not seg:
            continue
        out.add(seg)
        for n in range(2, min(9, len(seg) + 1)):
            for i in range(0, len(seg) - n + 1):
                out.add(seg[i:i + n])
    return out


N = len(det)
df = collections.Counter()
for x in det:
    for s in segments(x['name']):
        if s and not re.fullmatch(r'[\d\W_]+', s):
            df[s] += 1

# 只保留「像事项」的片段
cand = {}
for s, c in df.items():
    if c < 3:                      # 少于 3 份文件共享，不算事项
        continue
    if c > N * 0.15:               # 超过 15% 的文件都有 → 是部门名/口头禅，区分力低
        continue
    if s in STOP or len(s) < 3:
        continue
    if any(s == t for t in STOP):
        continue
    # 片段里全是停用词拼的，扔掉
    if all(s[i:i + 2] in STOP for i in range(max(1, len(s) - 1))):
        continue
    cand[s] = c

# 最长匹配优先：如果 "技术谱系"(18) 存在，就不要 "技术谱"(18) "术谱系"(18)
def is_sub_of_longer(s):
    for t in cand:
        if t != s and s in t and cand[t] >= cand[s] * 0.9:
            return True
    return False

kept = {s: c for s, c in cand.items() if not is_sub_of_longer(s)}
ordered = sorted(kept.items(), key=lambda kv: -kv[1])

print(f'文件 {N} 份，抽出候选专名 {len(ordered)} 个（DF>=3 且 <=15%）\n')
print('=== 前 45 个专名 ===')
for s, c in ordered[:45]:
    print(f'{c:4d}  {s}')

# ── 按专名分组（每个文件归给命中的「最长专名」）
owner = {}
for x in det:
    segs = segments(x['name'])
    best = None
    for s in segs:
        if s in kept and (best is None or (len(s), kept[s]) > (len(best), kept[best])):
            if best is None or len(s) > len(best):
                best = s
    owner[x['name']] = best

groups = collections.Counter(v for v in owner.values() if v)
print(f'\n=== 分组结果：{len(groups)} 组，覆盖 {sum(groups.values())}/{N} 份 ===')
for g, c in groups.most_common(25):
    print(f'{c:4d}  {g}')

uncovered = [k for k, v in owner.items() if not v]
print(f'\n未归入任何组: {len(uncovered)} 份')
print('样例:', ' | '.join(uncovered[:12]))

print('\n=== 抽样检查 3 个组的成员 ===')
for g, _ in groups.most_common(3):
    print(f'\n[{g}]')
    for k, v in owner.items():
        if v == g:
            print('   ', k[:56])
