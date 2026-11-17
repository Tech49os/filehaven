#!/usr/bin/env python3
"""
PDF 内容识别三路对比（用 Python 3.14 + pymupdf）：
  A. 只看文件名（现有 eval.py 做法，PDF 正文抽取返回空）
  B. 文本层解析：pymupdf 抽文字 → 喂文本模型 qwen2.5:7b
  C. 首页渲染成图 → vision 模型 qwen3.5:4b 看图

覆盖两类 PDF：有文本层的、扫描件/图片型的。
注意：本脚本用 C:/Python314/python.exe 运行（pymupdf 装在 3.14）。
"""
import base64
import json
import os
import re
import subprocess
import sys
import time
import urllib.error
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
OLLAMA_URL = "http://localhost:11434"
VISION_MODEL = sys.argv[1] if len(sys.argv) > 1 else "qwen3.5:4b"
OUT = sys.argv[2] if len(sys.argv) > 2 else "pdf-vision-result.json"

# 有真值的样本（从 bench.json）。其余无真值，只做定性观察。
PDFS = [
    ("合同签字版.pdf", r"C:\Users\kingb\Desktop\归档\合同签字版.pdf", "商务采购"),
    ("关于成立海油发展数字化运营中心的通知.pdf",
     r"C:\Users\kingb\Desktop\归档\下载\20260908\关于成立海油发展数字化运营中心的通知.pdf", None),
    ("智能工厂人员安排.pdf", r"C:\Users\kingb\Desktop\管理\智能工厂人员安排.pdf", "智能工厂平台"),
    ("质量之星申报_从单兵作战到数字军团.pdf",
     r"C:\Users\kingb\Desktop\质量之星\质量之星申报_从单兵作战到数字军团.pdf", None),
    ("GB_T 22111-2008_895.pdf", r"C:\Users\kingb\Desktop\GB_T 22111-2008_895.pdf", None),
    ("数智化提升行动方案.pdf", r"C:\Users\kingb\Desktop\归档\20260916\数智化提升行动方案.pdf", None),
]


def ollama(prompt, model, image_path=None, timeout=180):
    payload = {"model": model, "prompt": prompt, "stream": False,
               "options": {"temperature": 0, "num_predict": 120, "num_ctx": 8192},
               "think": False}
    if image_path:
        with open(image_path, "rb") as f:
            payload["images"] = [base64.b64encode(f.read()).decode()]
    req = urllib.request.Request(OLLAMA_URL + "/api/generate",
                                 data=json.dumps(payload).encode(),
                                 headers={"Content-Type": "application/json"})
    for attempt in range(3):
        try:
            with urllib.request.urlopen(req, timeout=timeout) as r:
                return json.loads(r.read().decode()).get("response", "")
        except urllib.error.HTTPError as e:
            err = e.read().decode()[:200]
            if attempt == 2:
                return f"__ERROR__ {err}"
            time.sleep(2)
        except Exception as e:
            if attempt == 2:
                return f"__ERROR__ {e}"
            time.sleep(2)
    return "__ERROR__"


def build_prompt(name, text, taxonomy, with_image=False):
    lines = ["你是文件归档助手。根据文件名" +
             ("和图片内容" if with_image else "和正文摘要") + "，判断它属于哪个工作域、哪个文档类型。",
             "", "工作域 → 可选文档类型："]
    for ws, dts in taxonomy.items():
        lines.append(f"- {ws}：{'、'.join(dts)}")
    lines += ["", f"文件名：{name}"]
    if with_image:
        lines += ["", "请仔细识别图片中的文字和画面主题再判断。"]
    else:
        lines += [f"正文摘要：{text[:800] if text else '（无正文）'}"]
    lines += ["", '严格只输出一行 JSON：{"ws":"工作域","dt":"文档类型"}',
              '若完全无法判断，输出：{"ws":"","dt":""}']
    return "\n".join(lines)


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
    import pymupdf  # Python 3.14
    bench = json.load(open(os.path.join(HERE, "bench.json"), encoding="utf-8"))
    taxonomy = bench["taxonomy"]
    results = []
    for name, path, truth_ws in PDFS:
        if not os.path.exists(path):
            print(f"!! 不存在 {path}")
            continue
        print(f"\n=== {name}  (真值ws={truth_ws or '无'}) ===")
        doc = pymupdf.open(path)
        # 文本层：取前3页
        text = "".join(doc[i].get_text() for i in range(min(3, len(doc))))
        text = re.sub(r"\s+", " ", text).strip()
        print(f"  文本层抽取: {len(text)} 字 — {text[:90] if text else '（空，扫描件）'}")
        # 首页渲染成图（dpi=100 控制体积）
        img_path = os.path.join(HERE, "_pdf_page.png")
        pix = doc[0].get_pixmap(dpi=100)
        pix.save(img_path)
        doc.close()

        row = {"name": name, "truth_ws": truth_ws, "text_len": len(text), "text_head": text[:120]}
        # A 仅文件名
        raw_a = ollama(build_prompt(name, "", taxonomy), "qwen2.5:7b")
        wa, da = parse(raw_a, taxonomy)
        print(f"  A 仅文件名(7b): ws={wa or '空'} / dt={da or '空'}")
        row["A"] = {"ws": wa, "dt": da}
        # B 文本层 → 7b
        if text:
            raw_b = ollama(build_prompt(name, text, taxonomy), "qwen2.5:7b")
            wb, db = parse(raw_b, taxonomy)
            print(f"  B 文本层→7b    : ws={wb or '空'} / dt={db or '空'}")
        else:
            wb = db = None
            print(f"  B 文本层→7b    : 无可抽文本，跳过")
        row["B"] = {"ws": wb, "dt": db}
        # C 首页图 → vision 模型
        raw_c = ollama(build_prompt(name, "", taxonomy, with_image=True), VISION_MODEL, img_path)
        wc, dc = parse(raw_c, taxonomy)
        print(f"  C 看图→3.5     : ws={wc or '空'} / dt={dc or '空'}  {raw_c[:100] if str(raw_c).startswith('__ERROR__') else ''}")
        row["C"] = {"ws": wc, "dt": dc, "raw": raw_c[:200]}
        results.append(row)
        if truth_ws:
            for k, w in (("A", wa), ("B", wb), ("C", wc)):
                row[k]["ok"] = (w == truth_ws)

    json.dump(results, open(os.path.join(HERE, OUT), "w", encoding="utf-8"),
              ensure_ascii=False, indent=1)
    print(f"\n>> {OUT} 已写入")
    for k in ("A", "B", "C"):
        scored = [r for r in results if k in r and "ok" in r[k]]
        if scored:
            print(f"  有真值样本 {k} 正确: {sum(1 for r in scored if r[k]['ok'])}/{len(scored)}")


if __name__ == "__main__":
    main()
