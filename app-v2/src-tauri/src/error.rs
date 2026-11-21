//! 结构化错误：前端按 `code` 分支，不再匹配中文文案

use serde::Serialize;
use std::fmt;

pub const IO: &str = "IO";
pub const DB: &str = "DB";
pub const BAD_TRANSITION: &str = "BAD_TRANSITION";
pub const DIR_EXISTS: &str = "DIR_EXISTS";
pub const DIR_INVALID: &str = "DIR_INVALID";
pub const ARCHIVE_IN_SOURCE: &str = "ARCHIVE_IN_SOURCE";
pub const SOURCE_UNREADABLE: &str = "SOURCE_UNREADABLE";
pub const FILE_CHANGED: &str = "FILE_CHANGED";
pub const MIGRATE_FAILED: &str = "MIGRATE_FAILED";
/// 归档盘剩余空间不够（R7：迁移前就拦住，别搬到一半炸）
pub const DISK_FULL: &str = "DISK_FULL";
pub const NOT_CONFIGURED: &str = "NOT_CONFIGURED";
pub const MODEL_DOWN: &str = "MODEL_DOWN";
pub const MODEL_TIMEOUT: &str = "MODEL_TIMEOUT";
pub const MODEL_BAD_OUTPUT: &str = "MODEL_BAD_OUTPUT";
/// 模型闸被别人占着（目录建议 / 视觉解读），等满超时还没拿到——本轮跳过，不是文件的问题
pub const MODEL_BUSY: &str = "MODEL_BUSY";
/// 文件太少，不值得问模型（少于 10 份）
pub const TOO_FEW_FILES: &str = "TOO_FEW_FILES";
/// 输入不合法（空标签名、超长目录名一类）
pub const BAD_INPUT: &str = "BAD_INPUT";
/// 这个标签不许删（`type` / `time` 是基础标签，删了界面就没法按类型/时间找了）
pub const TAG_LOCKED: &str = "TAG_LOCKED";

#[derive(Debug, Clone, Serialize)]
pub struct CmdError {
    pub code: String,
    pub message: String,
}

impl CmdError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
        }
    }
}

impl fmt::Display for CmdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for CmdError {}

impl From<std::io::Error> for CmdError {
    fn from(e: std::io::Error) -> Self {
        CmdError::new(IO, e.to_string())
    }
}

impl From<rusqlite::Error> for CmdError {
    fn from(e: rusqlite::Error) -> Self {
        CmdError::new(DB, e.to_string())
    }
}
