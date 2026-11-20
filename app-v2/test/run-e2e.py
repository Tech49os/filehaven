"""跑一次隔离库的端到端验证，输出写到 test/e2e-log.txt（不走管道，避免 tail 卡住）

每次用**新的**数据目录与 WebView2 目录，不做批量删除 —— 清理由人来做，
脚本只负责造数据。目录名带时间戳，跑完可以整目录删掉。
"""
import os
import subprocess
import sys
import time
from pathlib import Path

APP = 'D:/filemanager/发布/文件管家_v0.3.0-alpha.5.exe'
LOG = Path('D:/filemanager/app-v2/test/e2e-log.txt')
STAMP = time.strftime('%H%M%S')
DATA = f'D:/filemanager/.e2e-data-{STAMP}'
WV2 = f'D:/filemanager/.e2e-wv2-{STAMP}'

env = dict(os.environ)
env['FILEBUTLER_DATA_DIR'] = DATA
env['WEBVIEW2_USER_DATA_FOLDER'] = WV2
env['WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS'] = '--remote-debugging-port=9222'

print(f'data={DATA} wv2={WV2}')
subprocess.run([sys.executable, 'test/e2e-p1fix.py', 'seed'], check=True, env=env)
proc = subprocess.Popen([APP], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
time.sleep(3)
try:
    r = subprocess.run(
        [sys.executable, 'test/e2e-p1fix.py', 'verify'],
        env=env,
        capture_output=True,
        text=True,
        timeout=900,
    )
    LOG.write_text(r.stdout + '\n=== STDERR ===\n' + r.stderr, encoding='utf-8')
    print(r.stdout[-5000:])
    sys.exit(r.returncode)
finally:
    subprocess.run(['taskkill', '/F', '/PID', str(proc.pid)], capture_output=True)
