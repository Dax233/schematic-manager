//! flate2 的桩实现：提供被检查代码用到的 API，并**真正实现解压**。
//!
//! 解压不能只做空壳：PNG 解码与 zip 条目解压都依赖它，若一律返回空，
//! 「从 jar 构建资源包」这条链路的核心步骤在本地就无法验证。
//! 因此这里调用同目录 inflate.rs 中的实现（RFC1950/1951 解码子集）。
//! 压缩仍为空壳——本地没有压缩需求。
use std::io::{Read, Result, Write};

#[path = "inflate.rs"]
mod inflate;

/// gzip 用的 CRC-32（与 zip 的实现一致）
pub fn crc32_gz(data: &[u8]) -> u32 {
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

#[derive(Clone, Copy)]
pub struct Compression(u32);
impl Compression {
    pub fn new(level: u32) -> Self { Compression(level) }
}
impl Default for Compression {
    fn default() -> Self { Compression(6) }
}

pub mod read {
    use super::*;
    /// gzip：.litematic 等使用。真正解压，使该链路本地可验证。
    ///
    /// 状态必须**存活在结构体里**：若在每次 read 内部新建解码器，
    /// 第二次调用时长已耗尽，会报出与真实原因无关的错误。
    pub struct GzDecoder<R> {
        inner: super::inflate::OnceReader<R>,
    }
    impl<R: Read> GzDecoder<R> {
        pub fn new(r: R) -> Self {
            Self { inner: super::inflate::OnceReader::new(r, super::inflate::gzip_decompress) }
        }
    }
    impl<R: Read> Read for GzDecoder<R> {
        fn read(&mut self, buf: &mut [u8]) -> Result<usize> { self.inner.read(buf) }
    }

    /// zlib：PNG 的 IDAT 等使用
    pub struct ZlibDecoder<R> {
        inner: super::inflate::OnceReader<R>,
    }
    impl<R: Read> ZlibDecoder<R> {
        pub fn new(r: R) -> Self {
            Self { inner: super::inflate::OnceReader::new(r, super::inflate::zlib_decompress) }
        }
    }
    impl<R: Read> Read for ZlibDecoder<R> {
        fn read(&mut self, buf: &mut [u8]) -> Result<usize> { self.inner.read(buf) }
    }

    /// raw deflate：zip 条目的压缩形式（无 zlib 头与校验）
    pub struct DeflateDecoder<R> {
        inner: super::inflate::OnceReader<R>,
    }
    impl<R: Read> DeflateDecoder<R> {
        pub fn new(r: R) -> Self {
            Self { inner: super::inflate::OnceReader::new(r, super::inflate::deflate_decompress) }
        }
    }
    impl<R: Read> Read for DeflateDecoder<R> {
        fn read(&mut self, buf: &mut [u8]) -> Result<usize> { self.inner.read(buf) }
    }
}

pub mod write {
    use super::*;
    /// 编码器：累积输入，finish 时产出**合法的 stored 块**。
    ///
    /// 必须真正写出内容：若只返回长度，写出端产出的就是空数据，
    /// 写入-读取的往返测试会全部失败（而那不是产品代码的问题）。
    pub struct GzEncoder<W> {
        out: W,
        buf: Vec<u8>,
    }
    impl<W: Write> GzEncoder<W> {
        pub fn new(w: W, _c: Compression) -> Self { Self { out: w, buf: Vec::new() } }
        pub fn finish(self) -> Result<W> {
            // 与真实 flate2 一致：把压缩结果写入底层 writer 并返回它
            let mut w = self.out;
            // gzip 容器：头 + deflate + CRC32 + 长度
            let mut body = vec![0x1f, 0x8b, 0x08, 0x00, 0, 0, 0, 0, 0x00, 0xff];
            body.extend_from_slice(&super::inflate::deflate_store(&self.buf));
            body.extend_from_slice(&crate::crc32_gz(&self.buf).to_le_bytes());
            body.extend_from_slice(&(self.buf.len() as u32).to_le_bytes());
            w.write_all(&body)?;
            Ok(w)
        }
    }
    impl<W: Write> Write for GzEncoder<W> {
        fn write(&mut self, buf: &[u8]) -> Result<usize> {
            self.buf.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> Result<()> { Ok(()) }
    }

    /// raw deflate 编码器：zip 条目与 PNG 测试数据使用
    pub struct DeflateEncoder<W> {
        out: W,
        buf: Vec<u8>,
    }
    impl<W: Write> DeflateEncoder<W> {
        pub fn new(w: W, _c: Compression) -> Self { Self { out: w, buf: Vec::new() } }
        pub fn finish(self) -> Result<W> {
            let mut w = self.out;
            let body = super::inflate::deflate_store(&self.buf);
            w.write_all(&body)?;
            Ok(w)
        }
    }
    impl<W: Write> Write for DeflateEncoder<W> {
        fn write(&mut self, buf: &[u8]) -> Result<usize> {
            self.buf.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> Result<()> { Ok(()) }
    }

    pub struct ZlibEncoder<W> {
        out: W,
        buf: Vec<u8>,
    }
    impl<W: Write> ZlibEncoder<W> {
        pub fn new(w: W, _c: Compression) -> Self { Self { out: w, buf: Vec::new() } }
        pub fn finish(self) -> Result<W> {
            let mut w = self.out;
            let body = super::inflate::zlib_store(&self.buf);
            w.write_all(&body)?;
            Ok(w)
        }
    }
    impl<W: Write> Write for ZlibEncoder<W> {
        fn write(&mut self, buf: &[u8]) -> Result<usize> {
            self.buf.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> Result<()> { Ok(()) }
    }
}
