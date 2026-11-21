#!/usr/bin/env python3
"""T3.7 检索性能闸门：调 exe 的 --perf-search 造 5000 份临时库，量三类查询 p95。

判据（设计 §10.2「输入即出」）：三行 p95 全部 ≤ 100ms，超了就加索引或改 SQL，
不许把门槛放宽。数字落 test/perf-search.json 供 P3_执行状态.md 引用。
"""
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
EXE = ROOT / "src-tauri" / "target" / "debug" / "filemanager-v2.exe"
OUT = Path(__file__).resolve().parent / "perf-search.json"
LIMIT_MS = 100

def main() -> int:
    n = sys.argv[1] if len(sys.argv) > 1 else "5000"
    res = subprocess.run([str(EXE), "--perf-search", n], capture_output=True, text=True)
    if res.returncode != 0:
        print("FAIL: perf-search 退出码", res.returncode, res.stderr)
        return 1
    data = json.loads(res.stdout.strip().splitlines()[-1])
    rows = {
        "纯关键词": data["keywordP95Ms"],
        "单标签": data["tagP95Ms"],
        "标签+类型+时间": data["mixedP95Ms"],
    }
    ok = True
    print(f"造库 {data['count']} 份耗时 {data['insertMs']}ms（不含判据）")
    for name, ms in rows.items():
        verdict = "OK" if ms <= LIMIT_MS else "超了"
        if ms > LIMIT_MS:
            ok = False
        print(f"{name}: p95 = {ms}ms  {verdict}")
    data["limitMs"] = LIMIT_MS
    data["verdict"] = "pass" if ok else "fail"
    OUT.write_text(json.dumps(data, ensure_ascii=False, indent=1), encoding="utf-8")
    print("已写", OUT)
    return 0 if ok else 1

if __name__ == "__main__":
    sys.exit(main())
