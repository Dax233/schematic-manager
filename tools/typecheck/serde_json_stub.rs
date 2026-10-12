//! serde_json 的最小替身。
//!
//! 命令层若用到 to_string / from_str 等，这里给出同名同签名的函数，
//! 使类型检查能覆盖调用处的参数与返回值，而不涉及实际序列化行为。

use crate::serde::{Deserialize, Serialize};

pub fn to_string<T: Serialize>(_v: &T) -> Result<String, String> {
    Ok(String::new())
}

pub fn from_str<T: for<'de> Deserialize<'de>>(_s: &str) -> Result<T, String> {
    Err("桩不支持反序列化".into())
}

pub fn to_value<T: Serialize>(_v: T) -> Result<Value, String> {
    Ok(Value::Null)
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Value>),
    Object,
}
