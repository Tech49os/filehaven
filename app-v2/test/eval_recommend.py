#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""推荐评测汇总（纯标准库）：读 `--eval` 产出的 out.json，算 top1/top3/无候选。

用法：
    python test/eval_recommend.py out.json            # 按现行阈值汇总
    python test/eval_recommend.py out.json --grid     # 附阈值网格（UNCERTAIN_MIN × noCandidate）

口径
----
* `rank` 由后端给出，是「有理由的候选」按分数降序的名次（1 起，null = 没进榜）。
  它**与阈值无关** —— 阈值只决定「第一名算主推还是待你指定」，不改排序。
  所以 top1/top3 拿来比「改前改后」是干净的。
* 无候选 = 后端 `needsYou=true`（真实走的是 `candidates()`，与界面同一条路）。
"""

import json
import sys

PRIMARY = 0.35
UNCERTAIN_MIN = 0.20


def ratio(n, d):
    return 0.0 if d == 0 else n * 100.0 / d


def summarize(payload, uncertain_min=UNCERTAIN_MIN, label=""):
    cases = payload["cases"]
    n = len(cases)
    top1 = sum(1 for c in cases if c["rank"] == 1)
    top3 = sum(1 for c in cases if c["rank"] is not None and c["rank"] <= 3)
    none = sum(
        1
        for c in cases
        if not c["ranked"] or c["ranked"][0]["score"] < uncertain_min
    )
    deep = [c for c in cases if c.get("hasDeepTags")]
    shallow = [c for c in cases if not c.get("hasDeepTags")]
    return {
        "label": label,
        "n": n,
        "top1": ratio(top1, n),
        "top3": ratio(top3, n),
        "noCandidate": ratio(none, n),
        "top1_n": top1,
        "top3_n": top3,
        "none_n": none,
        "deep_n": len(deep),
        "shallow_n": len(shallow),
    }


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    path = sys.argv[1]
    grid = "--grid" in sys.argv
    with open(path, encoding="utf-8") as f:
        payload = json.load(f)

    s = summarize(payload, label="现行阈值")
    print(
        f"cases={s['n']} dirs={payload.get('dirCount')}  "
        f"top1={s['top1']:.1f}% ({s['top1_n']}/{s['n']})  "
        f"top3={s['top3']:.1f}% ({s['top3_n']}/{s['n']})  "
        f"noCandidate={s['noCandidate']:.1f}% ({s['none_n']}/{s['n']})"
    )
    print(f"分组：有深度标签 {s['deep_n']} 份 / 无 {s['shallow_n']} 份")

    # 漏掉的 case 打出来：改法的方向要看着这些名字定
    misses = [c for c in payload["cases"] if c["rank"] != 1]
    if misses:
        print(f"\n未中 top1 的 {len(misses)} 条：")
        for c in misses[:12]:
            best = c["ranked"][0] if c["ranked"] else None
            got = best["relPath"].split("/")[-1] if best else "（无候选）"
            sc = f"{best['score']:.2f}" if best else "-"
            print(f"  × {c['fileName'][:34]:34s} 期望={c['expectDir'].split('/')[-1]:10s} "
                  f"实得={got:10s} {sc}")

    if grid:
        print("\n阈值网格（UNCERTAIN_MIN → 无候选占比；排名与阈值无关）：")
        print("| UNCERTAIN_MIN | 无候选 | 说明 |")
        print("|---|---|---|")
        for i in range(0, 11):
            t = round(i * 0.05, 2)
            g = summarize(payload, uncertain_min=t)
            print(f"| {t:.2f} | {g['noCandidate']:.1f}% | {'P1 现行' if abs(t - 0.20) < 1e-9 else ''} |")
    return 0


if __name__ == "__main__":
    sys.exit(main())
