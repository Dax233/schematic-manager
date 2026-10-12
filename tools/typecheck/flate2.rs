//! flate2 的最小桩实现。
//! 用途：本机无法访问 crates.io，而类型检查不需要真正的压缩实现。
//! 只提供被检查代码实际用到的 API 表面。
use std::io::{Read, Result, Write};

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
    pub struct GzDecoder<R>(pub R);
    impl<R: Read> GzDecoder<R> {
        pub fn new(r: R) -> Self { GzDecoder(r) }
    }
    impl<R: Read> Read for GzDecoder<R> {
        fn read(&mut self, _buf: &mut [u8]) -> Result<usize> { Ok(0) }
    }
    pub struct ZlibDecoder<R>(pub R);
    impl<R: Read> ZlibDecoder<R> {
        pub fn new(r: R) -> Self { ZlibDecoder(r) }
    }
    impl<R: Read> Read for ZlibDecoder<R> {
        fn read(&mut self, _buf: &mut [u8]) -> Result<usize> { Ok(0) }
    }
    /// raw deflate：zip 条目的压缩形式（无 zlib 头与校验）
    pub struct DeflateDecoder<R>(pub R);
    impl<R: Read> DeflateDecoder<R> {
        pub fn new(r: R) -> Self { DeflateDecoder(r) }
    }
    impl<R: Read> Read for DeflateDecoder<R> {
        fn read(&mut self, _buf: &mut [u8]) -> Result<usize> { Ok(0) }
    }
}

pub mod write {
    use super::*;
    pub struct GzEncoder<W>(pub W);
    impl<W: Write> GzEncoder<W> {
        pub fn new(w: W, _c: Compression) -> Self { GzEncoder(w) }
        pub fn finish(self) -> Result<W> { Ok(self.0) }
    }
    impl<W: Write> Write for GzEncoder<W> {
        fn write(&mut self, buf: &[u8]) -> Result<usize> { Ok(buf.len()) }
        fn flush(&mut self) -> Result<()> { Ok(()) }
    }
    /// raw deflate 编码器：zip 条目写测试数据时使用
    pub struct DeflateEncoder<W>(pub W);
    impl<W: Write> DeflateEncoder<W> {
        pub fn new(w: W, _c: Compression) -> Self { DeflateEncoder(w) }
        pub fn finish(self) -> Result<W> { Ok(self.0) }
    }
    impl<W: Write> Write for DeflateEncoder<W> {
        fn write(&mut self, buf: &[u8]) -> Result<usize> { Ok(buf.len()) }
        fn flush(&mut self) -> Result<()> { Ok(()) }
    }
    pub struct ZlibEncoder<W>(pub W);
    impl<W: Write> ZlibEncoder<W> {
        pub fn new(w: W, _c: Compression) -> Self { ZlibEncoder(w) }
        pub fn finish(self) -> Result<W> { Ok(self.0) }
    }
    impl<W: Write> Write for ZlibEncoder<W> {
        fn write(&mut self, buf: &[u8]) -> Result<usize> { Ok(buf.len()) }
        fn flush(&mut self) -> Result<()> { Ok(()) }
    }
}
