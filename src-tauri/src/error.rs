use thiserror::Error;

/// 统一错误类型。后续解析/渲染模块的错误都收敛到这里，
/// 便于通过 `Result<T, AppError>` 直接返回给前端。
#[derive(Debug, Error)]
pub enum AppError {
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),

    #[error("解析失败: {0}")]
    Parse(String),

    #[error("不支持的格式版本: {0}")]
    UnsupportedVersion(String),

    #[error("数据损坏: {0}")]
    Corrupt(String),
}

pub type AppResult<T> = Result<T, AppError>;
