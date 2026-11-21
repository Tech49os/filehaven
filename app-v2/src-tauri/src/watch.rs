//! 运行时监听（P2 M3）：源区有新文件就开始整理，而不是天天手点扫描
//!
//! 四条硬规矩，每一条都是踩过或算得出来的：
//!
//! 1. **必须排除归档根与隔离区**。迁移是「复制 + 校验 + 源文件进隔离区」，
//!    归档区那边每一次写入都会变成事件 —— 不过滤就是自激风暴，CPU 打满。
//! 2. **必须排除系统目录**（与扫描同一份黑名单），否则 Windows 的
//!    `C:\Windows\Temp` 一天能刷出几万个事件。
//! 3. **删除/改名只对 3 个状态生效**（`queued` / `labeled` / `pending_confirm`）。
//!    迁移会把源文件搬走，监听必然看到删除；按「未见即 missing」处理，
//!    会把刚迁移的整批文件打成丢失。
//! 4. **写库短事务、抢不到锁就丢批**，下一轮启动扫描会补上 ——
//!    监听线程永远不许为了写一行数据去等 UI。

use crate::scan::is_system_dir;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// 防抖窗口：复制 100 份文件会产生几百个事件，合并成一批再写库
pub const DEBOUNCE_MS: u64 = 2000;

/// 监听配置
#[derive(Clone, Debug, Default)]
pub struct WatchConfig {
    pub roots: Vec<PathBuf>,
    pub archive_root: Option<PathBuf>,
}

/// 一条路径该不该处理（纯函数，单测的主战场）
pub fn keep_path(path: &Path, cfg: &WatchConfig) -> bool {
    let s = path.to_string_lossy();
    let low = s.to_lowercase();

    // 归档根与隔离区：写事件全在这儿发生，看了就是自激
    if let Some(a) = &cfg.archive_root {
        let alow = a.to_string_lossy().to_lowercase();
        if !alow.is_empty() && low.starts_with(&alow) {
            return false;
        }
    }

    // 系统目录：路径上任意一段命中黑名单就丢
    for comp in path.components() {
        let name = comp.as_os_str().to_string_lossy();
        if !name.is_empty() && is_system_dir(&name) {
            return false;
        }
    }

    // 文件名层面的噪声
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    if name.is_empty() {
        return false;
    }
    let nlow = name.to_lowercase();
    if name.starts_with("~$") || name.starts_with('.') {
        return false;
    }
    for ext in [".tmp", ".crdownload", ".part", ".lnk"] {
        if nlow.ends_with(ext) {
            return false;
        }
    }
    // 压缩包/程序/系统文件：扫描入库时会被判 skipped，监听再报一次没有意义
    let ext = crate::scan::ext_of(&name);
    if crate::tag::kind_of_ext(&ext) == "other" && !ext.is_empty() {
        return false;
    }
    true
}

/// 一批事件的去重集合（同一个文件在 2s 内被写 5 次，只算 1 个）
#[derive(Default, Debug)]
pub struct Batch {
    /// 路径 → 最后一次事件的时刻（只用来去重，不参与判定）
    seen: HashMap<PathBuf, Instant>,
}

impl Batch {
    pub fn push(&mut self, p: PathBuf) {
        self.seen.insert(p, Instant::now());
    }

    pub fn len(&self) -> usize {
        self.seen.len()
    }

    pub fn is_empty(&self) -> bool {
        self.seen.is_empty()
    }

    pub fn take(&mut self) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = self.seen.drain().map(|(p, _)| p).collect();
        v.sort();
        v
    }
}

pub struct WatchGuard {
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
    /// 给测试用：已投递的批次计数
    pub batches: Arc<std::sync::atomic::AtomicUsize>,
}

