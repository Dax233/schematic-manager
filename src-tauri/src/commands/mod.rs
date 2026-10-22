//! Tauri 命令层：前端可调用的全部入口。
//!
//! 按业务域拆成子模块；命令只做参数校验、调用解析层、把结果转成 DTO，
//! 不在这里承载业务规则，便于解析层独立测试。

pub(crate) mod app;
pub(crate) mod blocks;
pub(crate) mod convert;
mod dto;
pub(crate) mod duplicates;
pub(crate) mod files;
pub(crate) mod log;
pub(crate) mod schematic;
pub(crate) mod setup;
mod shared;
pub(crate) mod state;
