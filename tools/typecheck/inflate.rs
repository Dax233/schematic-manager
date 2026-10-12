//! 一个最小但正确的 DEFLATE/zlib 解压实现，供本地类型检查环境使用。
//!
//! 为什么需要它：产品代码依赖真实的 flate2（CI 可取到），而本地无法访问
//! crates.io。原先的桩让解码器一律返回 0 字节，导致 PNG 解码与 zip 条目
//! 解压在本地**必然失败**——于是「从 jar 构建资源包」这条链路的核心步骤
//! 无法在本地验证，只能整体推给 CI。
//!
//! 这里实现 RFC1950（zlib）与 RFC1951（deflate）的解码子集：
//!   - 仅解码（stored / 固定霍夫曼 / 动态霍夫曼三种块类型）
//!   - 支持 zlib 与 raw deflate 两种容器
//!   - 不做校验和验证以外的事；zlib 的 Adler-32 会核对
//!
//! 这是**测试设施**，不进入产品：产品用真实 flate2。

use std::io::{Read, Result};

/// 位读取器（DEFLATE 按位从低位开始读）
struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    bit: u32,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0, bit: 0 }
    }
    fn bit(&mut self) -> Result<u32> {
        if self.pos >= self.data.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "deflate 数据提前结束",
            ));
        }
        let b = (self.data[self.pos] >> self.bit) & 1;
        self.bit += 1;
        if self.bit == 8 {
            self.bit = 0;
            self.pos += 1;
        }
        Ok(b as u32)
    }
    fn bits(&mut self, n: u32) -> Result<u32> {
        let mut v = 0u32;
        for i in 0..n {
            v |= self.bit()? << i;
        }
        Ok(v)
    }
    /// 跳到字节边界
    fn align(&mut self) {
        if self.bit != 0 {
            self.bit = 0;
            self.pos += 1;
        }
    }
    fn take_bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        self.align();
        if self.pos + n > self.data.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "deflate stored 数据不足",
            ));
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
}

/// 霍夫曼解码表。
///
/// 用规范式（canonical）码构建：DEFLATE 的码是从低位开始读的，
/// 因此建表时把码反序后按码长分组；解码时逐位累加并与同码长的桶比对。
///
/// 这里不用「预生成查找表」是为了让实现尽量直白——本文件只在本地
/// 检查环境使用，正确性远比速度重要。
struct Huffman {
    /// buckets[len] = 该码长下 (反序码, 符号) 的列表
    buckets: Vec<Vec<(u32, u32)>>,
}

impl Huffman {
    /// 由码长表构建
    fn from_lengths(lengths: &[u32]) -> Result<Self> {
        let max = lengths.iter().copied().max().unwrap_or(0) as usize;
        let mut buckets: Vec<Vec<(u32, u32)>> = vec![Vec::new(); max + 1];
        if max == 0 {
            return Ok(Self { buckets });
        }

        // 统计各码长的数量
        let mut bl_count = vec![0u32; max + 1];
        for &l in lengths {
            if l > 0 {
                bl_count[l as usize] += 1;
            }
        }
        // 计算每个码长的第一个码（规范式）
        let mut next_code = vec![0u32; max + 1];
        let mut code = 0u32;
        for bits in 1..=max {
            code = (code + bl_count[bits - 1]) << 1;
            next_code[bits] = code;
        }
        // 按符号顺序分配码，并按「反序码」入桶
        for (sym, &l) in lengths.iter().enumerate() {
            if l > 0 {
                let c = next_code[l as usize];
                next_code[l as usize] += 1;
                buckets[l as usize].push((reverse_bits(c, l), sym as u32));
            }
        }
        Ok(Self { buckets })
    }

    /// 解出一个符号
    fn decode(&self, r: &mut BitReader<'_>) -> Result<u32> {
        let mut code = 0u32;
        for len in 1..self.buckets.len() {
            code |= r.bit()? << (len - 1);
            for (c, sym) in &self.buckets[len] {
                if *c == code {
                    return Ok(*sym);
                }
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "deflate 霍夫曼码无效",
        ))
    }
}

fn bit_len(v: u32) -> u32 {
    if v == 0 {
        0
    } else {
        32 - v.leading_zeros()
    }
}

/// 位反序：DEFLATE 的码按低位先读，因此建表时需反序
fn reverse_bits(v: u32, n: u32) -> u32 {
    let mut out = 0u32;
    for i in 0..n {
        out |= ((v >> i) & 1) << (n - 1 - i);
    }
    out
}

