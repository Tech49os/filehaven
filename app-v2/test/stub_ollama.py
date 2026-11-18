"""最小 Ollama 桩：专用来验「降级三型」里的两支（零依赖，只用标准库）。

为什么需要它：MODEL_TIMEOUT 和 MODEL_BAD_OUTPUT 这两条路径没法靠真模型稳定复现
——真模型要么答对，要么慢到 180s（等一次 3 分钟，且不可控）。桩可以钉死行为。

用法：
    python test/stub_ollama.py <port> bad-json     # 回一段 Markdown 表格（不听 JSON 格式）
    python test/stub_ollama.py <port> slow         # 睡 20s 再回，用来撞客户端超时

配合：
    FILEBUTLER_BASE=http://127.0.0.1:<port> FILEBUTLER_TIMEOUT=3 \
        ./src-tauri/target/debug/filemanager-v2.exe --suggest test/fixtures/real-scan.json out.json

预期：bad-json → MODEL_BAD_OUTPUT；slow + 3s → MODEL_TIMEOUT。
（MODEL_DOWN 不用桩：把 base 指到 127.0.0.1:9 就是。）
"""

import json
import sys
import time
from http.server import BaseHTTPRequestHandler, HTTPServer

PORT = int(sys.argv[1])
MODE = sys.argv[2] if len(sys.argv) > 2 else "bad-json"

BAD_JSON = "当然可以！下面是我的建议：\n\n| 目录 | 依据 |\n|---|---|\n| 安全检查 | 检查报告 |\n"


class Handler(BaseHTTPRequestHandler):
    def _send(self, obj):
        body = json.dumps(obj, ensure_ascii=False).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if self.path.startswith("/api/tags"):
            self._send({"models": []})
        elif self.path.startswith("/api/ps"):
            self._send({"models": []})
        else:
            self._send({})

    def do_POST(self):
        length = int(self.headers.get("Content-Length") or 0)
        self.rfile.read(length)  # 请求体用不上，读掉就行
        if MODE == "slow":
            time.sleep(20)
        if MODE == "bad-json":
            self._send({"response": BAD_JSON})
        else:
            self._send({"response": '{"schemes":[]}'})

    def log_message(self, *args):
        pass  # 安静，别把每次 /api/tags 都刷到终端


if __name__ == "__main__":
    print(f"stub ollama on 127.0.0.1:{PORT} mode={MODE}", flush=True)
    HTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
