//! 方块库读取。
//!
//! 格式见 docs/blocks-dat-format.md，由 legacy-cpp 的写出实现
//! （packbuild.cpp）与读取实现（blockdb.cpp）对照确认。
//!
//! 每个 pack 文件只含一个版本；程序启动时加载多个 pack
//! （基础版 + 模组叠加包）后合并使用。

use crate::error::{AppError, AppResult};
use std::collections::HashMap;

const MAGIC: &[u8; 7] = b"PMBLKDB";
const FORMAT_VERSION: u32 = 3;

/// 各段的数量上限，用于拒绝异常或损坏的文件。
/// 取值与 legacy-cpp 的读取实现一致。
const MAX_VERSIONS: u32 = 100_000;
const MAX_TEXTURES: u32 = 2_000_000;
const MAX_MODELS: u32 = 5_000_000;
const MAX_LANGS: u32 = 100_000;
const MAX_STRINGS: u32 = 20_000_000;

/// 贴图缺失的哨兵值
pub const NO_TEXTURE: u32 = 0xFFFF_FFFF;

/// 六个面。faceMask 的位 0~5 依次对应下表。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Face {
    Down = 0,
    Up = 1,
    North = 2,
    South = 3,
    West = 4,
    East = 5,
}

/// 模型中的一个面
#[derive(Debug, Clone, PartialEq)]
pub struct ModelFace {
    /// 贴图**字符串表下标**，不是贴图池下标。
    ///
    /// 两者数值空间完全无关：实测随包的方块库中字符串表 6263 项、贴图池仅
    /// 1685 项，48% 的面的取值超出贴图池长度。若误把字符串下标当池下标用，
    /// 多数面会取不到贴图（渲染空白），少数会取到完全无关的贴图。
    ///
    /// 这里不能直接存池下标：模型跨版本共享，而路径到池下标的映射按版本
    /// 不同，只能在按版本查用时解析（见 BlockDb::texture_pool_index）。
    pub texture: u32,
    pub uv: [f32; 4],
    /// 0xFF 表示无剔除面
    pub cullface: u8,
    pub tintindex: i16,
    /// 已归一化到 0~359
    pub rotation: u16,
}

/// 模型元素（一个长方体）
#[derive(Debug, Clone, PartialEq)]
pub struct ModelElement {
    pub from: [f32; 3],
    pub to: [f32; 3],
    pub has_rotation: bool,
    pub rot_kind: u8,
    pub origin: [f32; 3],
    pub axis: u8,
    pub angle: f32,
    pub rescale: bool,
    pub rot_vec: [f32; 3],
    pub face_mask: u8,
    /// 顺序对应 face_mask 中自低位到高位置位的面
    pub faces: Vec<ModelFace>,
}

impl ModelElement {
    /// 取指定面的数据（若该面存在）
    pub fn face(&self, f: Face) -> Option<&ModelFace> {
        if self.face_mask & (1u8 << (f as u8)) == 0 {
            return None;
        }
        // faces 按位置位的顺序排列，需要数出该位之前有几个置位
        let idx = (0..(f as u8)).filter(|b| self.face_mask & (1u8 << b) != 0).count();
        self.faces.get(idx)
    }
}

/// 模型
#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    pub translucent: bool,
    pub elements: Vec<ModelElement>,
}

/// 贴图在像素池中的位置
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextureInfo {
    pub w: u16,
    pub h: u16,
    pub offset: u32,
    pub len: u32,
    pub has_alpha: bool,
}

/// 语言表条目
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LangEntry {
    pub block_name: String,
    pub en: String,
    pub zh: String,
}

/// 方块的一个变体（或 multipart 的一个分支）
#[derive(Debug, Clone, PartialEq)]
pub struct Variant {
    /// 条件字符串在方块属性上下文中的表达
    pub when: String,
    pub model_idx: u32,
    /// 绕 X 轴旋转的 90 度倍数
    pub rot_x_div90: u8,
    /// 绕 Y 轴旋转的 90 度倍数
    pub rot_y_div90: u8,
    pub uvlock: bool,
}

/// 方块定义
#[derive(Debug, Clone, PartialEq)]
pub struct BlockDef {
    pub name: String,
    pub variants: Vec<Variant>,
    pub multipart: Vec<Variant>,
}

