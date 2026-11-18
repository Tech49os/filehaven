//! 全局状态：单连接 + Mutex。
//!
//! 铁律（详设 §5.1）：绝不在持锁期间做 IO —— 扫描、哈希、HTTP 全部在锁外完成，
//! 只在落库的几百毫秒内持锁。

use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::Mutex;

pub struct AppState {
    pub db: Mutex<Connection>,
    pub app_dir: PathBuf,
}

impl AppState {
    pub fn new(db: Connection, app_dir: PathBuf) -> Self {
        Self {
            db: Mutex::new(db),
            app_dir,
        }
    }
}
