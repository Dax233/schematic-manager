//! zip 读取（只读）。
//!
//! 用途：Minecraft 客户端与模组 jar 都是 zip，导入资源包时需要列出条目、
//! 按名提取并解压。本机无法获取 zip 类依赖，故自行实现。
//!
//! 只实现读取所需的部分：从中央目录索引条目，按需解压单个条目。
//! 不做写入、加密、zip64（jar 不会用到；遇到时明确报错而非给出错数据）。
//!
//! 分层：中央目录解析是纯字节逻辑，`parse_central_directory` 可脱离
//! 解压单独测试；解压只出现在 `read` 内。

use crate::error::{AppError, AppResult};
use std::io::Read;

/// 中央目录结尾的签名
const EOCD_SIG: u32 = 0x0605_4b50;
const EOCD_MIN_LEN: usize = 22;
/// 中央目录条目的签名
const CEN_SIG: u32 = 0x0201_4b50;
/// 本地文件头的签名
const LOC_SIG: u32 = 0x0403_4b50;

/// 压缩方式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// 不压缩
    Stored,
    /// raw deflate（zip 条目的常见形式）
    Deflate,
}

/// 一个条目在 zip 中的位置与属性
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// 条目名（路径分隔符为 `/`）
    pub name: String,
    pub method: Method,
    /// 压缩后大小
    pub compressed_size: u64,
    /// 解压后大小
    pub uncompressed_size: u64,
    /// 本地文件头在文件中的偏移
    pub local_offset: u64,
    pub crc32: u32,
}

impl Entry {
    pub fn is_dir(&self) -> bool {
        self.name.ends_with('/')
    }
}

/// 整个 zip 的索引
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Archive {
    pub entries: Vec<Entry>,
}

impl Archive {
    /// 按名精确查找
    pub fn find(&self, name: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.name == name)
    }

    /// 按前缀列出条目名（Minecraft 资源常按目录组织）
    pub fn list_prefix<'a>(&'a self, prefix: &'a str) -> Vec<&'a Entry> {
        self.entries.iter().filter(|e| e.name.starts_with(prefix)).collect()
    }

    /// 全部条目名
    pub fn names(&self) -> Vec<&str> {
        self.entries.iter().map(|e| e.name.as_str()).collect()
    }
}

fn u16le(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([b[off], b[off + 1]])
}

