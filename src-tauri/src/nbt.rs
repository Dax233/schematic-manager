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
    List(Vec<Value>),
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
            Value::List(_) => TAG_LIST,
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
            Value::List(v) => Some(v),
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

    /// NBT 字符串：u16 长度 + UTF-8 字节（Java 的 modified-UTF8 对常见文本与 UTF-8 等价）
    fn string(&mut self) -> AppResult<String> {
        let len = self.u16()? as usize;
        let b = self.take(len)?;
        Ok(String::from_utf8_lossy(b).into_owned())
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
                // 元素类型为 End 时列表必为空（规范如此），但仍按 n 读取以容错
                let mut items = Vec::with_capacity(n.min(4096) as usize);
                for _ in 0..n {
                    items.push(self.payload(elem, depth + 1)?);
                }
                Value::List(items)
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
            let b = s.as_bytes();
            out.extend_from_slice(&(b.len() as u16).to_be_bytes());
            out.extend_from_slice(b);
        }
        Value::List(items) => {
            // 空列表按规范写元素类型 End
            let elem = items.first().map(|v| v.type_id()).unwrap_or(TAG_END);
            out.push(elem);
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
                Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)]),
            ),
            (
                "nested".into(),
                Value::Compound(vec![("x".into(), Value::Int(3)), ("y".into(), Value::Int(4))]),
            ),
            ("longs".into(), Value::LongArray(vec![i64::MIN, -1, 0, 1, i64::MAX])),
        ])
    }

    #[test]
    fn 往返序列化保持一致() {
        let v = sample();
        let bytes = serialize(&v, "");
        let back = parse(&bytes).unwrap();
        assert_eq!(v, back);
    }

    #[test]
    fn 字段顺序被保留() {
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
    fn gzip_往返() {
        let v = sample();
        let z = serialize_gzip(&v, "", 6).unwrap();
        assert_eq!(&z[0..2], &[0x1f, 0x8b]);
        assert_eq!(parse_maybe_gzip(&z).unwrap(), v);
    }

    #[test]
    fn 未压缩数据也能解析() {
        let v = sample();
        let raw = serialize(&v, "");
        assert_eq!(parse_maybe_gzip(&raw).unwrap(), v);
    }

    #[test]
    fn 三元组读取() {
        let v = Value::Compound(vec![
            ("x".into(), Value::Int(1)),
            ("y".into(), Value::Int(-2)),
            ("z".into(), Value::Int(3)),
        ]);
        assert_eq!(v.as_xyz(), Some([1, -2, 3]));
    }

    #[test]
    fn 截断数据报错而非 panic() {
        let bytes = serialize(&sample(), "");
        for cut in [1usize, 3, 10, 25] {
            let _ = parse(&bytes[..cut.min(bytes.len())]);
        }
    }

    #[test]
    fn set_替换保持位置() {
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
