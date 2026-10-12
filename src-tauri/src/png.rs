//! PNG 解码。
//!
//! 用途：Minecraft 客户端与模组 jar 里的贴图是 PNG，而内部资源包格式存的
//! 是 RGBA8888 原始像素，导入 jar 时必须解码。
//!
//! 本机无法获取 png 类依赖（crates.io 不可达），故自行实现。只解实际会用到的
//! 子集：8 位与 4 位、非隔行。遇到不支持的形式明确报错，而不是给出错色。
//!
//! 分层：`unfilter` 与 `to_rgba` 只处理已解压的扫描线数据，不依赖 zlib，
//! 因此可脱离压缩环节单独测试——这是本文件测试的主要对象。

use crate::error::{AppError, AppResult};

/// 解码后的图像
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    /// RGBA8888，长度 = width * height * 4
    pub rgba: Vec<u8>,
}

/// 像素调色板
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Palette {
    /// 每项 3 字节 RGB
    pub rgb: Vec<[u8; 3]>,
    /// 每项 1 字节 alpha，长度可能与 rgb 相同或为空
    pub alpha: Vec<u8>,
}

/// 色型对应的每像素通道数
fn channels(color_type: u8) -> AppResult<usize> {
    match color_type {
        0 => Ok(1), // 灰度
        2 => Ok(3), // RGB
        3 => Ok(1), // 调色板索引
        4 => Ok(2), // 灰度 + alpha
        6 => Ok(4), // RGBA
        other => Err(AppError::Corrupt(format!("PNG 色型 {other} 不受支持"))),
    }
}

/// 一行扫描线的字节数（不含行首的滤波字节）
fn stride_of(width: u32, color_type: u8, bit_depth: u8) -> AppResult<usize> {
    let ch = channels(color_type)?;
    let bits = (width as usize)
        .checked_mul(ch)
        .and_then(|v| v.checked_mul(bit_depth as usize))
        .ok_or_else(|| AppError::Corrupt("PNG 宽度过大".into()))?;
    Ok((bits + 7) / 8)
}

/// 滤波时使用的「每像素字节数」（PNG 规范规定最小为 1）
fn filter_bpp(color_type: u8, bit_depth: u8) -> AppResult<usize> {
    let ch = channels(color_type)?;
    let bits = ch * bit_depth as usize;
    Ok(((bits + 7) / 8).max(1))
}

/// 还原 PNG 的行滤波。
///
/// `filtered` 为 IDAT 拼接并解压后的数据，每行开头有一个滤波类型字节，
/// 因此每行占 `stride + 1` 字节。四种滤波都要用到左、上、左上三个邻居，
/// 必须逐行顺序还原。
pub fn unfilter(
    filtered: &[u8],
    width: u32,
    height: u32,
    color_type: u8,
    bit_depth: u8,
) -> AppResult<Vec<u8>> {
    let stride = stride_of(width, color_type, bit_depth)?;
    let bpp = filter_bpp(color_type, bit_depth)?;
    let row_len = stride + 1;
    let expect = row_len
        .checked_mul(height as usize)
        .ok_or_else(|| AppError::Corrupt("PNG 尺寸过大".into()))?;
    if filtered.len() != expect {
        return Err(AppError::Corrupt(format!(
            "PNG 像素数据长度 {} 与预期 {expect} 不符（宽 {width} 高 {height}）",
            filtered.len()
        )));
    }

    let mut out = vec![0u8; stride * height as usize];
    for y in 0..height as usize {
        let row = &filtered[y * row_len..(y + 1) * row_len];
        let ft = row[0];
        let src = &row[1..];
        let base = y * stride;
        for x in 0..stride {
            let left = if x >= bpp { out[base + x - bpp] } else { 0 };
            let up = if y > 0 { out[base - stride + x] } else { 0 };
            let up_left = if y > 0 && x >= bpp {
                out[base - stride + x - bpp]
            } else {
                0
            };
            // Paeth 预测器：取与 p 最接近的邻居
            let paeth = |a: u8, b: u8, c: u8| -> u8 {
                let (pa, pb, pc) = (
                    (b as i32 - c as i32).abs(),
                    (a as i32 - c as i32).abs(),
                    (a as i32 + b as i32 - 2 * c as i32).abs(),
                );
                if pa <= pb && pa <= pc {
                    a
                } else if pb <= pc {
                    b
                } else {
                    c
                }
            };
            out[base + x] = match ft {
                0 => src[x],
                1 => src[x].wrapping_add(left),
                2 => src[x].wrapping_add(up),
                3 => src[x].wrapping_add(((left as u16 + up as u16) / 2) as u8),
                4 => src[x].wrapping_add(paeth(left, up, up_left)),
                other => {
                    return Err(AppError::Corrupt(format!("PNG 滤波类型 {other} 无效")))
                }
            };
        }
    }
    Ok(out)
}