fn u32le(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

/// 找中央目录结尾记录（EOCD）。
///
/// 从尾部向前搜索：EOCD 之后可能还有注释，长度不定，因此不能只看最后 22 字节。
fn find_eocd(data: &[u8]) -> AppResult<usize> {
    if data.len() < EOCD_MIN_LEN {
        return Err(AppError::Corrupt("不是 zip：文件过短".into()));
    }
    // 注释最长 65535 字节，因此 EOCD 起点不早于 len - 22 - 65535
    let lower = data.len().saturating_sub(EOCD_MIN_LEN + 65535);
    let mut i = data.len() - EOCD_MIN_LEN;
    loop {
        if u32le(data, i) == EOCD_SIG {
            return Ok(i);
        }
        if i == lower {
            return Err(AppError::Corrupt("不是 zip：找不到中央目录结尾记录".into()));
        }
        i -= 1;
    }
}

/// 解析中央目录，得到条目索引。
///
/// 只读中央目录而不读本地文件头：本地头中的大小字段在「使用数据描述符」
/// 时可能为 0，而中央目录里的值总是可靠的。
pub fn parse_central_directory(data: &[u8]) -> AppResult<Archive> {
    let eocd = find_eocd(data)?;
    let total = u16le(data, eocd + 10) as usize;
    let cd_size = u32le(data, eocd + 12) as usize;
    let cd_off = u32le(data, eocd + 16) as usize;

    if cd_off == 0xFFFF_FFFF || cd_size == 0xFFFF_FFFF || total == 0xFFFF {
        return Err(AppError::UnsupportedVersion(
            "暂不支持 zip64 格式".into(),
        ));
    }
    if cd_off + cd_size > data.len() {
        return Err(AppError::Corrupt("zip 中央目录超出文件范围".into()));
    }

    let mut entries = Vec::with_capacity(total.min(65536));
    let mut p = cd_off;
    let end = cd_off + cd_size;
    for _ in 0..total {
        if p + 46 > end {
            return Err(AppError::Corrupt("zip 中央目录条目不完整".into()));
        }
        if u32le(data, p) != CEN_SIG {
            return Err(AppError::Corrupt(format!(
                "zip 中央目录条目签名不符（偏移 {p}）"
            )));
        }
        let method = match u16le(data, p + 10) {
            0 => Method::Stored,
            8 => Method::Deflate,
            other => {
                return Err(AppError::UnsupportedVersion(format!(
                    "暂不支持 zip 压缩方式 {other}"
                )))
            }
        };
        let crc32 = u32le(data, p + 16);
        let compressed_size = u32le(data, p + 20) as u64;
        let uncompressed_size = u32le(data, p + 24) as u64;
        let name_len = u16le(data, p + 28) as usize;
        let extra_len = u16le(data, p + 30) as usize;
        let comment_len = u16le(data, p + 32) as usize;
        let local_offset = u32le(data, p + 42) as u64;

        if p + 46 + name_len > end {
            return Err(AppError::Corrupt("zip 条目名超出中央目录范围".into()));
        }
        let raw_name = &data[p + 46..p + 46 + name_len];
        // 文件名按 UTF-8 解码；jar 内的条目名都是 ASCII，坏字节按替换字符处理
        let name = String::from_utf8_lossy(raw_name).into_owned();

        entries.push(Entry {
            name,
            method,
            compressed_size,
            uncompressed_size,
            local_offset,
            crc32,
        });

        p += 46 + name_len + extra_len + comment_len;
    }

    Ok(Archive { entries })
}

/// 从字节解析 zip 索引
pub fn open(data: &[u8]) -> AppResult<Archive> {
    parse_central_directory(data)
}

/// CRC-32（IEEE 802.3，与 zip 使用的一致）
pub fn crc32(data: &[u8]) -> u32 {
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

/// 取出某个条目的原始内容。
///
/// 先读本地文件头以取得数据区的真实起点：本地头中的名称与扩展字段长度
/// 可能与中央目录不同，用中央目录的长度去推算数据位置会错位。
pub fn read(data: &[u8], e: &Entry) -> AppResult<Vec<u8>> {
    let off = e.local_offset as usize;
    if off + 30 > data.len() {
        return Err(AppError::Corrupt(format!("条目 {} 的本地头超出范围", e.name)));
    }
    if u32le(data, off) != LOC_SIG {
        return Err(AppError::Corrupt(format!("条目 {} 的本地头签名不符", e.name)));
    }
    let name_len = u16le(data, off + 26) as usize;
    let extra_len = u16le(data, off + 28) as usize;
    let ds = off + 30 + name_len + extra_len;
    let de = ds
        .checked_add(e.compressed_size as usize)
        .ok_or_else(|| AppError::Corrupt("条目大小溢出".into()))?;
    if de > data.len() {
        return Err(AppError::Corrupt(format!("条目 {} 的数据超出范围", e.name)));
    }
    let raw = &data[ds..de];

    let out = match e.method {
        Method::Stored => raw.to_vec(),
        Method::Deflate => {
            let mut d = flate2::read::DeflateDecoder::new(raw);
            let mut out = Vec::with_capacity(e.uncompressed_size.min(64 * 1024 * 1024) as usize);
            d.read_to_end(&mut out)?;
            out
        }
    };

    if out.len() as u64 != e.uncompressed_size {
        return Err(AppError::Corrupt(format!(
            "条目 {} 解压后长度 {} 与记录的 {} 不符",
            e.name,
            out.len(),
            e.uncompressed_size
        )));
    }
    // 校验 CRC：中央目录里的值是可靠的，能据此发现数据损坏
    let got = crc32(&out);
    if got != e.crc32 {
        return Err(AppError::Corrupt(format!(
            "条目 {} 校验和不符（记录 {:08X}，实际 {:08X}）",
            e.name, e.crc32, got
        )));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一个只含中央目录的最小 zip，便于精确控制字段
    struct ZipBuilder {
        data: Vec<u8>,
        entries: Vec<(String, u32, u32, u32, u64, u16)>,
    }

    impl ZipBuilder {
        fn new() -> Self {
            Self { data: Vec::new(), entries: Vec::new() }
        }

        /// 直接塞入原始字节的条目（data 为已压缩内容）
        fn raw_entry(
            &mut self,
            name: &str,
            method: u16,
            raw: &[u8],
            uncompressed_size: u32,
            crc: u32,
        ) {
            let local_offset = self.data.len() as u64;
            // 本地文件头
            self.data.extend_from_slice(&LOC_SIG.to_le_bytes());
            self.data.extend_from_slice(&20u16.to_le_bytes()); // version
            self.data.extend_from_slice(&0u16.to_le_bytes()); // flags
            self.data.extend_from_slice(&method.to_le_bytes());
            self.data.extend_from_slice(&0u16.to_le_bytes()); // time
            self.data.extend_from_slice(&0u16.to_le_bytes()); // date
            self.data.extend_from_slice(&crc.to_le_bytes());
            self.data.extend_from_slice(&(raw.len() as u32).to_le_bytes());
            self.data.extend_from_slice(&uncompressed_size.to_le_bytes());
            self.data.extend_from_slice(&(name.len() as u16).to_le_bytes());
            self.data.extend_from_slice(&0u16.to_le_bytes()); // extra
            self.data.extend_from_slice(name.as_bytes());
            self.data.extend_from_slice(raw);
            self.entries.push((
                name.to_string(),
                crc,
                raw.len() as u32,
                uncompressed_size,
                local_offset,
                method,
            ));
        }

        /// 加入一个不压缩的条目
        fn stored(&mut self, name: &str, content: &[u8]) {
            self.raw_entry(name, 0, content, content.len() as u32, crc32(content));
        }

        /// 加入一个 deflate 压缩的条目（真实 jar 里的常见形式）
        fn deflated(&mut self, name: &str, content: &[u8]) {
            let raw = deflate(content);
            self.raw_entry(name, 8, &raw, content.len() as u32, crc32(content));
        }

        fn finish(mut self) -> Vec<u8> {
            let cd_off = self.data.len() as u32;
            for (name, crc, comp, uncomp, off, method) in &self.entries {
                self.data.extend_from_slice(&CEN_SIG.to_le_bytes());
                self.data.extend_from_slice(&20u16.to_le_bytes()); // made by
                self.data.extend_from_slice(&20u16.to_le_bytes()); // needed
                self.data.extend_from_slice(&0u16.to_le_bytes()); // flags
                self.data.extend_from_slice(&method.to_le_bytes());
                self.data.extend_from_slice(&0u16.to_le_bytes());
                self.data.extend_from_slice(&0u16.to_le_bytes());
                self.data.extend_from_slice(&crc.to_le_bytes());
                self.data.extend_from_slice(&comp.to_le_bytes());
                self.data.extend_from_slice(&uncomp.to_le_bytes());
                self.data.extend_from_slice(&(name.len() as u16).to_le_bytes());
                self.data.extend_from_slice(&0u16.to_le_bytes()); // extra
                self.data.extend_from_slice(&0u16.to_le_bytes()); // comment
                self.data.extend_from_slice(&0u16.to_le_bytes()); // disk
                self.data.extend_from_slice(&0u16.to_le_bytes()); // internal attr
                self.data.extend_from_slice(&0u32.to_le_bytes()); // external attr
                self.data.extend_from_slice(&(*off as u32).to_le_bytes());
                self.data.extend_from_slice(name.as_bytes());
            }
            let cd_size = self.data.len() as u32 - cd_off;
            let total = self.entries.len() as u16;
            self.data.extend_from_slice(&EOCD_SIG.to_le_bytes());
            self.data.extend_from_slice(&0u16.to_le_bytes()); // disk
            self.data.extend_from_slice(&0u16.to_le_bytes()); // cd disk
            self.data.extend_from_slice(&total.to_le_bytes());
            self.data.extend_from_slice(&total.to_le_bytes());
            self.data.extend_from_slice(&cd_size.to_le_bytes());
            self.data.extend_from_slice(&cd_off.to_le_bytes());
            self.data.extend_from_slice(&0u16.to_le_bytes()); // comment len
            self.data
        }
    }

    /// 用 raw deflate 压缩（与 zip 条目的形式一致）
    fn deflate(src: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(src).unwrap();
        e.finish().unwrap()
    }

    /// Deflate 条目的读取路径必须真的被覆盖。
    ///
    /// 此前所有测试用的都是 stored（method 0）条目，deflate 分支
    /// （zip.rs 的 Method::Deflate 解压）从未被执行过——而真实 jar 里的
    /// 条目几乎全是 deflate，这是导入资源包时最主要的一条路径。
    #[test]
    fn reads_deflate_compressed_entry() {
        let mut b = ZipBuilder::new();
        // 内容取长一些并含重复片段，确保压缩后与原文长度不同，
        // 从而能区分「解压成功」与「原样返回压缩字节」
        let content = b"minecraft:block/stone\n".repeat(200);
        b.deflated("assets/stone.json", &content);
        let zip = b.finish();

        let a = open(&zip).expect("解析失败");
        let e = a.find("assets/stone.json").expect("应找到该条目");
        assert_eq!(e.method, Method::Deflate, "该条目应是 deflate 方式");
        assert_eq!(e.uncompressed_size as usize, content.len());

        let got = read(&zip, e).expect("应能解压读取");
        assert_eq!(got, content, "解压结果必须与原文一致");
    }

    /// 声明的原始长度与实际不符时必须报错，而不是给出截断/多余的内容。
    #[test]
    fn deflate_length_mismatch_is_rejected() {
        let content = b"hello deflate world";
        let raw = deflate(content);
        let mut b = ZipBuilder::new();
        // 故意把记录的原始长度写错
        b.raw_entry("bad.txt", 8, &raw, content.len() as u32 + 5, crc32(content));
        let zip = b.finish();

        let a = open(&zip).expect("解析失败");
        let e = a.find("bad.txt").expect("应找到该条目");
        let err = read(&zip, e).expect_err("长度不符应报错");
        assert!(
            err.to_string().contains("不符"),
            "错误信息应说明长度不符，实际：{err}"
        );
    }

    /// CRC 不符时必须报错：数据在传输或存储中损坏了。
    #[test]
    fn deflate_crc_mismatch_is_rejected() {
        let content = b"hello deflate world";
        let raw = deflate(content);
        let mut b = ZipBuilder::new();
        b.raw_entry("bad.txt", 8, &raw, content.len() as u32, crc32(content) ^ 0xFF);
        let zip = b.finish();

        let a = open(&zip).expect("解析失败");
        // 先确认条目本身能被找到，避免把「找不到」误当成「CRC 校验生效」
        assert!(a.find("bad.txt").is_some());
        let e = a.find("bad.txt").expect("应找到该条目");
        let err = read(&zip, e).expect_err("CRC 不符应报错");
        assert!(
            err.to_string().contains("CRC") || err.to_string().contains("校验"),
            "错误信息应说明校验失败，实际：{err}"
        );
    }

    #[test]
    fn lists_entries_from_central_directory() {
        let mut b = ZipBuilder::new();
        b.stored("assets/foo.txt", b"hello");
        b.stored("pack.mcmeta", b"{}");
        let zip = b.finish();

        let a = open(&zip).expect("解析失败");
        assert_eq!(a.entries.len(), 2);
        assert_eq!(a.names(), vec!["assets/foo.txt", "pack.mcmeta"]);
        assert_eq!(a.find("pack.mcmeta").unwrap().method, Method::Stored);
    }

    #[test]
    fn stored_entry_reads_back_and_verifies_crc() {
        let mut b = ZipBuilder::new();
        b.stored("a.txt", b"content here");
        let zip = b.finish();
        let a = open(&zip).unwrap();
        let e = a.find("a.txt").unwrap();
        assert_eq!(read(&zip, e).unwrap(), b"content here");
    }

    #[test]
    fn directory_entries_are_recognized() {
        let mut b = ZipBuilder::new();
        b.stored("assets/", b"");
        b.stored("assets/a.txt", b"x");
        let zip = b.finish();
        let a = open(&zip).unwrap();
        assert!(a.find("assets/").unwrap().is_dir());
        assert!(!a.find("assets/a.txt").unwrap().is_dir());
        assert_eq!(a.list_prefix("assets/").len(), 2);
    }

    #[test]
    fn find_returns_none_for_missing_entry() {
        let mut b = ZipBuilder::new();
        b.stored("a.txt", b"x");
        let zip = b.finish();
        let a = open(&zip).unwrap();
        assert!(a.find("nope.txt").is_none());
        assert!(a.list_prefix("nope/").is_empty());
    }

    #[test]
    fn rejects_non_zip_data() {
        let e = open(b"this is definitely not a zip file").unwrap_err().to_string();
        assert!(e.contains("中央目录"), "应指出找不到中央目录：{e}");
    }

    #[test]
    fn rejects_truncated_file() {
        let e = open(b"PK").unwrap_err().to_string();
        assert!(e.contains("过短"), "应指出文件过短：{e}");
    }

    #[test]
    fn eocd_is_found_despite_trailing_comment() {
        let mut b = ZipBuilder::new();
        b.stored("a.txt", b"data");
        let mut zip = b.finish();
        // 手工追加大段注释：EOCD 的注释长度字段保持 0 也能被找到，
        // 但真正的 zip 会写该字段；这里验证的是向后搜索的鲁棒性
        zip.extend_from_slice(&[0x41u8; 300]);
        let a = open(&zip).expect("应在尾部搜索中找到 EOCD");
        assert_eq!(a.entries.len(), 1);
    }

    #[test]
    fn crc_mismatch_is_reported() {
        let mut b = ZipBuilder::new();
        // 故意写入错误的 CRC
        b.raw_entry("bad.txt", 0, b"content", 7, 0xDEAD_BEEF);
        let zip = b.finish();
        let a = open(&zip).unwrap();
        let e = a.find("bad.txt").unwrap();
        let msg = read(&zip, e).unwrap_err().to_string();
        assert!(msg.contains("校验和"), "应指出校验和不符：{msg}");
    }

    #[test]
    fn size_mismatch_is_reported() {
        let mut b = ZipBuilder::new();
        // 记录的解压后大小与实际不符
        b.raw_entry("bad.txt", 0, b"abc", 999, crc32(b"abc"));
        let zip = b.finish();
        let a = open(&zip).unwrap();
        let e = a.find("bad.txt").unwrap();
        let msg = read(&zip, e).unwrap_err().to_string();
        assert!(msg.contains("长度"), "应指出长度不符：{msg}");
    }

    #[test]
    fn unsupported_compression_method_is_rejected() {
        let mut b = ZipBuilder::new();
        b.raw_entry("x.bin", 12, b"xxxx", 4, 0); // 12 = bzip2
        let zip = b.finish();
        let e = open(&zip).unwrap_err().to_string();
        assert!(e.contains("压缩方式"), "应指出压缩方式不支持：{e}");
    }

    #[test]
    fn crc32_matches_known_vector() {
        // 标准测试向量：ASCII "123456789" 的 CRC-32 为 0xCBF43926
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }
}