const LEN_BASE: [u32; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LEN_EXTRA: [u32; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u32; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u32; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
const CLEN_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

fn fixed_tables() -> (Huffman, Huffman) {
    // 固定霍夫曼：字面量 0..143 为 8 位、144..255 为 9 位、256..279 为 7 位、280..287 为 8 位
    let mut lit_len = vec![0u32; 288];
    for i in 0..144 {
        lit_len[i] = 8;
    }
    for i in 144..256 {
        lit_len[i] = 9;
    }
    for i in 256..280 {
        lit_len[i] = 7;
    }
    for i in 280..288 {
        lit_len[i] = 8;
    }
    let dist_len = vec![5u32; 30];
    (
        Huffman::from_lengths(&lit_len).expect("固定表"),
        Huffman::from_lengths(&dist_len).expect("固定表"),
    )
}

/// 解压 deflate 数据
fn inflate(data: &[u8]) -> Result<Vec<u8>> {
    let mut r = BitReader::new(data);
    let mut out: Vec<u8> = Vec::new();
    loop {
        let last = r.bit()?;
        let btype = r.bits(2)?;
        match btype {
            0 => {
                // 非压缩块：BFINAL 与 BTYPE 之后必须跳到字节边界，
                // 否则读到的 LEN/NLEN 会错位（Deflate 规范明确要求）
                r.align();
                let len = r.bits(16)? as usize;
                let nlen = r.bits(16)? as usize;
                if len != (!nlen & 0xFFFF) {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "deflate stored 块长度校验失败",
                    ));
                }
                let bytes = r.take_bytes(len)?;
                out.extend_from_slice(bytes);
            }
            1 | 2 => {
                // btype 1 用固定表，btype 2 从流里读动态表
                let (lit, dist) = if btype == 1 {
                    fixed_tables()
                } else {
                    read_dynamic_tables(&mut r)?
                };
                decode_block(&mut r, &lit, &dist, &mut out)?;
            }
            3 => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "deflate 块类型 3 无效",
                ))
            }
            _ => unreachable!(),
        }
        if last == 1 {
            break;
        }
    }
    Ok(out)
}

fn read_dynamic_tables(r: &mut BitReader<'_>) -> Result<(Huffman, Huffman)> {
    let hlit = r.bits(5)? as usize + 257;
    let hdist = r.bits(5)? as usize + 1;
    let hclen = r.bits(4)? as usize + 4;

    let mut clen = vec![0u32; 19];
    for i in 0..hclen {
        clen[CLEN_ORDER[i]] = r.bits(3)?;
    }
    let cl = Huffman::from_lengths(&clen)?;

    // 用码长表展开出字面量与距离的码长
    let mut lengths: Vec<u32> = Vec::with_capacity(hlit + hdist);
    while lengths.len() < hlit + hdist {
        let sym = cl.decode(r)?;
        match sym {
            0..=15 => lengths.push(sym),
            16 => {
                if lengths.is_empty() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "deflate 码长重复但无前值",
                    ));
                }
                let prev = *lengths.last().unwrap();
                let n = 3 + r.bits(2)?;
                for _ in 0..n {
                    lengths.push(prev);
                }
            }
            17 => {
                let n = 3 + r.bits(3)?;
                for _ in 0..n {
                    lengths.push(0);
                }
            }
            18 => {
                let n = 11 + r.bits(7)?;
                for _ in 0..n {
                    lengths.push(0);
                }
            }
            _ => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "deflate 码长符号无效",
                ))
            }
        }
    }
    lengths.truncate(hlit + hdist);
    let lit = Huffman::from_lengths(&lengths[..hlit])?;
    let dist = Huffman::from_lengths(&lengths[hlit..])?;
    Ok((lit, dist))
}

fn decode_block(
    r: &mut BitReader<'_>,
    lit: &Huffman,
    dist: &Huffman,
    out: &mut Vec<u8>,
) -> Result<()> {
    loop {
        let sym = lit.decode(r)?;
        match sym {
            0..=255 => out.push(sym as u8),
            256 => return Ok(()),
            257..=285 => {
                let i = (sym - 257) as usize;
                let len = LEN_BASE[i] + r.bits(LEN_EXTRA[i])?;
                let dsym = dist.decode(r)? as usize;
                if dsym >= DIST_BASE.len() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "deflate 距离符号越界",
                    ));
                }
                let d = DIST_BASE[dsym] + r.bits(DIST_EXTRA[dsym])?;
                if d as usize > out.len() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "deflate 距离超出已输出范围",
                    ));
                }
                let start = out.len() - d as usize;
                for k in 0..len as usize {
                    let b = out[start + k];
                    out.push(b);
                }
            }
            _ => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "deflate 字面量符号越界",
                ))
            }
        }
    }
}

/// zlib 容器解压（含 2 字节头与 4 字节 Adler-32）
pub fn zlib_decompress(data: &[u8]) -> Result<Vec<u8>> {
    if data.len() < 6 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "zlib 数据过短",
        ));
    }
    let cmf = data[0];
    let flg = data[1];
    if cmf & 0x0F != 8 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "zlib 压缩方法不支持",
        ));
    }
    if (cmf as u16 * 256 + flg as u16) % 31 != 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "zlib 头校验失败",
        ));
    }
    let out = inflate(&data[2..data.len() - 4])?;
    // 核对 Adler-32
    let expect = u32::from_be_bytes([
        data[data.len() - 4],
        data[data.len() - 3],
        data[data.len() - 2],
        data[data.len() - 1],
    ]);
    if adler32(&out) != expect {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "zlib Adler-32 校验失败",
        ));
    }
    Ok(out)
}

