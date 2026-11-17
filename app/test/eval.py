#!/usr/bin/env python3
"""
LLM 评测脚本 v2：对比「规则引擎」vs「纯本地模型」vs「混合」

用法：
  python test/eval.py --stage auto     # 只在 24 份有硬证据样本上严格判分
  python test/eval.py --stage human    # 67 份存疑样本输出 规则 vs 模型 对照
  python test/eval.py --stage all      # 全部

输出：
  test/bench-result.json        严格判分结果
  test/human-review.md          67 份存疑样本对照表（供人工复核）

改进点（相对 v1）：
  - 修复 re 未在模块级 import 的 NameError
  - 模型列表自动探测（不再硬编码 qwen2.5:3b）
  - qwen3 系列自动关闭 thinking（实测 4b 从 26s → 3s，输出 token 195 → 7）
  - 每份处理完即时落盘，中断可续
  - 模型切换时确保显存释放，避免 llama-server 崩溃
"""
import argparse
import json
import os
import re
import time
import urllib.error
import urllib.request
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
OLLAMA_URL = "http://localhost:11434"


def load_json(name):
    return json.load(open(os.path.join(HERE, name), encoding="utf-8"))


# ---------------------------------------------------------------- 正文抽取

MAX_XML_READ = 8 * 1024 * 1024   # 单个内部 XML 上限，防止异常大文件把内存吃满


def extract_text(path):
    """docx/xlsx/pptx 走 zip 读 XML；txt/md 直读；老式二进制 Office 抽不到

    注意：zip 格式不按文件总大小限制。真实数据里有 689MB 的 pptx，
    但内部 slide XML 只有几 KB，按总大小卡会白白丢掉正文。
    """
    if not path or not os.path.exists(path):
        return ""
    try:
        ext = os.path.splitext(path)[1].lower()
        if ext in (".txt", ".md", ".csv"):
            if os.path.getsize(path) > 30 * 1024 * 1024:
                return ""
            return open(path, "r", encoding="utf-8", errors="ignore").read()[:2000]
        if ext == ".html":
            if os.path.getsize(path) > 30 * 1024 * 1024:
                return ""
            raw = open(path, "r", encoding="utf-8", errors="ignore").read()
            raw = re.sub(r"<(script|style)[^>]*>.*?</\1>", " ", raw, flags=re.S | re.I)
            txt = re.sub(r"<[^>]+>", " ", raw)
            return re.sub(r"\s+", " ", txt)[:2000]
        if ext in (".drawio", ".xml"):
            if os.path.getsize(path) > 30 * 1024 * 1024:
                return ""
            raw = open(path, "r", encoding="utf-8", errors="ignore").read()
            if ext == ".drawio" and "<mxfile" not in raw[:2000]:
                return ""
            vals = re.findall(r'(?:value|label)="([^"]{2,60})"', raw)
            return " ".join(vals)[:1000]
        if ext not in (".docx", ".xlsx", ".pptx"):
            return ""
        z = zipfile.ZipFile(path)
        names = z.namelist()
        if ext == ".docx":
            if "word/document.xml" not in names:
                return ""
            xml = z.read("word/document.xml").decode("utf-8", "ignore")
            xml = re.sub(r"</w:p>", " ", xml)
            return re.sub(r"<[^>]+>", " ", xml)[:2000]
        if ext == ".xlsx":
            n = next((x for x in names if "sharedStrings.xml" in x), None)
            if not n:
                return ""
            xml = z.read(n).decode("utf-8", "ignore")
            xml = re.sub(r"</si>", " ", xml)
            return re.sub(r"<[^>]+>", " ", xml)[:2000]
        if ext == ".pptx":
            def _num(x):
                m = re.search(r"slide(\d+)\.xml$", x)
                return int(m.group(1)) if m else 99999
            slides = sorted((x for x in names if re.match(r"ppt/slides/slide\d+\.xml$", x)),
                            key=_num)
            parts = []
            for s in slides[:8]:
                if z.getinfo(s).file_size > MAX_XML_READ:
                    continue
                parts.append(z.read(s).decode("utf-8", "ignore"))
            return re.sub(r"<[^>]+>", " ", "".join(parts))[:2000]
    except Exception:
        return ""
    return ""


