//! thiserror 的桩：本机无法获取该依赖，而类型检查不需要衍生宏。
//! 形状与 src-tauri/src/error.rs 一致，供其余模块检查使用。
use std::fmt;

#[derive(Debug)]
pub enum AppError {
    Io(std::io::Error),
    Parse(String),
    UnsupportedVersion(String),
    Corrupt(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::Io(e) => write!(f, "IO 错误: {e}"),
            AppError::Parse(s) => write!(f, "解析失败: {s}"),
            AppError::UnsupportedVersion(s) => write!(f, "不支持的格式版本: {s}"),
            AppError::Corrupt(s) => write!(f, "数据损坏: {s}"),
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self { AppError::Io(e) }
}

pub type AppResult<T> = Result<T, AppError>;
