#!/usr/bin/env python3
"""
视觉能力对比测试：验证 qwen3.5:4b 的 vision 能否补上"文件名无信息"的短板。

对每张图片跑两遍：
  A. 只看文件名（复现旧 eval.py 的行为，正文抽取对图片返回空）
  B. 文件名 + 图片本身（Ollama images 字段传 base64）

对照：qwen2.5:7b / qwen3:4b 无 vision 能力，B 模式对它们不可用。
"""
import base64
import json
import os
import re
import sys
import time
import urllib.error
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
OLLAMA_URL = "http://localhost:11434"
MODEL = sys.argv[1] if len(sys.argv) > 1 else "qwen3.5:4b"
OUT = sys.argv[2] if len(sys.argv) > 2 else "vision-result.json"

IMAGES = [
    ("20260917-185927.jpg", r"C:\Users\kingb\Desktop\20260917-185927.jpg",
     "智能工厂智匠智能体海报（CNOOC）", "智能工厂平台"),
    ("苹果风格后台.jpg", r"C:\Users\kingb\Desktop\归档\下载\20260908\苹果风格后台.jpg",
     "中国海油智能体应用开发平台-菜单管理后台", "智能工厂平台"),
    ("海油发展智能工厂系统应用图标生成.png",
     r"C:\Users\kingb\Desktop\归档\下载\20260908\海油发展智能工厂系统应用图标生成.png",
     "海油发展智能工厂系统图标（海上平台+齿轮）", "智能工厂平台"),
    ("cd847e97a66c7c186d5e8e3dbd4f51a7.jpeg",
     r"C:\Users\kingb\Desktop\文件夹\cd847e97a66c7c186d5e8e3dbd4f51a7.jpeg",
     "智能问数机器人+数据看板小图", "智能工厂平台"),
]


def build_prompt(name, taxonomy, with_image):
    lines = ["你是文件归档助手。根据文件名" + ("和图片内容" if with_image else "") +
             "，判断它属于哪个工作域、哪个文档类型。", "",
             "工作域 → 可选文档类型："]
    for ws, dts in taxonomy.items():
        lines.append(f"- {ws}：{'、'.join(dts)}")
    lines += ["", f"文件名：{name}"]
    if with_image:
        lines += ["", "请仔细识别图片中的文字和画面主题再判断。"]
    lines += ["", '严格只输出一行 JSON：{"ws":"工作域","dt":"文档类型"}',
              '若完全无法判断，输出：{"ws":"","dt":""}']
    return "\n".join(lines)


def classify(prompt, img_path=None, timeout=180):
    payload = {
        "model": MODEL,
        "prompt": prompt,
        "stream": False,
        # 图片会被编码成大量 vision token，num_ctx 太小会直接 400（实测 4329 token > 4096）
        "options": {"temperature": 0, "num_predict": 120, "num_ctx": 8192},
        "think": False,
    }
    if img_path:
        with open(img_path, "rb") as f:
            payload["images"] = [base64.b64encode(f.read()).decode()]
    req = urllib.request.Request(
        OLLAMA_URL + "/api/generate",
        data=json.dumps(payload).encode(),
        headers={"Content-Type": "application/json"})
    t0 = time.time()
    for attempt in range(3):
        try:
            with urllib.request.urlopen(req, timeout=timeout) as r:
                resp = json.loads(r.read().decode())
            return resp.get("response", ""), time.time() - t0
        except urllib.error.HTTPError as e:
            err = e.read().decode()[:300]
            if attempt == 2:
                return f"__ERROR__ {err}", time.time() - t0
            time.sleep(2)
        except Exception as e:
            if attempt == 2:
                return f"__ERROR__ {e}", time.time() - t0
            time.sleep(2)
    return "__ERROR__", time.time() - t0


def parse(raw, taxonomy):
    m = re.search(r"\{[^{}]*\}", raw or "")
    if not m:
        return None, None
    try:
        obj = json.loads(m.group(0))
    except Exception:
        return None, None
    ws = (obj.get("ws") or "").strip()
    dt = (obj.get("dt") or "").strip()
    if ws not in taxonomy:
        ws = ""
    if ws and dt and dt not in taxonomy.get(ws, []):
        dt = ""
    return ws or None, dt or None


def main():
    bench = json.load(open(os.path.join(HERE, "bench.json"), encoding="utf-8"))
    taxonomy = bench["taxonomy"]
    print(f"模型：{MODEL}（vision={True}）\n")
    results = []
    for name, path, truth_desc, truth_ws in IMAGES:
        if not os.path.exists(path):
            print(f"!! 文件不存在：{path}")
            continue
        print(f"=== {name} ===")
        print(f"  真实内容（我人工看的）：{truth_desc} → 应属 {truth_ws}")
        row = {"name": name, "truth_desc": truth_desc, "truth_ws": truth_ws}
        for mode, with_img in (("A_仅文件名", False), ("B_看图", True)):
            prompt = build_prompt(name, taxonomy, with_img)
            raw, sec = classify(prompt, path if with_img else None)
            ws, dt = parse(raw, taxonomy)
            ok = (ws == truth_ws)
            print(f"  {mode}: ws={ws or '空'} / dt={dt or '空'}  {'✓对' if ok else '✗错'}  ({sec:.1f}s)")
            print(f"      raw={raw[:150]}")
            row[mode] = {"ws": ws, "dt": dt, "ok": ok, "sec": sec, "raw": raw[:300]}
        results.append(row)
        print()
    json.dump(results, open(os.path.join(HERE, OUT), "w", encoding="utf-8"),
              ensure_ascii=False, indent=1)
    a_ok = sum(1 for r in results if r["A_仅文件名"]["ok"])
    b_ok = sum(1 for r in results if r["B_看图"]["ok"])
    print(f"汇总：仅文件名 {a_ok}/{len(results)} 正确 | 看图 {b_ok}/{len(results)} 正确")
    print(f">> {OUT} 已写入")


if __name__ == "__main__":
    main()
