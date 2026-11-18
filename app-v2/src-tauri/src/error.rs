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
pub const NOT_CONFIGURED: &str = "NOT_CONFIGURED";
pub const MODEL_DOWN: &str = "MODEL_DOWN";
pub const MODEL_TIMEOUT: &str = "MODEL_TIMEOUT";
pub const MODEL_BAD_OUTPUT: &str = "MODEL_BAD_OUTPUT";
/// 文件太少，不值得问模型（少于 10 份）
pub const TOO_FEW_FILES: &str = "TOO_FEW_FILES";

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
