# -*- coding: utf-8 -*-
"""
P3 T4.3 视觉闸门：真机 10 张真实语料图 → qwen3.5:9b 视觉通道 → 逐张记录结果。

判定口径：≥6/10 「可用」（标签与图对得上、或诚实返回空数组）才默认开「图片解读」。
脚本只负责跑和记录，「可用与否」由人工对照 JSON 逐条裁决，写回 verdict 字段。

硬约束（见 MEMORY）：
- 9b 独占显存：跑之前必须卸载其他驻留模型（keep_alive=0），加载后不再碰文本模型。
- 单张 15~33s、冷启动更慢是正常值，超时给 120s，别中途杀进程。
"""

import base64
import json
import sys
import time
import urllib.request

BASE = "http://127.0.0.1:11434"
MODEL = "qwen3.5:9b"

# 真实语料（desktop-scan.json 里的 17 张挑 10 张有代表性的）：
# 大屏截图 4 + 项目照片 2 + 普通截图/杂图 4。跑之前逐一验证存在。
CANDIDATES = [
    r"C:/Users/kingb/Desktop/归档/安环大屏1.png",
    r"C:/Users/kingb/Desktop/归档/安环大屏2.jpg",
    r"C:/Users/kingb/Desktop/归档/安环大屏3.jpg",
    r"C:/Users/kingb/Desktop/归档/安环大屏4.png",
    r"C:/Users/kingb/Desktop/归档/油化厂.png",
    r"C:/Users/kingb/Desktop/20260917-185927.jpg",
    r"C:/Users/kingb/Desktop/lchkhneagjjojibd.png",
    r"C:/Users/kingb/Desktop/归档/下载/20260908/缩小图片.png",
    r"C:/Users/kingb/Desktop/归档/下载/20260908/苹果风格后台.jpg",
    r"C:/Users/kingb/Desktop/归档/下载/20260908/下载.png",
]

PROMPT = (
    "看这张图片（文件名：{name}），回答：\n"
    "1. 这是一张什么图（照片/截图/图表/扫描件）；\n"
    "2. 与什么项目或事项相关；\n"
    "3. 给 3~5 个主题标签（2~6 字的通用名词短语，如「现场照片」「设备验收」），"
    "并用一句话说明这张图是什么（20~40 字）。\n"
    "看不清或没有依据就不要猜：返回空数组 []，summary 填空字符串。\n"
    '只输出 JSON：{{"tags":["…"],"summary":"…"}}'
)


def api(path, payload, timeout):
    req = urllib.request.Request(
        BASE + path,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.loads(r.read().decode("utf-8"))


def unload_others():
    """9b 要独占显存：先卸载所有驻留模型（不 kill serve 进程）。"""
    ps = urllib.request.urlopen(BASE + "/api/ps", timeout=10)
    loaded = [m["name"] for m in json.loads(ps.read().decode("utf-8")).get("models", [])]
    for m in loaded:
        print(f"卸载驻留模型 {m} ...")
        api("/api/generate", {"model": m, "prompt": "", "keep_alive": 0}, 30)


def pick_existing():
    ok = [p for p in CANDIDATES if __import__("os").path.isfile(p)]
    if len(ok) < 10:
        print(f"警告：候选只剩 {len(ok)} 张（有文件已不在原位）")
    return ok[:10]


def parse_body(raw):
    """与 Rust deep_tag::parse 同思路：去围栏、取首尾花括号，字段缺失按空处理。"""
    body = raw
    if "```" in body:
        parts = body.split("```")
        body = parts[1] if len(parts) > 1 else body
        if body.startswith("json"):
            body = body[4:]
    s, e = body.find("{"), body.rfind("}")
    if s < 0 or e <= s:
        return None
    try:
        d = json.loads(body[s : e + 1])
        tags = d.get("tags", [])
        if isinstance(tags, str):
            tags = [t.strip() for t in tags.replace("，", ",").split(",") if t.strip()]
        return {"tags": tags or [], "summary": (d.get("summary") or "").strip()}
    except Exception:
        return None


def main():
    out_path = "app-v2/test/vision-gate.json"
    files = pick_existing()
    unload_others()
    results = []
    for i, p in enumerate(files, 1):
        name = p.replace("\\", "/").rsplit("/", 1)[-1]
        with open(p, "rb") as f:
            b64 = base64.b64encode(f.read()).decode("ascii")
        print(f"[{i}/{len(files)}] {name} ...", flush=True)
        t0 = time.time()
        rec = {"file": p, "name": name, "size": len(b64) * 3 // 4}
        try:
            resp = api(
                "/api/generate",
                {
                    "model": MODEL,
                    "prompt": PROMPT.format(name=name),
                    "images": [b64],
                    "stream": False,
                    "format": "json",
                    "think": False,
                    "options": {"num_ctx": 4096, "temperature": 0.2},
                },
                120,
            )
            ms = int((time.time() - t0) * 1000)
            rec["ms"] = ms
            parsed = parse_body(resp.get("response", ""))
            if parsed is None:
                rec.update({"ok": False, "error": "bad_output", "raw": resp.get("response", "")[:200]})
            else:
                rec.update({"ok": True, "tags": parsed["tags"], "summary": parsed["summary"]})
        except Exception as e:  # noqa: BLE001 —— 闸门脚本，啥错都记下来
            rec.update({"ok": False, "error": f"{type(e).__name__}: {e}"})
        results.append(rec)
        print(json.dumps(rec, ensure_ascii=False)[:200], flush=True)

    good = sum(1 for r in results if r.get("ok") and (r.get("tags") or r.get("summary")))
    summary = {
        "model": MODEL,
        "count": len(results),
        "usable_raw": good,
        "verdict": None,  # 人工裁决后写回：可用张数 + 一句话理由
        "results": results,
    }
    with open(out_path, "w", encoding="utf-8") as f:
        json.dump(summary, f, ensure_ascii=False, indent=2)
    print(f"\n初筛（产出非空）{good}/{len(results)}，明细在 {out_path}，等人工裁决 verdict")
    return 0


if __name__ == "__main__":
    sys.exit(main())