impl WatchGuard {
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for WatchGuard {
    fn drop(&mut self) {
        self.stop();
    }
}

/// 起监听。`on_batch` 在防抖窗口结束后被调用一次，参数是这一批去重后的路径。
///
/// `window` 传 0 用默认 2s；测试里可以传小一点，不然每个用例都要等 2 秒。
pub fn start<F>(
    cfg: WatchConfig,
    window: Duration,
    on_batch: F,
) -> Result<WatchGuard, crate::error::CmdError>
where
    F: FnMut(Vec<PathBuf>) + Send + 'static,
{
    use notify::{RecursiveMode, Watcher};

    let (tx, rx) = channel::<PathBuf>();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(ev) = res {
            for p in ev.paths {
                let _ = tx.send(p);
            }
        }
    })
    .map_err(|e| crate::error::CmdError::new("WATCH_FAILED", format!("监听启动失败：{e}")))?;

    for root in &cfg.roots {
        if root.is_dir() {
            watcher
                .watch(root, RecursiveMode::Recursive)
                .map_err(|e| {
                    crate::error::CmdError::new(
                        "WATCH_FAILED",
                        format!("监听 {} 失败：{e}", root.display()),
                    )
                })?;
        }
    }

    let stop = Arc::new(AtomicBool::new(false));
    let batches = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let stop2 = stop.clone();
    let batches2 = batches.clone();
    let cfg2 = cfg.clone();
    let handle = std::thread::spawn(move || {
        // watcher 必须活在这个线程里，drop 掉监听就没了
        let _keep = watcher;
        let mut batch = Batch::default();
        let mut last = Instant::now();
        let mut on_batch = on_batch;
        loop {
            if stop2.load(Ordering::Relaxed) {
                break;
            }
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(p) => {
                    if keep_path(&p, &cfg2) {
                        batch.push(p);
                        last = Instant::now();
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            if !batch.is_empty() && last.elapsed() >= window {
                let paths = batch.take();
                batches2.fetch_add(1, Ordering::Relaxed);
                on_batch(paths);
                last = Instant::now();
            }
        }
    });

    Ok(WatchGuard {
        stop,
        handle: Some(handle),
        batches,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> WatchConfig {
        WatchConfig {
            roots: vec![],
            archive_root: Some(PathBuf::from("D:/文档归档")),
        }
    }

    #[test]
    fn archive_and_quarantine_are_ignored() {
        let c = cfg();
        assert!(!keep_path(Path::new("D:/文档归档/信科/合同/合同.docx"), &c));
        assert!(!keep_path(
            Path::new("D:/文档归档/.filebutler/quarantine/a.docx"),
            &c
        ));
        assert!(keep_path(Path::new("C:/Users/kingb/Desktop/合同.docx"), &c));
    }

    #[test]
    fn system_dirs_and_noise_are_ignored() {
        let c = cfg();
        assert!(!keep_path(Path::new("C:/Windows/Temp/a.docx"), &c));
        assert!(!keep_path(Path::new("C:/x/node_modules/pkg/a.docx"), &c));
        assert!(!keep_path(Path::new("C:/桌面/~$报告.docx"), &c));
        assert!(!keep_path(Path::new("C:/下载/大文件.crdownload"), &c));
        assert!(!keep_path(Path::new("C:/下载/包.zip"), &c));
        assert!(keep_path(Path::new("C:/桌面/报告.docx"), &c));
    }

    // 3 个文件在防抖窗口内 → 只回调一次，且带 3 条
    //
    // 注意目录位置：不能用 `tempfile::tempdir()`，它在 `AppData\\Local\\Temp` 下，
    // 而 AppData 是系统目录黑名单的一员 —— 事件会被正确丢掉，测试就永远收不到批。
    #[test]
    fn debounce_merges_a_burst_into_one_batch() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("watch-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let dir = root.clone();
        let cfg = WatchConfig {
            roots: vec![root.clone()],
            archive_root: None,
        };
        let _ = &dir;
        let (tx, rx) = channel::<Vec<PathBuf>>();
        // 1200ms：OS 事件投递偶发延迟（实测这台机器上能 >400ms），窗口太小
        // 会在「合并」断言上误报 —— 这里测的是合并语义，不是投递延迟
        let mut guard = start(cfg, Duration::from_millis(1200), move |paths| {
            let _ = tx.send(paths);
        })
        .unwrap();

        std::thread::sleep(Duration::from_millis(300));
        for n in ["a.docx", "b.docx", "c.docx"] {
            std::fs::write(root.join(n), b"x").unwrap();
            std::thread::sleep(Duration::from_millis(60));
        }

        let batch = rx.recv_timeout(Duration::from_secs(5)).expect("应该收到一批");
        assert_eq!(batch.len(), 3, "三个写入要合并成一批：{batch:?}");
        assert!(batch.iter().all(|p| p.to_string_lossy().contains(".docx")));

        // 停了就不再有回调
        guard.stop();
        std::fs::write(root.join("d.docx"), b"x").unwrap();
        assert!(
            rx.recv_timeout(Duration::from_millis(1500)).is_err(),
            "stop() 之后不许再有回调"
        );
    }
}