/// 从低位深（1/2/4 位）的扫描线中取出第 i 个索引
fn sub_byte_index(row: &[u8], i: usize, bit_depth: u8) -> u8 {
    let per_byte = 8 / bit_depth as usize;
    let byte = row[i / per_byte];
    let shift = 8 - bit_depth as usize * ((i % per_byte) + 1);
    (byte >> shift) & ((1u16 << bit_depth) - 1) as u8
}

/// 把已还原的扫描线转成 RGBA8888。
///
/// 支持 8 位的灰度/RGB/调色板/灰度+alpha/RGBA，以及 4 位与 1/2 位的
/// 灰度与调色板（Minecraft 贴图基本是 8 位 RGBA，但资源包里偶有低位深）。
pub fn to_rgba(
    raw: &[u8],
    width: u32,
    height: u32,
    color_type: u8,
    bit_depth: u8,
    palette: &Palette,
    transparency: Option<&[[u8; 3]]>,
) -> AppResult<Vec<u8>> {
    let w = width as usize;
    let h = height as usize;
    let stride = stride_of(width, color_type, bit_depth)?;
    if raw.len() != stride * h {
        return Err(AppError::Corrupt("PNG 扫描线长度与尺寸不符".into()));
    }
    let mut out = vec![0u8; w * h * 4];

    for y in 0..h {
        let row = &raw[y * stride..(y + 1) * stride];
        for x in 0..w {
            let px = (y * w + x) * 4;
            match (color_type, bit_depth) {
                (0, 8) => {
                    let g = row[x];
                    out[px] = g;
                    out[px + 1] = g;
                    out[px + 2] = g;
                    out[px + 3] = 255;
                }
                (0, 4) | (0, 2) | (0, 1) => {
                    // 低位深灰度需按位深缩放到 0..255
                    let v = sub_byte_index(row, x, bit_depth);
                    let max = ((1u16 << bit_depth) - 1) as u32;
                    let g = ((v as u32 * 255) / max) as u8;
                    out[px] = g;
                    out[px + 1] = g;
                    out[px + 2] = g;
                    out[px + 3] = 255;
                }
                (2, 8) => {
                    out[px] = row[x * 3];
                    out[px + 1] = row[x * 3 + 1];
                    out[px + 2] = row[x * 3 + 2];
                    out[px + 3] = 255;
                }
                (3, 8) => {
                    let idx = row[x] as usize;
                    let c = palette
                        .rgb
                        .get(idx)
                        .ok_or_else(|| AppError::Corrupt(format!("调色板下标 {idx} 越界")))?;
                    out[px] = c[0];
                    out[px + 1] = c[1];
                    out[px + 2] = c[2];
                    out[px + 3] = palette.alpha.get(idx).copied().unwrap_or(255);
                }
                (3, 4) | (3, 2) | (3, 1) => {
                    let idx = sub_byte_index(row, x, bit_depth) as usize;
                    let c = palette
                        .rgb
                        .get(idx)
                        .ok_or_else(|| AppError::Corrupt(format!("调色板下标 {idx} 越界")))?;
                    out[px] = c[0];
                    out[px + 1] = c[1];
                    out[px + 2] = c[2];
                    out[px + 3] = palette.alpha.get(idx).copied().unwrap_or(255);
                }
                (4, 8) => {
                    let g = row[x * 2];
                    out[px] = g;
                    out[px + 1] = g;
                    out[px + 2] = g;
                    out[px + 3] = row[x * 2 + 1];
                }
                (6, 8) => {
                    out[px] = row[x * 4];
                    out[px + 1] = row[x * 4 + 1];
                    out[px + 2] = row[x * 4 + 2];
                    out[px + 3] = row[x * 4 + 3];
                }
                (ct, bd) => {
                    return Err(AppError::Corrupt(format!(
                        "PNG 色型 {ct} 位深 {bd} 的组合不受支持"
                    )))
                }
            }
        }
    }

    // tRNS 对 8 位灰度与 RGB 表示「哪一个采样值视为全透明」
    if let Some(t) = transparency {
        match (color_type, bit_depth) {
            (0, 8) => {
                let key = t[0][0];
                for i in 0..w * h {
                    if raw[i] == key {
                        out[i * 4 + 3] = 0;
                    }
                }
            }
            (2, 8) => {
                let key = t[0];
                for y in 0..h {
                    let row = &raw[y * stride..(y + 1) * stride];
                    for x in 0..w {
                        if row[x * 3] == key[0] && row[x * 3 + 1] == key[1] && row[x * 3 + 2] == key[2]
                        {
                            out[(y * w + x) * 4 + 3] = 0;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    Ok(out)
}

/// 读取大端 u32
fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

/// 解码 PNG 字节
pub fn decode(data: &[u8]) -> AppResult<Image> {
    const SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if data.len() < 8 || data[..8] != SIG {
        return Err(AppError::Corrupt("不是 PNG：文件签名不符".into()));
    }

    let mut pos = 8usize;
    let (mut width, mut height) = (0u32, 0u32);
    let (mut color_type, mut bit_depth) = (0u8, 0u8);
    let mut idat: Vec<u8> = Vec::new();
    let mut palette = Palette::default();
    let mut transparency: Option<Vec<[u8; 3]>> = None;
    let mut saw_header = false;

    while pos + 8 <= data.len() {
        let len = be32(&data[pos..]) as usize;
        let ctype = &data[pos + 4..pos + 8];
        let body_start = pos + 8;
        let body_end = body_start
            .checked_add(len)
            .ok_or_else(|| AppError::Corrupt("PNG 块长度溢出".into()))?;
        if body_end + 4 > data.len() {
            return Err(AppError::Corrupt("PNG 块数据不完整".into()));
        }
        let body = &data[body_start..body_end];

        match ctype {
            b"IHDR" => {
                if body.len() < 13 {
                    return Err(AppError::Corrupt("IHDR 长度不足".into()));
                }
                width = be32(body);
                height = be32(&body[4..]);
                bit_depth = body[8];
                color_type = body[9];
                if width == 0 || height == 0 {
                    return Err(AppError::Corrupt("PNG 尺寸为 0".into()));
                }
                if body[12] != 0 {
                    return Err(AppError::UnsupportedVersion(
                        "暂不支持隔行（Adam7）PNG".into(),
                    ));
                }
                if !matches!(bit_depth, 1 | 2 | 4 | 8) {
                    return Err(AppError::UnsupportedVersion(format!(
                        "暂不支持 {bit_depth} 位深"
                    )));
                }
                channels(color_type)?; // 提前校验色型
                saw_header = true;
            }
            b"PLTE" => {
                palette.rgb = body.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
            }
            b"tRNS" => {
                match color_type {
                    // 调色板透明度逐项给出
                    3 => palette.alpha = body.to_vec(),
                    // 灰度/RGB 的透明键，取前 2 / 6 字节
                    0 if body.len() >= 2 => {
                        transparency = Some(vec![[body[1], 0, 0]]);
                    }
                    2 if body.len() >= 6 => {
                        transparency = Some(vec![[body[1], body[3], body[5]]]);
                    }
                    _ => {}
                }
            }
            b"IDAT" => idat.extend_from_slice(body),
            b"IEND" => break,
            _ => {} // 其余块（gAMA、pHYs 等）与像素无关
        }
        pos = body_end + 4;
    }

    if !saw_header {
        return Err(AppError::Corrupt("PNG 缺少 IHDR".into()));
    }
    if idat.is_empty() {
        return Err(AppError::Corrupt("PNG 缺少 IDAT".into()));
    }

    let raw = crate::nbt::inflate_zlib(&idat)?;
    let pixels = unfilter(&raw, width, height, color_type, bit_depth)?;
    let rgba = to_rgba(
        &pixels,
        width,
        height,
        color_type,
        bit_depth,
        &palette,
        transparency.as_deref(),
    )?;
    Ok(Image { width, height, rgba })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 把扫描线打包成带滤波字节的形式
    fn pack_rows(rows: &[Vec<u8>], filter: u8) -> Vec<u8> {
        let mut out = Vec::new();
        for r in rows {
            out.push(filter);
            out.extend_from_slice(r);
        }
        out
    }

    #[test]
    fn filter_none_passes_through() {
        let rows = vec![vec![10u8, 20, 30], vec![40, 50, 60]];
        let filtered = pack_rows(&rows, 0);
        let got = unfilter(&filtered, 3, 2, 0, 8).unwrap();
        assert_eq!(got, vec![10, 20, 30, 40, 50, 60]);
    }

    #[test]
    fn filter_sub_adds_left_neighbor() {
        // Sub：每个字节加上左侧同通道的还原值
        let rows = vec![vec![10u8, 20, 30]];
        let filtered = pack_rows(&rows, 1);
        let got = unfilter(&filtered, 3, 1, 0, 8).unwrap();
        assert_eq!(got[0], 10);
        assert_eq!(got[1], 30, "20 + 左侧 10");
        assert_eq!(got[2], 60, "30 + 左侧 30");
    }

    #[test]
    fn filter_up_adds_previous_row() {
        let rows = vec![vec![10u8, 20], vec![5, 7]];
        let filtered = pack_rows(&rows, 2);
        let got = unfilter(&filtered, 2, 2, 0, 8).unwrap();
        assert_eq!(&got[0..2], &[10, 20]);
        assert_eq!(&got[2..4], &[15, 27], "第二行加上第一行");
    }

    #[test]
    fn filter_average_matches_spec() {
        // Average：加 floor((左 + 上) / 2)。
        // 注意「左」指的是**已还原**的左侧像素，因此同一行内也会逐步传播，
        // 不能拿原始字节当期望值。
        let rows = vec![vec![10u8, 20], vec![4, 4]];
        let filtered = pack_rows(&rows, 3);
        let got = unfilter(&filtered, 2, 2, 0, 8).unwrap();

        // 第 0 行：x=0 左 0 上 0 → 10；x=1 左 10 上 0 → 20 + 5 = 25
        assert_eq!(got[0], 10);
        assert_eq!(got[1], 25, "20 + floor((10 + 0) / 2)");
        // 第 1 行：x=0 左 0 上 10 → 4 + 5 = 9；x=1 左 9 上 25 → 4 + 17 = 21
        assert_eq!(got[2], 9, "4 + floor((0 + 10) / 2)");
        assert_eq!(got[3], 21, "4 + floor((9 + 25) / 2)");
    }

    #[test]
    fn filter_paeth_picks_nearest() {
        // Paeth：a=左 b=上 c=左上，取预测值最接近的邻居
        let rows = vec![vec![10u8], vec![20]];
        let filtered = pack_rows(&rows, 4);
        let got = unfilter(&filtered, 1, 2, 0, 8).unwrap();
        assert_eq!(got[0], 10);
        // x=0 时 a=0 b=10 c=0 → pa=|10-0|=10 pb=0 pc=10 → 取 b=10
        assert_eq!(got[1], 30, "20 + 10");
    }

    #[test]
    fn unfilter_rejects_wrong_length() {
        // 少一行应报错，而不是给出错位像素
        let e = unfilter(&[0, 1, 2], 3, 2, 0, 8).unwrap_err().to_string();
        assert!(e.contains("不符"), "错误信息应说明长度不符：{e}");
    }

    #[test]
    fn unfilter_rejects_unknown_filter_type() {
        let filtered = vec![9u8, 1, 2, 3];
        let e = unfilter(&filtered, 3, 1, 0, 8).unwrap_err().to_string();
        assert!(e.contains("滤波类型"), "应指出滤波类型无效：{e}");
    }

    #[test]
    fn rgba8_is_taken_verbatim() {
        let raw = vec![1u8, 2, 3, 4, 5, 6, 7, 8];
        let got = to_rgba(&raw, 2, 1, 6, 8, &Palette::default(), None).unwrap();
        assert_eq!(got, vec![1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn rgb8_gets_opaque_alpha() {
        let raw = vec![1u8, 2, 3, 4, 5, 6];
        let got = to_rgba(&raw, 2, 1, 2, 8, &Palette::default(), None).unwrap();
        assert_eq!(got, vec![1, 2, 3, 255, 4, 5, 6, 255]);
    }

    #[test]
    fn gray4_is_scaled_to_full_range() {
        // 两个 4 位灰度样本打包在一个字节：0x0F = 灰 0 与灰 15
        let raw = vec![0x0Fu8];
        let got = to_rgba(&raw, 2, 1, 0, 4, &Palette::default(), None).unwrap();
        assert_eq!(got[0], 0);
        assert_eq!(got[3], 255);
        assert_eq!(got[4], 255, "4 位最大值应映射到 255");
    }

    #[test]
    fn palette_uses_alpha_table() {
        let pal = Palette {
            rgb: vec![[10, 20, 30], [40, 50, 60]],
            alpha: vec![128, 255],
        };
        let raw = vec![1u8, 0];
        let got = to_rgba(&raw, 2, 1, 3, 8, &pal, None).unwrap();
        assert_eq!(&got[0..4], &[40, 50, 60, 255]);
        assert_eq!(&got[4..8], &[10, 20, 30, 128]);
    }

    #[test]
    fn palette_index_out_of_range_is_an_error() {
        let pal = Palette { rgb: vec![[1, 2, 3]], alpha: Vec::new() };
        let raw = vec![5u8];
        let e = to_rgba(&raw, 1, 1, 3, 8, &pal, None).unwrap_err().to_string();
        assert!(e.contains("越界"), "应指出调色板越界：{e}");
    }

    #[test]
    fn gray_alpha_splits_channels() {
        let raw = vec![77u8, 128];
        let got = to_rgba(&raw, 1, 1, 4, 8, &Palette::default(), None).unwrap();
        assert_eq!(got, vec![77, 77, 77, 128]);
    }

    #[test]
    fn transparency_key_makes_matching_pixel_fully_transparent() {
        let raw = vec![9u8, 9, 9, 1, 2, 3];
        let key = [[9u8, 9, 9]];
        let got = to_rgba(&raw, 2, 1, 2, 8, &Palette::default(), Some(&key)).unwrap();
        assert_eq!(got[3], 0, "与透明键相符的像素 alpha 应为 0");
        assert_eq!(got[7], 255, "其余像素保持不透明");
    }

    #[test]
    fn decode_rejects_non_png() {
        let e = decode(b"not a png at all").unwrap_err().to_string();
        assert!(e.contains("签名"), "应指出签名不符：{e}");
    }

    #[test]
    fn stride_accounts_for_sub_byte_depths() {
        // 5 个 4 位像素 = 20 位 = 3 字节
        assert_eq!(stride_of(5, 0, 4).unwrap(), 3);
        // 8 个 1 位像素 = 1 字节
        assert_eq!(stride_of(8, 0, 1).unwrap(), 1);
        // 3 个 RGB 像素 = 9 字节
        assert_eq!(stride_of(3, 2, 8).unwrap(), 9);
    }

    #[test]
    fn unsupported_color_type_is_rejected() {
        let e = channels(7).unwrap_err().to_string();
        assert!(e.contains("色型"), "应指出色型不支持：{e}");
    }
}