/// 一个版本
#[derive(Debug, Clone)]
pub struct VersionDef {
    pub name: String,
    pub data_version: i32,
    pub lang_idx: u32,
    /// 贴图路径 -> 贴图池下标
    pub texture_by_path: HashMap<String, u32>,
    pub blocks: Vec<BlockDef>,
    pub langs: Vec<LangEntry>,
    pub is_overlay: bool,
}

impl VersionDef {
    /// 按方块名查中文名，找不到时回退到英文名，再回退到方块名本身
    pub fn display_name(&self, block_name: &str) -> String {
        for e in &self.langs {
            if e.block_name == block_name {
                if !e.zh.is_empty() {
                    return e.zh.clone();
                }
                if !e.en.is_empty() {
                    return e.en.clone();
                }
                break;
            }
        }
        block_name
            .split_once(':')
            .map(|(_, n)| n.to_string())
            .unwrap_or_else(|| block_name.to_string())
    }
}

/// 解析器。所有读取都做边界检查，损坏数据返回错误而不是越界。
struct BinReader<'a> {
    data: &'a [u8],
    pos: usize,
    le: bool,
}

impl<'a> BinReader<'a> {
    fn new(data: &'a [u8], little_endian: bool) -> Self {
        Self { data, pos: 0, le: little_endian }
    }

