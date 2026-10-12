//! Minecraft NBT（大端序）读写 + gzip 容器。
//!
//! 语义与 legacy-cpp/nbt.cpp 保持一致：
//!   - 全部整数/浮点均为大端序
//!   - Compound 使用有序 Vec 而非 HashMap，保证序列化后字段顺序不变
//!     （投影文件的字节级往返依赖这一点）
//!   - gzip 容器自动识别：若前两字节为 1f 8b 则先解压

use crate::error::{AppError, AppResult};
use std::io::{Read, Write};

pub const TAG_END: u8 = 0;
pub const TAG_BYTE: u8 = 1;
pub const TAG_SHORT: u8 = 2;
pub const TAG_INT: u8 = 3;
pub const TAG_LONG: u8 = 4;
pub const TAG_FLOAT: u8 = 5;
pub const TAG_DOUBLE: u8 = 6;
pub const TAG_BYTE_ARRAY: u8 = 7;
pub const TAG_STRING: u8 = 8;
pub const TAG_LIST: u8 = 9;
pub const TAG_COMPOUND: u8 = 10;
pub const TAG_INT_ARRAY: u8 = 11;
pub const TAG_LONG_ARRAY: u8 = 12;

/// NBT 取值。字段顺序敏感的 Compound 用有序 Vec 表示。
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<u8>),
    String(String),
    /// 列表。第二项为**文件声明的元素类型**：
    /// 空列表在序列化时无法从元素推断类型，必须原样保留，
    /// 否则「元素类型 Int、长度 0」的列表往返后会变成「元素类型 End」。
    List(Vec<Value>, u8),
    Compound(Vec<(String, Value)>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

impl Value {
    pub fn type_id(&self) -> u8 {
        match self {
            Value::Byte(_) => TAG_BYTE,
            Value::Short(_) => TAG_SHORT,
            Value::Int(_) => TAG_INT,
            Value::Long(_) => TAG_LONG,
            Value::Float(_) => TAG_FLOAT,
            Value::Double(_) => TAG_DOUBLE,
            Value::ByteArray(_) => TAG_BYTE_ARRAY,
            Value::String(_) => TAG_STRING,
            Value::List(..) => TAG_LIST,
            Value::Compound(_) => TAG_COMPOUND,
            Value::IntArray(_) => TAG_INT_ARRAY,
            Value::LongArray(_) => TAG_LONG_ARRAY,
        }
    }

    pub fn type_name(&self) -> &'static str {
        tag_type_name(self.type_id())
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Compound(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// 存在则替换（保持原位置），否则追加到末尾。
    pub fn set(&mut self, key: &str, value: Value) {
        if let Value::Compound(fields) = self {
            if let Some(slot) = fields.iter_mut().find(|(k, _)| k == key) {
                slot.1 = value;
                return;
            }
            fields.push((key.to_string(), value));
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Byte(v) => Some(*v as i64),
            Value::Short(v) => Some(*v as i64),
            Value::Int(v) => Some(*v as i64),
            Value::Long(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Float(v) => Some(*v as f64),
            Value::Double(v) => Some(*v),
            _ => self.as_i64().map(|v| v as f64),
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&[Value]> {
        match self {
            Value::List(v, _) => Some(v),
            _ => None,
        }
    }

    pub fn as_longs(&self) -> Option<&[i64]> {
        match self {
            Value::LongArray(v) => Some(v),
            _ => None,
        }
    }

    /// 读取 {x,y,z} 三元组，缺失时返回 None。
    pub fn as_xyz(&self) -> Option<[i32; 3]> {
        let get = |k: &str| self.get(k).and_then(|v| v.as_i64()).map(|v| v as i32);
        Some([get("x")?, get("y")?, get("z")?])
    }
}

/// 解码 Java modified UTF-8。
///
/// Minecraft 的 NBT 字符串由 DataOutputStream.writeUTF 写出，用的是
/// modified UTF-8，有两类字节序列不是合法 UTF-8：NUL 写作 C0 80，
/// 补充平面字符（如 emoji）写作 CESU-8 代理对（每个 6 字节）。
/// 用 String::from_utf8_lossy 会把它们替换成 U+FFFD 并改变长度，
/// 再序列化就永久改写了文件内容。
fn decode_modified_utf8(bytes: &[u8]) -> AppResult<String> {
    let bad = || AppError::Corrupt("NBT 字符串不是合法的 modified UTF-8".into());
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if b & 0x80 == 0 {
            out.push(b as char);
            i += 1;
            continue;
        }
        let (cp, len) = if b & 0xF0 == 0xE0 {
            if i + 2 >= bytes.len() { return Err(bad()); }
            ((((b & 0x0F) as u32) << 12)
                | (((bytes[i + 1] & 0x3F) as u32) << 6)
                | ((bytes[i + 2] & 0x3F) as u32), 3)
        } else if b & 0xE0 == 0xC0 {
            if i + 1 >= bytes.len() { return Err(bad()); }
            ((((b & 0x1F) as u32) << 6) | ((bytes[i + 1] & 0x3F) as u32), 2)
        } else {
            return Err(bad());
        };
        if (0xD800..0xDC00).contains(&cp) {
            let j = i + len;
            if j + 2 >= bytes.len() || bytes[j] & 0xF0 != 0xE0 { return Err(bad()); }
            let lo = (((bytes[j] & 0x0F) as u32) << 12)
                | (((bytes[j + 1] & 0x3F) as u32) << 6)
                | ((bytes[j + 2] & 0x3F) as u32);
            if !(0xDC00..0xE000).contains(&lo) { return Err(bad()); }
            let combined = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
            out.push(char::from_u32(combined).ok_or_else(bad)?);
            i = j + 3;
            continue;
        }
        if (0xDC00..0xE000).contains(&cp) { return Err(bad()); }
        out.push(char::from_u32(cp).ok_or_else(bad)?);
        i += len;
    }
    Ok(out)
}

/// 编码为 Java modified UTF-8，与 decode_modified_utf8 互为逆运算。
fn encode_modified_utf8(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for ch in s.chars() {
        let cp = ch as u32;
        if cp == 0 {
            out.extend_from_slice(&[0xC0, 0x80]);
        } else if cp < 0x80 {
            out.push(cp as u8);
        } else if cp < 0x800 {
            out.push(0xC0 | (cp >> 6) as u8);
            out.push(0x80 | (cp & 0x3F) as u8);
        } else if cp < 0x10000 {
            out.push(0xE0 | (cp >> 12) as u8);
            out.push(0x80 | ((cp >> 6) & 0x3F) as u8);
            out.push(0x80 | (cp & 0x3F) as u8);
        } else {
            let v = cp - 0x10000;
            for c in [0xD800 + (v >> 10), 0xDC00 + (v & 0x3FF)] {
                out.push(0xE0 | (c >> 12) as u8);
                out.push(0x80 | ((c >> 6) & 0x3F) as u8);
                out.push(0x80 | (c & 0x3F) as u8);
            }
        }
    }
    out
}

pub fn tag_type_name(t: u8) -> &'static str {
    match t {
        TAG_END => "End",
        TAG_BYTE => "Byte",
        TAG_SHORT => "Short",
        TAG_INT => "Int",
        TAG_LONG => "Long",
        TAG_FLOAT => "Float",
        TAG_DOUBLE => "Double",
        TAG_BYTE_ARRAY => "ByteArray",
        TAG_STRING => "String",
        TAG_LIST => "List",
        TAG_COMPOUND => "Compound",
        TAG_INT_ARRAY => "IntArray",
        TAG_LONG_ARRAY => "LongArray",
        _ => "Unknown",
    }
}

// ---------------------------------------------------------------- 读取

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn take(&mut self, n: usize) -> AppResult<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| AppError::Corrupt("长度溢出".into()))?;
        if end > self.data.len() {
            return Err(AppError::Corrupt(format!(
                "数据不足：需要 {} 字节，位置 {}，总长 {}",
                n,
                self.pos,
                self.data.len()
            )));
        }
        let s = &self.data[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn u8(&mut self) -> AppResult<u8> {
        Ok(self.take(1)?[0])
    }
    fn i8(&mut self) -> AppResult<i8> {
        Ok(self.u8()? as i8)
    }
    fn i16(&mut self) -> AppResult<i16> {
        let b = self.take(2)?;
        Ok(i16::from_be_bytes([b[0], b[1]]))
    }
    fn u16(&mut self) -> AppResult<u16> {
        Ok(self.i16()? as u16)
    }
    fn i32(&mut self) -> AppResult<i32> {
        let b = self.take(4)?;
        Ok(i32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn i64(&mut self) -> AppResult<i64> {
        let b = self.take(8)?;
        Ok(i64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
    }
    fn f32(&mut self) -> AppResult<f32> {
        Ok(f32::from_bits(self.i32()? as u32))
    }
    fn f64(&mut self) -> AppResult<f64> {
        Ok(f64::from_bits(self.i64()? as u64))
    }

    /// NBT 字符串：u16 字节长度 + modified UTF-8 字节
    fn string(&mut self) -> AppResult<String> {
        let len = self.u16()? as usize;
        let b = self.take(len)?;
        decode_modified_utf8(b)
    }

    fn payload(&mut self, tag: u8, depth: u32) -> AppResult<Value> {
        if depth > 512 {
            return Err(AppError::Corrupt("NBT 嵌套过深".into()));
        }
        Ok(match tag {
            TAG_BYTE => Value::Byte(self.i8()?),
            TAG_SHORT => Value::Short(self.i16()?),
            TAG_INT => Value::Int(self.i32()?),
            TAG_LONG => Value::Long(self.i64()?),
            TAG_FLOAT => Value::Float(self.f32()?),
            TAG_DOUBLE => Value::Double(self.f64()?),
            TAG_BYTE_ARRAY => {
                let n = self.i32()?;
                if n < 0 {
                    return Err(AppError::Corrupt("ByteArray 长度为负".into()));
                }
                Value::ByteArray(self.take(n as usize)?.to_vec())
            }
            TAG_STRING => Value::String(self.string()?),
            TAG_LIST => {
                let elem = self.u8()?;
                let n = self.i32()?;
                if n < 0 {
                    return Err(AppError::Corrupt("List 长度为负".into()));
                }
                let mut items = Vec::with_capacity(n.min(4096) as usize);
                for _ in 0..n {
                    items.push(self.payload(elem, depth + 1)?);
                }
                Value::List(items, elem)
            }
            TAG_COMPOUND => {
                let mut fields = Vec::new();
                loop {
                    let t = self.u8()?;
                    if t == TAG_END {
                        break;
                    }
                    let name = self.string()?;
                    let v = self.payload(t, depth + 1)?;
                    fields.push((name, v));
                }
                Value::Compound(fields)
            }
            TAG_INT_ARRAY => {
                let n = self.i32()?;
                if n < 0 {
                    return Err(AppError::Corrupt("IntArray 长度为负".into()));
                }
                let mut v = Vec::with_capacity(n.min(1 << 20) as usize);
                for _ in 0..n {
                    v.push(self.i32()?);
                }
                Value::IntArray(v)
            }
            TAG_LONG_ARRAY => {
                let n = self.i32()?;
                if n < 0 {
                    return Err(AppError::Corrupt("LongArray 长度为负".into()));
                }
                let mut v = Vec::with_capacity(n.min(1 << 20) as usize);
                for _ in 0..n {
                    v.push(self.i64()?);
                }
                Value::LongArray(v)
            }
            other => {
                return Err(AppError::Corrupt(format!(
                    "未知的 NBT 标签类型 {other}"
                )))
            }
        })
    }
}

/// 解析以「具名根标签」开头的 NBT（.litematic 即此形式）。
pub fn parse(data: &[u8]) -> AppResult<Value> {
    let mut r = Reader::new(data);
    let tag = r.u8()?;
    if tag == TAG_END {
        return Err(AppError::Corrupt("根标签为 TAG_End".into()));
    }
    let _name = r.string()?;
    r.payload(tag, 0)
}

// ---------------------------------------------------------------- 写入

fn write_payload(out: &mut Vec<u8>, v: &Value) {
    match v {
        Value::Byte(x) => out.push(*x as u8),
        Value::Short(x) => out.extend_from_slice(&x.to_be_bytes()),
        Value::Int(x) => out.extend_from_slice(&x.to_be_bytes()),
        Value::Long(x) => out.extend_from_slice(&x.to_be_bytes()),
        Value::Float(x) => out.extend_from_slice(&x.to_bits().to_be_bytes()),
        Value::Double(x) => out.extend_from_slice(&x.to_bits().to_be_bytes()),
        Value::ByteArray(b) => {
            out.extend_from_slice(&(b.len() as i32).to_be_bytes());
            out.extend_from_slice(b);
        }
        Value::String(s) => {
            let b = encode_modified_utf8(s);
            out.extend_from_slice(&(b.len() as u16).to_be_bytes());
            out.extend_from_slice(&b);
        }
        Value::List(items, elem) => {
            // 原样写回解析时读到的元素类型。
            // 不能改用 items.first() 推断：空列表没有元素，会退化成 End，
            // 使「元素类型 Int、长度 0」的列表被改写。
            out.push(*elem);
            out.extend_from_slice(&(items.len() as i32).to_be_bytes());
            for it in items {
                write_payload(out, it);
            }
        }
        Value::Compound(fields) => {
            for (k, val) in fields {
                out.push(val.type_id());
                let kb = k.as_bytes();
                out.extend_from_slice(&(kb.len() as u16).to_be_bytes());
                out.extend_from_slice(kb);
                write_payload(out, val);
            }
            out.push(TAG_END);
        }
        Value::IntArray(v) => {
            out.extend_from_slice(&(v.len() as i32).to_be_bytes());
            for x in v {
                out.extend_from_slice(&x.to_be_bytes());
            }
        }
        Value::LongArray(v) => {
            out.extend_from_slice(&(v.len() as i32).to_be_bytes());
            for x in v {
                out.extend_from_slice(&x.to_be_bytes());
            }
        }
    }
}

/// 序列化为具名根标签形式的 NBT 字节。
pub fn serialize(root: &Value, root_name: &str) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(root.type_id());
    let nb = root_name.as_bytes();
    out.extend_from_slice(&(nb.len() as u16).to_be_bytes());
    out.extend_from_slice(nb);
    write_payload(&mut out, root);
    out
}

// ---------------------------------------------------------------- gzip

pub fn gunzip(data: &[u8]) -> AppResult<Vec<u8>> {
    let mut d = flate2::read::GzDecoder::new(data);
    let mut out = Vec::new();
    d.read_to_end(&mut out)?;
    Ok(out)
}

pub fn gzip(data: &[u8], level: u32) -> AppResult<Vec<u8>> {
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(level));
    e.write_all(data)?;
    Ok(e.finish()?)
}

/// 自动识别输入是否已 gzip 压缩，然后解析。
pub fn parse_maybe_gzip(file: &[u8]) -> AppResult<Value> {
    let raw = if file.len() >= 2 && file[0] == 0x1f && file[1] == 0x8b {
        gunzip(file)?
    } else {
        file.to_vec()
    };
    parse(&raw)
}

/// 序列化并 gzip 压缩。
pub fn serialize_gzip(root: &Value, root_name: &str, level: u32) -> AppResult<Vec<u8>> {
    gzip(&serialize(root, root_name), level)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Value {
        Value::Compound(vec![
            ("name".into(), Value::String("测试".into())),
            ("count".into(), Value::Int(-42)),
            ("flag".into(), Value::Byte(1)),
            ("ratio".into(), Value::Double(0.5)),
            (
                "list".into(),
                Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)], TAG_INT),
            ),
            (
                "nested".into(),
                Value::Compound(vec![("x".into(), Value::Int(3)), ("y".into(), Value::Int(4))]),
            ),
            ("longs".into(), Value::LongArray(vec![i64::MIN, -1, 0, 1, i64::MAX])),
        ])
    }

    #[test]
    fn modified_utf8_roundtrip_covers_specials() {
        // NUL 与补充平面字符是 modified UTF-8 与普通 UTF-8 的分歧点
        for t in ["", "abc", "中文测试", "a\u{0}b", "emoji\u{1F600}end", "\u{10FFFF}"] {
            let enc = encode_modified_utf8(t);
            assert_eq!(decode_modified_utf8(&enc).expect("应能解码"), t, "往返必须一致");
        }
        // NUL 必须写成 C0 80，而不是原始 0x00
        assert_eq!(encode_modified_utf8("\u{0}"), vec![0xC0, 0x80]);
        // 补充平面必须写成 CESU-8 代理对（6 字节）
        assert_eq!(encode_modified_utf8("\u{1F600}").len(), 6);
    }

    #[test]
    fn modified_utf8_string_survives_nbt_roundtrip() {
        let v = Value::Compound(vec![
            ("emoji".into(), Value::String("笑脸\u{1F600}".into())),
            ("nul".into(), Value::String("a\u{0}b".into())),
        ]);
        assert_eq!(parse(&serialize(&v, "")).unwrap(), v);
    }

    #[test]
    fn invalid_modified_utf8_is_rejected() {
        // 孤立低代理、非法首字节、截断的多字节序列都应显式报错，
        // 而不是替换成 U+FFFD 后继续
        assert!(decode_modified_utf8(&[0xED, 0xB0, 0x80]).is_err());
        assert!(decode_modified_utf8(&[0xF8, 0x80]).is_err());
        assert!(decode_modified_utf8(&[0xE0, 0x80]).is_err());
    }

    #[test]
    fn empty_list_keeps_declared_element_type() {
        // 「元素类型 Int、长度 0」的列表往返后必须仍是 Int。
        // 若改用 items.first() 推断元素类型，空列表会退化成 End。
        let v = Value::Compound(vec![
            ("emptyInts".into(), Value::List(Vec::new(), TAG_INT)),
            ("emptyStrs".into(), Value::List(Vec::new(), TAG_STRING)),
        ]);
        let bytes = serialize(&v, "");
        let back = parse(&bytes).unwrap();
        assert_eq!(back, v);
        if let Value::Compound(f) = &back {
            assert_eq!(f[0].1, Value::List(Vec::new(), TAG_INT));
            assert_eq!(f[1].1, Value::List(Vec::new(), TAG_STRING));
        } else {
            panic!("根应为 Compound");
        }
    }

    #[test]
    fn roundtrip_keeps_content() {
        let v = sample();
        let bytes = serialize(&v, "");
        let back = parse(&bytes).unwrap();
        assert_eq!(v, back);
    }

    #[test]
    fn field_order_is_preserved() {
        let bytes = serialize(&sample(), "");
        let back = parse(&bytes).unwrap();
        if let Value::Compound(f) = &back {
            let keys: Vec<&str> = f.iter().map(|(k, _)| k.as_str()).collect();
            assert_eq!(keys, ["name", "count", "flag", "ratio", "list", "nested", "longs"]);
        } else {
            panic!("根应为 Compound");
        }
    }

    #[test]
    fn gzip_roundtrip() {
        let v = sample();
        let z = serialize_gzip(&v, "", 6).unwrap();
        assert_eq!(&z[0..2], &[0x1f, 0x8b]);
        assert_eq!(parse_maybe_gzip(&z).unwrap(), v);
    }

    #[test]
    fn uncompressed_input_parses() {
        let v = sample();
        let raw = serialize(&v, "");
        assert_eq!(parse_maybe_gzip(&raw).unwrap(), v);
    }

    #[test]
    fn xyz_accessor_reads_triplet() {
        let v = Value::Compound(vec![
            ("x".into(), Value::Int(1)),
            ("y".into(), Value::Int(-2)),
            ("z".into(), Value::Int(3)),
        ]);
        assert_eq!(v.as_xyz(), Some([1, -2, 3]));
    }

    #[test]
    fn truncated_data_errors_without_panic() {
        let bytes = serialize(&sample(), "");
        for cut in [1usize, 3, 10, 25] {
            let _ = parse(&bytes[..cut.min(bytes.len())]);
        }
    }

    #[test]
    fn set_replaces_in_place() {
        let mut v = Value::Compound(vec![
            ("a".into(), Value::Int(1)),
            ("b".into(), Value::Int(2)),
        ]);
        v.set("a", Value::Int(9));
        v.set("c", Value::Int(3));
        if let Value::Compound(f) = &v {
            let keys: Vec<&str> = f.iter().map(|(k, _)| k.as_str()).collect();
            assert_eq!(keys, ["a", "b", "c"]);
            assert_eq!(f[0].1, Value::Int(9));
        }
    }
}