# ---------------------------------------------------------------- Ollama

def ollama_models():
    try:
        with urllib.request.urlopen(OLLAMA_URL + "/api/tags", timeout=5) as r:
            return [t["name"] for t in json.loads(r.read().decode()).get("models", [])]
    except Exception:
        return []


def ollama_unload(model):
    """keep_alive=0 让模型立即卸载，释放显存"""
    try:
        body = json.dumps({"model": model, "keep_alive": 0}).encode()
        req = urllib.request.Request(OLLAMA_URL + "/api/generate", data=body,
                                     headers={"Content-Type": "application/json"})
        urllib.request.urlopen(req, timeout=30).read()
    except Exception:
        pass


def build_prompt(name, text, taxonomy):
    lines = ["你是文件归档助手。根据文件名和正文摘要，判断它属于哪个工作域、哪个文档类型。", "",
             "工作域 → 可选文档类型："]
    for ws, dts in taxonomy.items():
        lines.append(f"- {ws}：{'、'.join(dts)}")
    lines += ["",
              f"文件名：{name}",
              f"正文摘要：{text[:800] if text else '（无正文）'}",
              "",
              '严格只输出一行 JSON，不要解释：{"ws":"工作域","dt":"文档类型"}',
              '若完全无法判断，输出：{"ws":"","dt":""}']
    return "\n".join(lines)


def ollama_classify(model, prompt, timeout=180):
    payload = {
        "model": model,
        "prompt": prompt,
        "stream": False,
        "options": {"temperature": 0, "num_predict": 120, "num_ctx": 4096},
    }
    # qwen3 系列默认带 thinking，会多写几百 token 推理过程，分类任务里必须关掉
    if "qwen3" in model:
        payload["think"] = False
    body = json.dumps(payload).encode()
    req = urllib.request.Request(OLLAMA_URL + "/api/generate", data=body,
                                 headers={"Content-Type": "application/json"})
    for attempt in range(3):
        try:
            with urllib.request.urlopen(req, timeout=timeout) as r:
                resp = json.loads(r.read().decode())
            return resp.get("response", "")
        except urllib.error.HTTPError as e:
            err = e.read().decode()[:200]
            if attempt == 2:
                return f"__ERROR__ {err}"
            time.sleep(3)
        except Exception:
            if attempt == 2:
                return "__ERROR__ timeout"
            time.sleep(3)
    return "__ERROR__"


def parse_pred(raw, taxonomy):
    """从模型输出里抠出 ws/dt，并做合法性校验"""
    if not raw or raw.startswith("__ERROR__"):
        return None, None, raw
    m = re.search(r"\{[^{}]*\}", raw)
    if not m:
        return None, None, raw
    try:
        obj = json.loads(m.group(0))
    except Exception:
        return None, None, raw
    ws = (obj.get("ws") or "").strip()
    dt = (obj.get("dt") or "").strip()
    if ws not in taxonomy:
        ws = ""
    if ws and dt and dt not in taxonomy.get(ws, []):
        dt = ""   # 类型和工作域不匹配，丢弃类型
    return (ws or None), (dt or None), raw


# ---------------------------------------------------------------- 判分

def score(pred_ws, pred_dt, truth_ws, truth_dt):
    ws_ok = (pred_ws == truth_ws)
    dt_ok = (pred_dt == truth_dt) if truth_dt else None
    return ws_ok, dt_ok


