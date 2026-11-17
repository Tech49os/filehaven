#!/usr/bin/env python3
"""
构建 LLM 评测基准（ground truth）
分层抽样：覆盖每个工作域 + 重点覆盖规则引擎的弱项（低置信/未分类/文件名无信息）
粗标策略：仅用文件名里的硬证据标注，主观不确定的标为待复核
"""
import json
import os
import random

random.seed(42)

HERE = os.path.dirname(os.path.abspath(__file__))
DETAILS = json.load(open(os.path.join(HERE, "real-classify.json"), encoding="utf-8"))["details"]

# 工作域 → 文档类型 的合法清单（与 classifier 保持一致，作为 ground truth 的取值域）
TAXONOMY = {
    "智能工厂平台": ["方案设计", "汇报材料", "会议纪要", "成本报价", "制度规范", "检查应急", "参考素材"],
    "钻采院项目": ["方案设计", "成本报价", "汇报材料", "会议纪要"],
    "油化厂项目": ["汇报材料", "成本报价", "单据表单", "会议纪要"],
    "人事培训": ["考核人事", "制度规范", "单据表单", "参考素材"],
    "部门管理": ["考核人事", "方案设计", "会议纪要", "制度规范"],
    "商务采购": ["合同协议", "成本报价", "单据表单"],
    "财务报销": ["单据表单", "成本报价"],
    "安全合规": ["检查应急", "制度规范", "汇报材料"],
    "知识沉淀": ["参考素材", "制度规范"],
}


def bucket(d):
    """把文件归入抽样桶"""
    if not d["ws"] or d["needReview"] or d["conf"] < 0.7:
        return "weak"  # 规则弱项，重点抽
    return f"ws:{d['ws']}"


groups = {}
for d in DETAILS:
    groups.setdefault(bucket(d), []).append(d)

print("=== 各桶数量 ===")
for k, v in sorted(groups.items(), key=lambda x: -len(x[1])):
    print(f"  {k:24s} {len(v)}")

# 抽样配额
QUOTA = {"weak": 40}  # 弱项抽 40
# 每个工作域抽 6 个（若不足则全取）
for ws in TAXONOMY:
    QUOTA[f"ws:{ws}"] = 6

picked = []
for key, quota in QUOTA.items():
    pool = groups.get(key, [])
    if not pool:
        continue
    random.shuffle(pool)
    picked.extend(pool[:quota])

# 去重（同一文件只出现一次）
seen = set()
uniq = []
for d in picked:
    if d["name"] in seen:
        continue
    seen.add(d["name"])
    uniq.append(d)

print(f"\n共抽取 {len(uniq)} 份样本")


def guess_ws(d):
    """仅凭文件名硬证据粗标工作域，无把握返回 None（待复核）"""
    name = d["name"]
    if "钻采院" in name or "钻采" in name:
        return "钻采院项目"
    if "油化厂" in name or "油田化工" in name or "油化" in name:
        return "油化厂项目"
    if "智能工厂" in name or "生产智能化" in name or "智能感知" in name or "工业互联网" in name:
        return "智能工厂平台"
    if any(k in name for k in ["差旅费", "报销", "出差申请", "费用", "发票"]):
        return "财务报销"
    if any(k in name for k in ["合同", "协议", "报价", "承诺书", "委托书", "采购"]):
        return "商务采购"
    if any(k in name for k in ["简历", "培训", "考核", "绩效", "人员", "招聘", "申报表", "内训"]):
        return "人事培训"
    if any(k in name for k in ["安全", "应急", "检查", "隐患", "预案", "月报"]):
        return "安全合规"
    return None


def guess_dt(d):
    """仅凭文件名粗标文档类型"""
    name = d["name"]
    if any(k in name for k in ["报价", "成本", "工作量", "清单", "预算"]):
        return "成本报价"
    if any(k in name for k in ["合同", "协议", "承诺书", "委托书"]):
        return "合同协议"
    if any(k in name for k in ["汇报", "月报", "进展", "周报", "提纲", "双周"]):
        return "汇报材料"
    if any(k in name for k in ["会议", "纪要", "推进会", "评审"]):
        return "会议纪要"
    if any(k in name for k in ["方案", "计划", "规划", "设计"]):
        return "方案设计"
    if any(k in name for k in ["制度", "规范", "标准", "办法", "规定"]):
        return "制度规范"
    if any(k in name for k in ["检查", "应急", "预案", "安全"]):
        return "检查应急"
    if any(k in name for k in ["简历", "考核", "绩效", "人员", "申报表"]):
        return "考核人事"
    if any(k in name for k in ["审批单", "申请表", "表单", "单据", "出差"]):
        return "单据表单"
    return None


out = []
for d in uniq:
    ws = guess_ws(d)
    dt = guess_dt(d)
    # 合法性校验：若类型不在该工作域合法清单内，则类型留空（交给复核）
    if ws and dt and ws in TAXONOMY and dt not in TAXONOMY[ws]:
        dt = None
    out.append({
        "name": d["name"],
        "kind": d["kind"],
        "ws_truth": ws,
        "dt_truth": dt,
        "rule_ws": d["ws"],
        "rule_dt": d["dt"],
        "rule_conf": d["conf"],
        "need_review": d["needReview"],
        "needs_human": ws is None or dt is None,  # 粗标没把握 → 需你复核
    })

# 分类统计
human = [o for o in out if o["needs_human"]]
auto = [o for o in out if not o["needs_human"]]
print(f"粗标完成：{len(auto)} 份自动标注（有硬证据），{len(human)} 份需你复核")

out_path = os.path.join(HERE, "bench.json")
json.dump(
    {"taxonomy": TAXONOMY, "samples": out},
    open(out_path, "w", encoding="utf-8"),
    ensure_ascii=False,
    indent=1,
)
print(f"已写出: {out_path}")

# 打印需复核清单，方便用户逐条看
print("\n=== 需你复核的样本（粗标没把握）===")
for o in human:
    print(f"  [{o['kind']:5s}] {o['name'][:52]}")
    if o["ws_truth"]:
        print(f"        粗标工作域={o['ws_truth']}（类型待定）")
