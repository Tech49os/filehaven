"""跑一次隔离库的端到端验证，输出写到 test/e2e-<stage>-log.txt（不走管道，避免 tail 卡住）

每次用**新的**数据目录与 WebView2 目录，不做批量删除 —— 清理由人来做，
脚本只负责造数据。目录名带时间戳，跑完可以整目录删掉。

用法：
    python test/run-e2e.py            # 两段都跑：p1（42 条）+ p2（P2 新增）
    python test/run-e2e.py p1
    python test/run-e2e.py p2
    FILEBUTLER_EXE=... python test/run-e2e.py   # 指定 exe（默认取 发布/ 里最新的裸 exe）
"""
import os
import re
import subprocess
import sys
import time
from pathlib import Path

RELEASE = Path('D:/filemanager/发布')
STAMP = time.strftime('%H%M%S')


def pick_exe() -> str:
    env = os.environ.get('FILEBUTLER_EXE')
    if env:
        return env
    cands = []
    for p in RELEASE.glob('文件管家_v*.exe'):
        m = re.search(r'v(\d+)\.(\d+)\.(\d+)(?:-alpha\.(\d+))?', p.name)
        if not m:
            continue
        key = tuple(int(x) if x else 0 for x in m.groups())
        cands.append((key, p))
    if not cands:
        raise SystemExit(f'{RELEASE} 里没有可跑的裸 exe')
    cands.sort()
    return str(cands[-1][1])


STAGES = {
    'p1': ('test/e2e-p1fix.py', 9222),
    'p2': ('test/e2e-p2.py', 9223),
    'p3': ('test/e2e-p3.py', 9224),
}
# 需要启两次应用的阶段（启动增量 + 节流），脚本自己管进程
RESTART_STAGES = {
    'p2r': 'test/e2e-p2-restart.py',
}


def run_stage(stage: str, exe: str) -> int:
    script, port = STAGES[stage]
    data = f'D:/filemanager/.e2e-{stage}-data-{STAMP}'
    wv2 = f'D:/filemanager/.e2e-{stage}-wv2-{STAMP}'
    log = Path(f'D:/filemanager/app-v2/test/e2e-{stage}-log.txt')

    env = dict(os.environ)
    env['FILEBUTLER_DATA_DIR'] = data
    env['WEBVIEW2_USER_DATA_FOLDER'] = wv2
    env['WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS'] = f'--remote-debugging-port={port}'
    # cdp.py 从 CDP_PORT 读端口：两段同时用一个端口会互相看不到对方的页面
    env['CDP_PORT'] = str(port)

    print(f'\n========== {stage}  data={data} port={port} exe={exe}')
    subprocess.run([sys.executable, script, 'seed'], check=True, env=env)
    proc = subprocess.Popen([exe], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(3)
    try:
        r = subprocess.run(
            [sys.executable, script, 'verify'],
            env=env,
            capture_output=True,
            text=True,
            timeout=900,
        )
        log.write_text(r.stdout + '\n=== STDERR ===\n' + r.stderr, encoding='utf-8')
        out = r.stdout
        print(out[-6000:])
        passes = len(re.findall(r'^\s*PASS', out, re.M))
        fails = len(re.findall(r'^\s*FAIL', out, re.M))
        print(f'---------- {stage}: PASS {passes} / FAIL {fails}（日志 {log}）')
        return r.returncode
    finally:
        subprocess.run(['taskkill', '/F', '/PID', str(proc.pid)], capture_output=True)


def run_restart_stage(stage: str, exe: str) -> int:
    script = RESTART_STAGES[stage]
    data = f'D:/filemanager/.e2e-{stage}-data-{STAMP}'
    log = Path(f'D:/filemanager/app-v2/test/e2e-{stage}-log.txt')
    env = dict(os.environ)
    env['FILEBUTLER_DATA_DIR'] = data
    print(f'\n========== {stage}  data={data} exe={exe}')
    r = subprocess.run(
        [sys.executable, script, exe], env=env, capture_output=True, text=True, timeout=600
    )
    log.write_text(r.stdout + '\n=== STDERR ===\n' + r.stderr, encoding='utf-8')
    print(r.stdout[-4000:])
    passes = len(re.findall(r'^\s*PASS', r.stdout, re.M))
    fails = len(re.findall(r'^\s*FAIL', r.stdout, re.M))
    print(f'---------- {stage}: PASS {passes} / FAIL {fails}（日志 {log}）')
    return r.returncode


def main() -> int:
    stage = sys.argv[1] if len(sys.argv) > 1 else 'all'
    exe = pick_exe()
    print(f'exe={exe}')
    todo = ['p1', 'p2', 'p2r', 'p3'] if stage == 'all' else [stage]
    for s in todo:
        if s not in STAGES and s not in RESTART_STAGES:
            raise SystemExit(f'未知阶段：{s}')
    rc = 0
    for s in todo:
        if s in RESTART_STAGES:
            rc |= run_restart_stage(s, exe)
        else:
            rc |= run_stage(s, exe)
    return rc


if __name__ == '__main__':
    sys.exit(main())