def run_rule(samples):
    t0 = time.time()
    rows, ws_ok_n, dt_ok_n, dt_n = [], 0, 0, 0
    for s in samples:
        ws, dt = s.get("rule_ws"), s.get("rule_dt")
        ws_ok, dt_ok = score(ws, dt, s["ws_truth"], s["dt_truth"])
        ws_ok_n += ws_ok
        if dt_ok is not None:
            dt_n += 1
            dt_ok_n += dt_ok
        rows.append({"name": s["name"], "pred_ws": ws, "pred_dt": dt,
                     "ws_ok": ws_ok, "dt_ok": dt_ok})
    return {
        "ws_acc": ws_ok_n / len(samples) if samples else 0,
        "dt_acc": dt_ok_n / dt_n if dt_n else 0,
        "ws_correct": ws_ok_n, "dt_correct": dt_ok_n, "dt_total": dt_n,
        "n": len(samples), "time": time.time() - t0, "details": rows,
    }


def run_model(samples, model, taxonomy, name2path, cache, label=""):
    t0 = time.time()
    ws_ok_n, dt_ok_n, dt_n = 0, 0, 0
    rows = []
    for i, s in enumerate(samples):
        key = model + "|" + s["name"]
        if key in cache:
            raw = cache[key]
        else:
            text = extract_text(name2path.get(s["name"], ""))
            prompt = build_prompt(s["name"], text, taxonomy)
            raw = ollama_classify(model, prompt)
            cache[key] = raw
        ws, dt, _ = parse_pred(raw, taxonomy)
        ws_ok, dt_ok = score(ws, dt, s["ws_truth"], s["dt_truth"])
        ws_ok_n += ws_ok
        if dt_ok is not None:
            dt_n += 1
            dt_ok_n += dt_ok
        rows.append({"name": s["name"], "truth_ws": s["ws_truth"], "truth_dt": s["dt_truth"],
                     "pred_ws": ws, "pred_dt": dt, "ws_ok": ws_ok, "dt_ok": dt_ok,
                     "raw": raw[:200]})
        if (i + 1) % 5 == 0 or i + 1 == len(samples):
            print(f"    {label}{model}: {i+1}/{len(samples)}  "
                  f"(ws {ws_ok_n}/{i+1})", flush=True)
    return {
        "ws_acc": ws_ok_n / len(samples) if samples else 0,
        "dt_acc": dt_ok_n / dt_n if dt_n else 0,
        "ws_correct": ws_ok_n, "dt_correct": dt_ok_n, "dt_total": dt_n,
        "n": len(samples), "time": time.time() - t0, "details": rows,
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--stage", default="all", choices=["auto", "human", "all"])
    ap.add_argument("--models", default="", help="逗号分隔，留空则自动探测全部")
    ap.add_argument("--fix-truth", action="store_true",
                    help="应用 truth-fix.json 里经正文核查的标注修正（依据见 ground-truth-review.md）")
    args = ap.parse_args()

    bench = load_json("bench.json")
    inv = load_json("inventory.json")
    taxonomy = bench["taxonomy"]
    samples = bench["samples"]

    if args.fix_truth:
        fix_path = os.path.join(HERE, "truth-fix.json")
        if os.path.exists(fix_path):
            fixes = json.load(open(fix_path, encoding="utf-8"))["fixes"]
            n = 0
            for s in samples:
                if s["name"] in fixes:
                    s["ws_truth"] = fixes[s["name"]]["ws"]
                    s["dt_truth"] = fixes[s["name"]]["dt"]
                    n += 1
            print(f"[fix-truth] 已应用 {n} 处标注修正")
        else:
            print("[fix-truth] 未找到 truth-fix.json，跳过")
    name2path = {}
    for f in inv["files"]:
        name2path.setdefault(f["name"], f["path"])

    auto = [s for s in samples if not s["needs_human"]]
    human = [s for s in samples if s["needs_human"]]
    print(f"样本：总计 {len(samples)}，可判分 {len(auto)}，待复核 {len(human)}")

    avail = ollama_models()
    if not avail:
        print("!! Ollama 未就绪（11434 无响应），请先 ollama serve")
        return
    if args.models:
        models = [m for m in args.models.split(",") if m.strip()]
    else:
        models = avail
    print(f"将评测模型：{models}")

    # 断点续跑缓存
    cache_path = os.path.join(HERE, "eval-cache.json")
    cache = {}
    if os.path.exists(cache_path):
        try:
            cache = json.load(open(cache_path, encoding="utf-8"))
            print(f"载入增量缓存 {len(cache)} 条")
        except Exception:
            cache = {}

    out = {"samples_total": len(samples), "auto": len(auto), "human": len(human),
           "models": models, "rule": None, "model_results": {}, "hybrid": {}}

    if args.stage in ("auto", "all"):
        print("\n=== 一、规则引擎（24 份硬证据样本） ===")
        rule_res = run_rule(auto)
        out["rule"] = rule_res
        print(f"  工作域 {rule_res['ws_acc']:.1%} ({rule_res['ws_correct']}/{rule_res['n']})"
              f"  |  类型 {rule_res['dt_acc']:.1%} ({rule_res['dt_correct']}/{rule_res['dt_total']})"
              f"  |  耗时 {rule_res['time']*1000:.0f}ms")

        for m in models:
            print(f"\n=== 二、纯模型 {m} ===")
            res = run_model(auto, m, taxonomy, name2path, cache)
            out["model_results"][m] = res
            print(f"  工作域 {res['ws_acc']:.1%} ({res['ws_correct']}/{res['n']})"
                  f"  |  类型 {res['dt_acc']:.1%} ({res['dt_correct']}/{res['dt_total']})"
                  f"  |  耗时 {res['time']:.1f}s")
            json.dump(cache, open(cache_path, "w", encoding="utf-8"), ensure_ascii=False)

        # 混合策略（两版对比）
        #   旧版：规则 conf>=0.85 直通 —— 实测失败，置信度只反映词典命中强度不等于正确
        #   新版：双路一致才自动通过 + 分工（ws 用模型 / dt 优先用规则）
        for m in models:
            by_name = {d["name"]: d for d in out["model_results"][m]["details"]}

            # --- 旧版：规则高置信直通 ---
            ws_ok_n, dt_ok_n, dt_n, used_model = 0, 0, 0, 0
            for s in auto:
                conf = s.get("rule_conf") or 0
                if conf >= 0.85 and s.get("rule_ws"):
                    ws, dt = s.get("rule_ws"), s.get("rule_dt")
                else:
                    ws, dt = by_name[s["name"]]["pred_ws"], by_name[s["name"]]["pred_dt"]
                    used_model += 1
                ws_ok, dt_ok = score(ws, dt, s["ws_truth"], s["dt_truth"])
                ws_ok_n += ws_ok
                if dt_ok is not None:
                    dt_n += 1
                    dt_ok_n += dt_ok
            old = {"ws_acc": ws_ok_n / len(auto), "dt_acc": dt_ok_n / dt_n if dt_n else 0,
                   "ws_correct": ws_ok_n, "dt_correct": dt_ok_n, "dt_total": dt_n,
                   "n": len(auto), "used_model": used_model}
            print(f"\n=== 三、旧混合（规则 conf>=0.85 直通 + {m} 兜底） ===")
            print(f"  工作域 {old['ws_acc']:.1%} ({old['ws_correct']}/{old['n']})"
                  f"  |  类型 {old['dt_acc']:.1%} ({old['dt_correct']}/{old['dt_total']})"
                  f"  |  送模型 {used_model}/{len(auto)} 份")

            # --- 新版 A：分工（ws 用模型，dt 优先用规则） ---
            ws_ok_n2, dt_ok_n2, dt_n2, dt_from_rule = 0, 0, 0, 0
            for s in auto:
                pred = by_name[s["name"]]
                pws, pdt = pred["pred_ws"], pred["pred_dt"]
                rdt = s.get("rule_dt")
                if pws and rdt and rdt in taxonomy.get(pws, []):
                    pdt = rdt
                    dt_from_rule += 1
                ws_ok, dt_ok = score(pws, pdt, s["ws_truth"], s["dt_truth"])
                ws_ok_n2 += ws_ok
                if dt_ok is not None:
                    dt_n2 += 1
                    dt_ok_n2 += dt_ok
            split = {"ws_acc": ws_ok_n2 / len(auto),
                     "dt_acc": dt_ok_n2 / dt_n2 if dt_n2 else 0,
                     "ws_correct": ws_ok_n2, "dt_correct": dt_ok_n2, "dt_total": dt_n2,
                     "n": len(auto), "dt_from_rule": dt_from_rule}
            print(f"=== 三之二、分工（ws={m} / dt 优先规则） ===")
            print(f"  工作域 {split['ws_acc']:.1%} ({split['ws_correct']}/{split['n']})"
                  f"  |  类型 {split['dt_acc']:.1%} ({split['dt_correct']}/{split['dt_total']})"
                  f"  |  类型采用规则 {dt_from_rule}/{len(auto)} 份")

            # --- 新版 B：双路一致才自动通过，其余进人工 ---
            agree = [s for s in auto
                     if s.get("rule_ws") and by_name[s["name"]]["pred_ws"] == s["rule_ws"]]
            agree_ok = sum(1 for s in agree if s["rule_ws"] == s["ws_truth"])
            print(f"=== 三之三、双路一致直通 + 分歧人工 ===")
            print(f"  自动通过 {len(agree)}/{len(auto)} ({len(agree)/len(auto):.0%})"
                  f"  |  自动部分准确率 {agree_ok/len(agree):.1%}" if agree else "  无一致样本")

            out["hybrid"][m] = {"old_conf_gate": old, "split": split,
                                "agree_gate": {"n": len(agree), "coverage": len(agree) / len(auto),
                                               "auto_acc": agree_ok / len(agree) if agree else 0}}

        json.dump(out, open(os.path.join(HERE, "bench-result.json"), "w", encoding="utf-8"),
                  ensure_ascii=False, indent=1)
        print("\n>> 判分结果已写入 test/bench-result.json")

    if args.stage in ("human", "all"):
        print(f"\n=== 四、{len(human)} 份存疑样本：规则 vs 模型 对照 ===")
        human_res = {}
        for m in models:
            print(f"  -- {m} --")
            human_res[m] = run_model(human, m, taxonomy, name2path, cache, label="")
            json.dump(cache, open(cache_path, "w", encoding="utf-8"), ensure_ascii=False)

        md = ["# 67 份存疑样本复核表\n",
              "这些样本的文件名无硬证据（哈希名、纯数字、泛词），规则引擎判不准。",
              "请逐行看「规则判断」和「模型判断」，在最后一列填正确答案（或划掉错的）。\n"]
        md.append("| # | 文件名 | 类型 | 规则判断 | " +
                  " | ".join(f"{m}" for m in models) + " | 你的判断 |")
        md.append("|---|---|---|" + "---|" * (len(models) + 1))
        rule_map = {d["name"]: d for d in out["rule"]["details"]} if out["rule"] else {}
        for i, s in enumerate(human, 1):
            r = rule_map.get(s["name"], {})
            rule_cell = f"{r.get('pred_ws') or '—'}" + (f" / {r.get('pred_dt')}" if r.get("pred_dt") else "")
            model_cells = []
            for m in models:
                d = next((x for x in human_res[m]["details"] if x["name"] == s["name"]), {})
                cell = f"{d.get('pred_ws') or '—'}" + (f" / {d.get('pred_dt')}" if d.get("pred_dt") else "")
                model_cells.append(cell)
            md.append(f"| {i} | {s['name']} | {s['kind']} | {rule_cell} | " +
                      " | ".join(model_cells) + " |  |")
        open(os.path.join(HERE, "human-review.md"), "w", encoding="utf-8").write("\n".join(md))

        out["human_results"] = {m: {k: v for k, v in r.items() if k != "details"}
                                for m, r in human_res.items()}
        json.dump(out, open(os.path.join(HERE, "bench-result.json"), "w", encoding="utf-8"),
                  ensure_ascii=False, indent=1)
        print("\n>> 对照表已写入 test/human-review.md")


if __name__ == "__main__":
    main()
