"""量一次真实渲染：卡片内边距、字段块与边框的距离。

关掉 alpha.3 那类「文字贴边」的猜测 —— 直接问浏览器要 computed style。
"""
import os
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import cdp  # noqa: E402

APP = 'D:/filemanager/发布/文件管家_v0.3.0-alpha.5.exe'
STAMP = time.strftime('%H%M%S')
env = dict(os.environ)
env['FILEBUTLER_DATA_DIR'] = f'D:/filemanager/.e2e-data-{STAMP}'
env['WEBVIEW2_USER_DATA_FOLDER'] = f'D:/filemanager/.e2e-wv2-{STAMP}'
env['WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS'] = '--remote-debugging-port=9222'

subprocess.run([sys.executable, 'test/e2e-p1fix.py', 'seed'], check=True, env=env)
proc = subprocess.Popen([APP], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
try:
    cdp.wait_port(30)
    p = cdp.Page()
    time.sleep(4)
    p.eval("location.hash = '#/workspace'")
    time.sleep(1.5)
    # 扫描一次把文件弄进来
    p.eval("location.hash = '#/settings'")
    time.sleep(1.0)
    p.click_text('重新扫描', 'button', exact=False)
    time.sleep(6)
    p.eval("location.hash = '#/workspace'")
    time.sleep(2.5)

    out = p.eval(
        """(() => {
          const card = document.querySelector('.mid .card');
          if (!card) return { error: 'no card' };
          const cs = getComputedStyle(card);
          const cr = card.getBoundingClientRect();
          const ico = card.querySelector('.ico');
          const ir = ico ? ico.getBoundingClientRect() : null;
          const path = card.querySelector('.pathrow');
          const pr = path ? path.getBoundingClientRect() : null;
          return {
            padding: cs.padding,
            rect: [cr.left, cr.top, cr.width, cr.height],
            icoLeft: ir ? ir.left : null,
            icoInset: ir ? ir.left - cr.left : null,
            pathLeft: pr ? pr.left : null,
            pathInset: pr ? pr.left - cr.left : null,
            pathRightGap: pr ? cr.right - pr.right : null,
            keysTop: document.querySelector('.keys')?.getBoundingClientRect().top ?? null,
          };
        })()"""
    )
    print(out)
finally:
    subprocess.run(['taskkill', '/F', '/PID', str(proc.pid)], capture_output=True)