    fn take(&mut self, n: usize) -> AppResult<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| AppError::Corrupt("读取长度溢出".into()))?;
        if end > self.data.len() {
            return Err(AppError::Corrupt(format!(
                "数据不足：位置 {} 需要 {} 字节，总长 {}",
                self.pos,
                n,
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
    fn u16(&mut self) -> AppResult<u16> {
        let b = self.take(2)?;
        let a = [b[0], b[1]];
        Ok(if self.le { u16::from_le_bytes(a) } else { u16::from_be_bytes(a) })
    }
    fn u32(&mut self) -> AppResult<u32> {
        let b = self.take(4)?;
        let a = [b[0], b[1], b[2], b[3]];
        Ok(if self.le { u32::from_le_bytes(a) } else { u32::from_be_bytes(a) })
    }
    fn i16(&mut self) -> AppResult<i16> {
        Ok(self.u16()? as i16)
    }
    fn i32(&mut self) -> AppResult<i32> {
        Ok(self.u32()? as i32)
    }
    fn f32(&mut self) -> AppResult<f32> {
        Ok(f32::from_bits(self.u32()?))
    }

    /// NUL 结尾的字符串。
    ///
    /// 格式本身不存长度，因此必须限制扫描范围：损坏文件里出现
    /// 缺失终止符时，无界扫描会一直读到文件尾甚至越界。
    fn cstr(&mut self) -> AppResult<String> {
        const MAX_STR: usize = 64 * 1024;
        let start = self.pos;
        let mut end = start;
        while end < self.data.len() && self.data[end] != 0 {
            end += 1;
            if end - start > MAX_STR {
                return Err(AppError::Corrupt("字符串长度异常，可能文件已损坏".into()));
            }
        }
        if end >= self.data.len() {
            return Err(AppError::Corrupt("字符串缺少终止符，文件已截断".into()));
        }
        let s = String::from_utf8_lossy(&self.data[start..end]).into_owned();
        self.pos = end + 1;
        Ok(s)
    }

    /// 读一个 f32 三元组
    fn f32x3(&mut self) -> AppResult<[f32; 3]> {
        Ok([self.f32()?, self.f32()?, self.f32()?])
    }

    /// 读一个 u32 并做上限校验
    fn count(&mut self, max: u32, what: &str) -> AppResult<u32> {
        let n = self.u32()?;
        if n > max {
            return Err(AppError::Corrupt(format!("{what}数量异常：{n}")));
        }
        Ok(n)
    }
}

/// 整个方块库
///
/// 注意：Debug 是派生实现的，格式化输出会包含整个像素池，
/// 因此不要在日志里直接打印本类型。
#[derive(Debug)]
pub struct BlockDb {
    pub format_version: u32,
    pub strings: Vec<String>,
    pub langs: Vec<Vec<LangEntry>>,
    pub textures: Vec<TextureInfo>,
    pub models: Vec<Model>,
    pub versions: Vec<VersionDef>,
    /// zlib 解压后的 RGBA8888 像素池
    pub pixels: Vec<u8>,
}

impl BlockDb {
    /// 从已解压的字节解析。
    ///
    /// `is_overlay` 必须由调用方给出，不能从数据推测：叠加包与基础版本的
    /// 差别在于**资源包类型**，而资源包类型是导入时由用户选择的
    /// （legacy-cpp 的 PackInfo::kind：0=内置基础库 1=MC 版本包 2=模组包，
    /// 见 blockdb_packs.cpp:41），并不记录在 pack 文件里。
    ///
    /// 尤其不能以 dataVersion 为负来判定：打包脚本会把**缺少**
    /// data_version 的普通版本也记为 -1
    /// （legacy-cpp/tools/pack_blockdb.py:446），那样会把基础版误判成
    /// 叠加包，进而把它排除在版本选择之外。
    pub fn load_from_bytes(data: &[u8], is_overlay: bool) -> AppResult<Self> {
        let mut r = BinReader::new(data, true);

        let magic = r.take(8)?;
        if &magic[..7] != MAGIC {
            return Err(AppError::Parse("方块数据库魔数不匹配".into()));
        }
        let format_version = r.u32()?;
        if format_version != FORMAT_VERSION {
            return Err(AppError::UnsupportedVersion(format!(
                "方块库格式版本 {format_version}，本程序支持 {FORMAT_VERSION}"
            )));
        }
        let version_count = r.count(MAX_VERSIONS, "版本")?;
        let texture_count = r.count(MAX_TEXTURES, "贴图")?;
        let model_count = r.count(MAX_MODELS, "模型")?;
        let lang_count = r.count(MAX_LANGS, "语言表")?;

        // ---- 字符串表 ----
        let str_count = r.count(MAX_STRINGS, "字符串")?;
        let mut strings = Vec::with_capacity(str_count as usize);
        for _ in 0..str_count {
            strings.push(r.cstr()?);
        }
        // 段内重复计数，必须与文件头一致
        let sec = r.u32()?;
        if sec != lang_count {
            return Err(AppError::Corrupt(format!(
                "语言表段内计数 {sec} 与文件头 {lang_count} 不一致"
            )));
        }

        // ---- 语言表 ----
        let mut langs = Vec::with_capacity(lang_count as usize);
        for _ in 0..lang_count {
            let n = r.count(MAX_STRINGS, "语言条目")?;
            let mut group = Vec::with_capacity(n as usize);
            for _ in 0..n {
                let bi = r.u32()?;
                let ei = r.u32()?;
                let zi = r.u32()?;
                group.push(LangEntry {
                    block_name: str_at(&strings, bi),
                    en: str_at(&strings, ei),
                    zh: str_at(&strings, zi),
                });
            }
            langs.push(group);
        }

        // ---- 贴图表 ----
        let sec = r.u32()?;
        if sec != texture_count {
            return Err(AppError::Corrupt(format!(
                "贴图段内计数 {sec} 与文件头 {texture_count} 不一致"
            )));
        }
        let mut textures = Vec::with_capacity(texture_count as usize);
        for _ in 0..texture_count {
            textures.push(TextureInfo {
                w: r.u16()?,
                h: r.u16()?,
                offset: r.u32()?,
                len: r.u32()?,
                has_alpha: r.u8()? != 0,
            });
        }
        let raw_size = r.u32()?;
        let comp_size = r.u32()?;
        let comp = r.take(comp_size as usize)?;
        let pixels = decompress(comp, raw_size as usize)?;

        // ---- 模型表 ----
        let sec = r.u32()?;
        if sec != model_count {
            return Err(AppError::Corrupt(format!(
                "模型段内计数 {sec} 与文件头 {model_count} 不一致"
            )));
        }
        let mut models = Vec::with_capacity(model_count.min(65536) as usize);
        for _ in 0..model_count {
            let translucent = r.u8()? != 0;
            let elem_count = r.u16()? as usize;
            let mut elements = Vec::with_capacity(elem_count.min(256));
            for _ in 0..elem_count {
                let from = r.f32x3()?;
                let to = r.f32x3()?;
                let has_rotation = r.u8()? != 0;
                let mut rot_kind = 0;
                let mut origin = [0.0f32; 3];
                let mut axis = 0;
                let mut angle = 0.0f32;
                let mut rescale = false;
                let mut rot_vec = [0.0f32; 3];
                if has_rotation {
                    rot_kind = r.u8()?;
                    origin = r.f32x3()?;
                    axis = r.u8()?;
                    angle = r.f32()?;
                    rescale = r.u8()? != 0;
                    rot_vec = r.f32x3()?;
                }
                let face_mask = r.u8()?;
                let mut faces = Vec::new();
                for bit in 0..6u8 {
                    if face_mask & (1u8 << bit) == 0 {
                        continue;
                    }
                    let texture = r.u32()?;
                    let uv = [r.f32()?, r.f32()?, r.f32()?, r.f32()?];
                    let cullface = r.u8()?;
                    let tintindex = r.i16()?;
                    let rotation = r.u16()?;
                    faces.push(ModelFace { texture, uv, cullface, tintindex, rotation });
                }
                elements.push(ModelElement {
                    from,
                    to,
                    has_rotation,
                    rot_kind,
                    origin,
                    axis,
                    angle,
                    rescale,
                    rot_vec,
                    face_mask,
                    faces,
                });
            }
            models.push(Model { translucent, elements });
        }

        // ---- 版本段 ----
        let sec = r.u32()?;
        if sec > MAX_VERSIONS {
            return Err(AppError::Corrupt(format!("版本段内计数异常：{sec}")));
        }
        if sec != version_count {
            return Err(AppError::Corrupt(format!(
                "版本段内计数 {sec} 与文件头 {version_count} 不一致"
            )));
        }
        let mut versions = Vec::with_capacity(sec as usize);
        for _ in 0..sec {
            let name_idx = r.u32()?;
            let data_version = r.i32()?;
            let lang_idx = r.u32()?;
            let tex_path_count = r.count(MAX_TEXTURES, "版本贴图路径")?;
            let mut texture_by_path = HashMap::with_capacity(tex_path_count.min(65536) as usize);
            for _ in 0..tex_path_count {
                let path_idx = r.u32()?;
                let pool_idx = r.u32()?;
                texture_by_path.insert(str_at(&strings, path_idx), pool_idx);
            }
            let block_count = r.count(MAX_STRINGS, "方块")?;
            let mut blocks = Vec::with_capacity(block_count.min(65536) as usize);
            for _ in 0..block_count {
                let block_name = str_at(&strings, r.u32()?);
                let variant_count = r.u16()? as usize;
                let mut variants = Vec::with_capacity(variant_count.min(256));
                for _ in 0..variant_count {
                    variants.push(read_variant(&mut r, &strings)?);
                }
                let mp_count = r.u16()? as usize;
                let mut multipart = Vec::with_capacity(mp_count.min(256));
                for _ in 0..mp_count {
                    multipart.push(read_variant(&mut r, &strings)?);
                }
                blocks.push(BlockDef {
                    name: block_name,
                    variants,
                    multipart,
                });
            }
            let langs_for_version = langs.get(lang_idx as usize).cloned().unwrap_or_default();
            versions.push(VersionDef {
                name: str_at(&strings, name_idx),
                data_version,
                lang_idx,
                texture_by_path,
                blocks,
                langs: langs_for_version,
                // 由载入方给出，不推测。dataVersion 为负只表示打包时
                // 缺少版本号，与是否为叠加包无关。
                is_overlay,
            });
        }

        // 贴图偏移必须落在像素池范围内，否则渲染时会越界读取
        for (i, t) in textures.iter().enumerate() {
            let end = t.offset as usize + t.len as usize;
            if end > pixels.len() {
                return Err(AppError::Corrupt(format!(
                    "贴图 {i} 的范围 {}+{} 超出像素池长度 {}",
                    t.offset,
                    t.len,
                    pixels.len()
                )));
            }
        }

        Ok(Self { format_version, strings, langs, textures, models, versions, pixels })
    }

    /// 从 pack 文件载入（文件为 zlib 压缩）。
    ///
    /// 参数含义见 load_from_bytes。
    pub fn load(path: &str, is_overlay: bool) -> AppResult<Self> {
        let bytes = std::fs::read(path)?;
        let raw = crate::nbt::gunzip(&bytes)
            .or_else(|_| Ok::<Vec<u8>, AppError>(bytes.clone()))?;
        Self::load_from_bytes(&raw, is_overlay)
    }

    /// 把模型的贴图字符串表下标解析成指定版本的贴图池下标。
    ///
    /// 模型跨版本共享，而路径到池下标的映射按版本不同，解析必须发生在
    /// 知道版本之后。找不到对应贴图时返回 NO_TEXTURE，下游据此跳过该面，
    /// 而不是拿一个错误的索引去取完全无关的贴图。
    pub fn texture_pool_index(&self, version_idx: usize, tex_str_idx: u32) -> u32 {
        if tex_str_idx == NO_TEXTURE {
            return NO_TEXTURE;
        }
        let Some(path) = self.strings.get(tex_str_idx as usize) else {
            return NO_TEXTURE;
        };
        self.versions
            .get(version_idx)
            .and_then(|v| v.texture_by_path.get(path).copied())
            .unwrap_or(NO_TEXTURE)
    }

    /// 取某个贴图的像素切片
    pub fn texture_pixels(&self, idx: usize) -> Option<&[u8]> {
        let t = self.textures.get(idx)?;
        let start = t.offset as usize;
        self.pixels.get(start..start + t.len as usize)
    }

    /// 按 dataVersion 选最接近的版本（下标）
    pub fn pick_version(&self, data_version: i32) -> Option<usize> {
        self.versions
            .iter()
            .enumerate()
            .filter(|(_, v)| !v.is_overlay)
            .min_by_key(|(_, v)| (v.data_version - data_version).abs())
            .map(|(i, _)| i)
    }
}


fn read_variant(r: &mut BinReader<'_>, strings: &[String]) -> AppResult<Variant> {
    Ok(Variant {
        when: str_at(strings, r.u32()?),
        model_idx: r.u32()?,
        rot_x_div90: r.u8()?,
        rot_y_div90: r.u8()?,
        uvlock: r.u8()? != 0,
    })
}

fn str_at(strings: &[String], idx: u32) -> String {
    strings.get(idx as usize).cloned().unwrap_or_default()
}

fn decompress(data: &[u8], expect: usize) -> AppResult<Vec<u8>> {
    use std::io::Read;
    let mut d = flate2::read::ZlibDecoder::new(data);
    let mut out = Vec::with_capacity(expect.min(64 * 1024 * 1024));
    d.read_to_end(&mut out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一个最小可用的 pack 字节流，用于验证解析流程。
    /// 真实文件由 legacy-cpp 的 packbuild 生成，这里只覆盖格式解析本身。
    fn build_minimal_pack() -> Vec<u8> {
        build_minimal_pack_with_dv(3465)
    }

    /// 构造最小 pack，可指定 dataVersion
    fn build_minimal_pack_with_dv(data_version: i32) -> Vec<u8> {
        let mut w: Vec<u8> = Vec::new();
        let s32 = |w: &mut Vec<u8>, v: i32| w.extend_from_slice(&v.to_le_bytes());
        let u32f = |w: &mut Vec<u8>, v: u32| w.extend_from_slice(&v.to_le_bytes());
        let u16f = |w: &mut Vec<u8>, v: u16| w.extend_from_slice(&v.to_le_bytes());

        w.extend_from_slice(b"PMBLKDB\0");
        u32f(&mut w, 3); // format
        u32f(&mut w, 1); // versions
        u32f(&mut w, 1); // textures
        u32f(&mut w, 1); // models
        u32f(&mut w, 1); // langs

        // 字符串表：索引 0=版本名 1=方块名 2=贴图路径 3=条件 4=en 5=zh
        let strs = ["1.20.1", "minecraft:stone", "block/stone", "", "Stone", "石头"];
        u32f(&mut w, strs.len() as u32);
        for s in strs {
            w.extend_from_slice(s.as_bytes());
            w.push(0);
        }

        // 语言表。注意读取端在「段内重复计数」之后直接按 langCount 逐组读取，
        // 没有独立的「组数」字段——多写一个 u32 会让后续全部错位。
        u32f(&mut w, 1); // 段内重复计数
        u32f(&mut w, 1); // 第 0 组的条目数
        u32f(&mut w, 1); // blockName 下标
        u32f(&mut w, 4); // en 下标
        u32f(&mut w, 5); // zh 下标

        // 贴图表：1 张 2x2 贴图 = 16 字节像素
        u32f(&mut w, 1);
        u16f(&mut w, 2);
        u16f(&mut w, 2);
        u32f(&mut w, 0);
        u32f(&mut w, 16);
        w.push(1); // hasAlpha
        let px: Vec<u8> = (0..16).collect();
        let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        use std::io::Write;
        enc.write_all(&px).unwrap();
        let comp = enc.finish().unwrap();
        u32f(&mut w, 16);
        u32f(&mut w, comp.len() as u32);
        w.extend_from_slice(&comp);

        // 模型表：1 个模型，1 个元素，仅含 Cube 的 down 面
        u32f(&mut w, 1);
        w.push(0); // translucent
        u16f(&mut w, 1); // 元素数
        for v in [0.0f32, 0.0, 0.0] {
            w.extend_from_slice(&v.to_le_bytes());
        }
        for v in [16.0f32, 16.0, 16.0] {
            w.extend_from_slice(&v.to_le_bytes());
        }
        w.push(0); // hasRot
        w.push(1); // faceMask = 仅位 0
        u32f(&mut w, 0); // texIdx
        for v in [0.0f32, 0.0, 16.0, 16.0] {
            w.extend_from_slice(&v.to_le_bytes());
        }
        w.push(0xFF); // cullface
        w.extend_from_slice(&(-1i16).to_le_bytes()); // tintindex
        u16f(&mut w, 0); // rotation

        // 版本段
        u32f(&mut w, 1); // 段内重复计数
        u32f(&mut w, 0); // 版本名下标
        s32(&mut w, data_version); // dataVersion
        u32f(&mut w, 0); // langTableIdx
        u32f(&mut w, 1); // 贴图路径数
        u32f(&mut w, 2);
        u32f(&mut w, 0);
        u32f(&mut w, 1); // 方块数
        u32f(&mut w, 1); // 方块名下标
        u16f(&mut w, 1); // 变体数
        u32f(&mut w, 3); // when
        u32f(&mut w, 0); // modelIdx
        w.push(0);
        w.push(0);
        w.push(1); // uvlock
        u16f(&mut w, 0); // multipart 数

        w
    }

    #[test]
    fn parses_minimal_pack() {
        let db = BlockDb::load_from_bytes(&build_minimal_pack(), false).expect("解析失败");
        assert_eq!(db.format_version, 3);
        assert_eq!(db.versions.len(), 1);
        assert_eq!(db.models.len(), 1);
        assert_eq!(db.textures.len(), 1);
        assert_eq!(db.pixels.len(), 16);

        let v = &db.versions[0];
        assert_eq!(v.name, "1.20.1");
        assert_eq!(v.data_version, 3465);
        assert!(!v.is_overlay);
        assert_eq!(v.blocks.len(), 1);
        assert_eq!(v.blocks[0].name, "minecraft:stone");
        assert_eq!(v.blocks[0].variants.len(), 1);
        assert!(v.blocks[0].variants[0].uvlock);
        assert_eq!(v.texture_by_path.get("block/stone"), Some(&0));
        // 中文名应能查到
        assert_eq!(v.display_name("minecraft:stone"), "石头");
        assert_eq!(v.display_name("minecraft:unknown"), "unknown");
    }

    #[test]
    fn model_face_lookup_follows_mask_order() {
        let db = BlockDb::load_from_bytes(&build_minimal_pack(), false).unwrap();
        let e = &db.models[0].elements[0];
        assert_eq!(e.face_mask, 1);
        assert!(e.face(Face::Down).is_some(), "位 0 对应 Down");
        assert!(e.face(Face::Up).is_none());
        assert_eq!(e.face(Face::Down).unwrap().tintindex, -1);
        assert_eq!(e.face(Face::Down).unwrap().cullface, 0xFF);
    }

    #[test]
    fn texture_pixels_are_sliced_correctly() {
        let db = BlockDb::load_from_bytes(&build_minimal_pack(), false).unwrap();
        let px = db.texture_pixels(0).unwrap();
        assert_eq!(px, &(0u8..16).collect::<Vec<u8>>()[..]);
        assert!(db.texture_pixels(99).is_none());
    }

    #[test]
    fn pick_version_chooses_nearest() {
        let db = BlockDb::load_from_bytes(&build_minimal_pack(), false).unwrap();
        assert_eq!(db.pick_version(3000), Some(0));
        assert_eq!(db.pick_version(3465), Some(0));
    }

    #[test]
    fn bad_magic_is_rejected() {
        let mut b = assert_baseline_parses();
        b[0] = b'X';
        assert!(BlockDb::load_from_bytes(&b, false).is_err());
    }

    #[test]
    fn unsupported_format_version_is_rejected() {
        let mut b = assert_baseline_parses();
        b[8..12].copy_from_slice(&99u32.to_le_bytes());
        let e = BlockDb::load_from_bytes(&b, false).unwrap_err().to_string();
        assert!(e.contains("格式版本"), "错误信息应说明版本不支持：{e}");
    }

    #[test]
    fn truncated_data_errors_instead_of_panicking() {
        let full = build_minimal_pack();
        for cut in [10usize, 40, 120, 200, full.len() - 1] {
            let _ = BlockDb::load_from_bytes(&full[..cut.min(full.len())], false);
        }
    }

    /// 语言表段内重复计数字段的字节偏移。
    ///
    /// 按布局逐段跳过：文件头(8+5*4) -> 字符串计数与各字符串 -> 该字段。
    fn lang_section_count_offset(b: &[u8]) -> usize {
        let mut pos = 8 + 5 * 4;
        let n = u32::from_le_bytes([b[pos], b[pos + 1], b[pos + 2], b[pos + 3]]) as usize;
        pos += 4;
        for _ in 0..n {
            while b[pos] != 0 {
                pos += 1;
            }
            pos += 1;
        }
        pos
    }

    /// 篡改型测试的公共前置。
    ///
    /// 必须先确认未篡改的 pack 能正常解析：否则篡改后的失败可能来自
    /// 别的错误，测试就变成「因为错误的原因通过」——此前就踩过这个坑。
    fn assert_baseline_parses() -> Vec<u8> {
        let good = build_minimal_pack();
        assert!(
            BlockDb::load_from_bytes(&good, false).is_ok(),
            "基准 pack 必须能正常解析，否则本测试验证的不是被篡改的那个校验"
        );
        good
    }

    #[test]
    fn section_count_mismatch_is_reported() {
        let mut b = assert_baseline_parses();
        let pos = lang_section_count_offset(&b);
        b[pos..pos + 4].copy_from_slice(&7u32.to_le_bytes());

        let e = BlockDb::load_from_bytes(&b, false).unwrap_err().to_string();
        assert!(e.contains("不一致"), "应报段内计数不一致：{e}");
        // 错误信息应指明是哪个段，便于排查
        assert!(e.contains("语言表"), "错误信息应指明段名：{e}");
    }

    #[test]
    fn truncated_string_table_is_rejected() {
        // 截在字符串表中间：缺少终止符或数据不足，都应报错而不是越界
        let mut b = build_minimal_pack();
        b.truncate(8 + 5 * 4 + 4 + 3);
        assert!(BlockDb::load_from_bytes(&b, false).is_err());
    }

    #[test]
    fn texture_out_of_range_is_reported() {
        // 把贴图的长度改到超出像素池，应被显式拒绝而不是留下越界偏移
        let mut b = assert_baseline_parses();
        let mut pos = lang_section_count_offset(&b);
        pos += 4;                 // 语言表段内重复计数
        pos += 4;                 // 第 0 组条目数
        pos += 12;                // 三个字符串下标
        pos += 4;                 // 贴图段内重复计数
        pos += 2 + 2 + 4;         // w, h, offset
        b[pos..pos + 4].copy_from_slice(&9999u32.to_le_bytes()); // len 越界
        let e = BlockDb::load_from_bytes(&b, false).unwrap_err().to_string();
        assert!(e.contains("超出像素池"), "应报贴图范围越界：{e}");
    }

    /// 负的 dataVersion 不等于「叠加包」。
    ///
    /// 打包脚本会把缺少 data_version 的**普通版本**记为 -1
    /// （legacy-cpp/tools/pack_blockdb.py:446），而叠加包与否取决于
    /// 资源包类型（导入时用户选择），并不在文件里。若用负值判定，
    /// 这类基础版会被误标为叠加包，从而被排除在版本选择之外。
    #[test]
    fn negative_data_version_is_not_an_overlay() {
        let pack = build_minimal_pack_with_dv(-1);

        // 明确告知是基础版本：不应被判为叠加包
        let base = BlockDb::load_from_bytes(&pack, false).expect("解析失败");
        assert_eq!(base.versions[0].data_version, -1);
        assert!(!base.versions[0].is_overlay, "负 dataVersion 不应被判为叠加包");
        // 基础版本必须能参与版本选择；若被误标为叠加包就会被排除
        assert!(
            base.pick_version(-1).is_some(),
            "基础版本即便缺少 dataVersion 也应能参与版本选择"
        );

        // 明确告知是叠加包：应如实标记
        let ov = BlockDb::load_from_bytes(&pack, true).expect("解析失败");
        assert!(ov.versions[0].is_overlay, "调用方声明的叠加包应被标记");
    }
}
