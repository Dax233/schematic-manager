//! serde 的最小替身，编译成独立 crate。
//!
//! 只需要让 `use serde::{Deserialize, Serialize};` 能解析。
//! 派生宏在生成副本里已被剥离，因此这里只提供 trait。

pub trait Serialize {}
pub trait Deserialize<'de> {}

impl Serialize for String {}
impl Serialize for &str {}
impl Serialize for u8 {}
impl Serialize for u16 {}
impl Serialize for u32 {}
impl Serialize for u64 {}
impl Serialize for i32 {}
impl Serialize for usize {}
impl Serialize for bool {}
impl Serialize for f32 {}
impl Serialize for f64 {}
impl<T: Serialize> Serialize for Vec<T> {}
impl<T: Serialize> Serialize for Option<T> {}
impl<T: Serialize> Serialize for [T] {}
impl<T: Serialize, const N: usize> Serialize for [T; N] {}
impl Serialize for () {}

impl<'de> Deserialize<'de> for String {}
impl<'de> Deserialize<'de> for u8 {}
impl<'de> Deserialize<'de> for u16 {}
impl<'de> Deserialize<'de> for u32 {}
impl<'de> Deserialize<'de> for u64 {}
impl<'de> Deserialize<'de> for i32 {}
impl<'de> Deserialize<'de> for usize {}
impl<'de> Deserialize<'de> for bool {}
impl<'de> Deserialize<'de> for f32 {}
impl<'de> Deserialize<'de> for f64 {}
impl<'de, T: Deserialize<'de>> Deserialize<'de> for Vec<T> {}
impl<'de, T: Deserialize<'de>> Deserialize<'de> for Option<T> {}
impl<'de, T: Deserialize<'de>, const N: usize> Deserialize<'de> for [T; N] {}
impl<'de> Deserialize<'de> for () {}
