//! 全局状态：单连接 + Mutex。
//!
//! 铁律（详设 §5.1）：绝不在持锁期间做 IO —— 扫描、哈希、HTTP 全部在锁外完成，
//! 只在落库的几百毫秒内持锁。

use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

pub struct AppState {
    pub db: Mutex<Connection>,
    pub app_dir: PathBuf,
    /// 上一次模型探活的结果。探活是同步 HTTP（最长 3s），不能挂在
    /// 「每次判定后刷新计数」这条高频路径上，所以只缓存、按需刷新。
    pub model_reachable: AtomicBool,
    /// 监听开关：关掉时事件照样收，但一律丢弃（重起重落的复杂度不值得）
    pub watch_enabled: AtomicBool,
}

impl AppState {
    pub fn new(db: Connection, app_dir: PathBuf) -> Self {
        Self {
            db: Mutex::new(db),
            app_dir,
            model_reachable: AtomicBool::new(false),
            watch_enabled: AtomicBool::new(true),
        }
    }
}