/// raw deflate 解压（zip 条目使用）
pub fn deflate_decompress(data: &[u8]) -> Result<Vec<u8>> {
    inflate(data)
}

/// gzip 容器解压（.litematic 等使用）
///
/// 结构：10 字节头 + 可选字段（FEXTRA/FNAME/FCOMMENT/FHCRC）+ deflate 流
/// + CRC32(4) + ISIZE(4)。可选字段不跳过会让后续 deflate 从错误的偏移开始，
/// 表现为「解压失败」而不是「数据损坏」，因此必须按 FLG 逐项处理。
pub fn gzip_decompress(data: &[u8]) -> Result<Vec<u8>> {
    if data.len() < 18 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "gzip 数据过短",
        ));
    }
    if data[0] != 0x1F || data[1] != 0x8B {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "gzip 魔数不符",
        ));
    }
    if data[2] != 8 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "gzip 压缩方法不支持",
        ));
    }
    let flg = data[3];
    let mut pos = 10usize;
    if flg & 0x04 != 0 {
        // FEXTRA：2 字节长度 + 内容
        if pos + 2 > data.len() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "gzip FEXTRA 截断"));
        }
        let n = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
        pos += 2 + n;
    }
    // FNAME / FCOMMENT：以 0 结尾的字符串
    for bit in [0x08u8, 0x10] {
        if flg & bit != 0 {
            while pos < data.len() && data[pos] != 0 {
                pos += 1;
            }
            pos += 1; // 跳过结束的 0
        }
    }
    if flg & 0x02 != 0 {
        pos += 2; // FHCRC
    }
    if pos + 8 > data.len() {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "gzip 头越界"));
    }
    let body = &data[pos..data.len() - 8];
    let out = inflate(body)?;
    let expect = u32::from_le_bytes([
        data[data.len() - 8],
        data[data.len() - 7],
        data[data.len() - 6],
        data[data.len() - 5],
    ]);
    if crc32_of(&out) != expect {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "gzip CRC32 校验失败",
        ));
    }
    Ok(out)
}

/// CRC-32（IEEE，与 zip/gzip 一致）
pub fn crc32_of(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, slot) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *slot = c;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for b in data {
        crc = table[((crc ^ *b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

pub fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for byte in data {
        a = (a + *byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// 有状态的解码读取器。
///
/// 必须跨 `read` 调用保持状态：调用方（read_to_end / PNG 的按块读取）
/// 会连续多次调用。若无状态地每次重新解码，第二次调用时底层输入已耗尽，
/// 会得到「数据过短」这类与真实原因无关的错误。
///
/// 解码在首次 read 时一次完成并缓存；后续按偏移供给。
pub struct OnceReader<R> {
    inner: Option<R>,
    decoded: Option<Vec<u8>>,
    pos: usize,
    decode: fn(&[u8]) -> Result<Vec<u8>>,
}

impl<R: Read> OnceReader<R> {
    pub fn new(inner: R, decode: fn(&[u8]) -> Result<Vec<u8>>) -> Self {
        Self { inner: Some(inner), decoded: None, pos: 0, decode }
    }
}

impl<R: Read> Read for OnceReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        if self.decoded.is_none() {
            let mut all = Vec::new();
            let mut inner = self.inner.take().expect("解码器只初始化一次");
            inner.read_to_end(&mut all)?;
            self.decoded = Some((self.decode)(&all)?);
        }
        let out = self.decoded.as_ref().expect("已解码");
        let rest = &out[self.pos..];
        let n = rest.len().min(buf.len());
        buf[..n].copy_from_slice(&rest[..n]);
        self.pos += n;
        Ok(n)
    }
}

/// 解码函数指针类型别名，便于作为函数项传入
pub type DecodeFn = fn(&[u8]) -> Result<Vec<u8>>;

/// 把数据编码成 deflate 的 stored（非压缩）块。
///
/// 用 stored 而不是真压缩：本地没有压缩需求，而**写出合法格式**是必需的——
/// 桩的编码器若只返回长度而不写内容，写出端产出的就是空数据，
/// 会让写入-读取的往返测试全部失败（而这不是产品代码的问题）。
pub fn deflate_store(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    if data.is_empty() {
        // 空输入也要产出一个合法的结束块
        out.push(0x01);
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0xFFFFu16.to_le_bytes());
        return out;
    }
    let mut pos = 0usize;
    while pos < data.len() {
        let chunk = (data.len() - pos).min(65535);
        let last = pos + chunk == data.len();
        // 块头：BFINAL(1 位) + BTYPE=00(2 位)，随后补齐到字节
        out.push(if last { 0x01 } else { 0x00 });
        out.extend_from_slice(&(chunk as u16).to_le_bytes());
        out.extend_from_slice(&(!(chunk as u16)).to_le_bytes());
        out.extend_from_slice(&data[pos..pos + chunk]);
        pos += chunk;
    }
    out
}

/// zlib 容器的 stored 编码（含头与 Adler-32）
pub fn zlib_store(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01]; // CMF/FLG：deflate、无字典、最快
    out.extend_from_slice(&deflate_store(data));
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}
