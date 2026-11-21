# -*- coding: utf-8 -*-
"""P4 M2 扫描件视觉闸门：10 份真实无文字层 PDF 走完整管线（渲染第 1 页 → 9b → 解析）。

判据沿用 P3 T4.3：≥6/10 产出可用标签 → 扫描件解读默认开。
跑之前先卸载驻留模型（9b 要独占 8GB 显存）。
"""
import json
import os
import subprocess
import sys
import time
import urllib.request

EXE = os.path.join(os.path.dirname(__file__), "..", "src-tauri", "target", "debug", "filemanager-v2.exe")
OUT = os.path.join(os.path.dirname(__file__), "vision-pdf-gate.json")
OLLAMA = "http://127.0.0.1:11434"

PDFS = [
    r"C:\Users\kingb\Desktop\GB_T 22111-2008_895.pdf",
    r"C:\Users\kingb\Desktop\【合同】智能工厂数字化系统深化建设项目--中海油信息科技有限公司.pdf",
    r"C:\Users\kingb\Desktop\归档\20260916\数智化提升行动方案.pdf",
    r"C:\Users\kingb\Desktop\归档\20260916\智能工厂技术室-开发组全员全栈转型心得材料.pdf",
    r"C:\Users\kingb\Desktop\归档\下载\20260908\业务联络函-关于水下采油树生产管理系统建设项目工作委托.pdf",
    r"C:\Users\kingb\Desktop\归档\下载\20260908\业务联络函-物流惠州.pdf",
    r"C:\Users\kingb\Desktop\归档\下载\20260908\井下三期-委托.pdf",
    r"C:\Users\kingb\Desktop\归档\下载\20260908\海油发展工程技术公司-服务器资源申请单.pdf",
    r"C:\Users\kingb\Desktop\归档\下载\20260908\采办谈判记录-智慧工技智慧实验子模块运维20260623.pdf",
    r"C:\Users\kingb\Desktop\归档\下载\20260908\附件2：服务器领用交接单 -（信科天分）.pdf",
]


def unload_all():
    req = urllib.request.Request(OLLAMA + "/api/ps")
    loaded = json.loads(urllib.request.urlopen(req, timeout=5).read()).get("models", [])
    for m in loaded:
        name = m.get("name") or m.get("model")
        print(f"卸载驻留模型 {name} …")
        r = urllib.request.Request(
            OLLAMA + "/api/generate",
            data=json.dumps({"model": name, "prompt": "", "keep_alive": 0}).encode(),
            headers={"Content-Type": "application/json"},
        )
        urllib.request.urlopen(r, timeout=30).read()


def main():
    exe = os.path.abspath(EXE)
    if not os.path.isfile(exe):
        print("exe 不存在，先 cargo build --bin filemanager-v2")
        sys.exit(2)
    unload_all()
    results = []
    for p in PDFS:
        name = os.path.basename(p)
        t0 = time.time()
        print(f"[{len(results)+1}/10] {name[:40]} …")
        try:
            r = subprocess.run(
                [exe, "--vision-one", p],
                capture_output=True,
                text=True,
                encoding="utf-8",
                errors="replace",
                timeout=180,
            )
            line = (r.stdout or "").strip().splitlines()
            data = json.loads(line[-1]) if line else {"error": "no-output", "stderr": (r.stderr or "")[-300:]}
        except Exception as e:
            data = {"error": "exception", "message": str(e)}
        data["name"] = name
        data["path"] = p
        data["wall_s"] = round(time.time() - t0, 1)
        data["tags"] = data.get("tags") or []
        data["usable"] = bool(data["tags"]) and "error" not in data
        results.append(data)
        tagtxt = ",".join(data["tags"]) or (data.get("error") or "")
        print(f"    {data['wall_s']}s → {tagtxt[:70]}")
    usable = sum(1 for r in results if r["usable"])
    out = {"usable": usable, "total": len(results), "results": results,
           "verdict": "", "verdict_usable": None}
    with open(OUT, "w", encoding="utf-8") as f:
        json.dump(out, f, ensure_ascii=False, indent=2)
    print(f"\n可用 {usable}/{len(results)}（判据 ≥6）→ 结果在 {OUT}")


if __name__ == "__main__":
    main()
