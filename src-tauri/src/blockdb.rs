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

/// 本程序支持的方块库格式版本。
///
/// 载入时用它校验文件头；对外公开是为了让诊断工具能报告「读到的格式版本」，
/// 而无需在 `BlockDb` 里再存一份与常量重复的副本。
pub const FORMAT_VERSION: u32 = 3;

/// 各段的数量上限，用于拒绝异常或损坏的文件。
/// 取值与 legacy-cpp 的读取实现一致。
const MAX_VERSIONS: u32 = 100_000;
const MAX_TEXTURES: u32 = 2_000_000;
const MAX_MODELS: u32 = 5_000_000;
const MAX_LANGS: u32 = 100_000;
const MAX_STRINGS: u32 = 20_000_000;
/// 单版本方块数上限。legacy-cpp 用的是 500000（blockdb.cpp:247），
/// 与字符串表的 2000 万不是一个量级——误用后者会让损坏文件
/// 通过校验并触发超大分配。
const MAX_BLOCKS: u32 = 500_000;

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
    #[cfg(test)]
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
            let block_count = r.count(MAX_BLOCKS, "方块")?;
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
            // 语言表下标必须能在本包的语言表池里取到。
            // 用 `unwrap_or_default()` 静默给一张空表的话，该版本的所有方块
            // 都会显示不出中文名，而用户看不出是数据坏了还是本来就没有名字。
            // lang_idx 是文件里的字段，属不可信输入，因此这里显式报错。
            let langs_for_version = langs.get(lang_idx as usize).cloned().ok_or_else(|| {
                AppError::Corrupt(format!(
                    "版本的语言表下标 {lang_idx} 超出了包内语言表数量 {}",
                    langs.len()
                ))
            })?;
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

        Ok(Self { strings, langs, textures, models, versions, pixels })
    }

    /// 从 pack 文件载入。
    ///
    /// 随包分发的 blocks.dat.z 用 **zlib** 容器压缩（与 legacy-cpp 的
    /// `uncompress` 一致），而 `.litematic` 用的是 gzip。两者容器不同，
    /// 只按一种解压会让内置方块库在发布版整块不可用。
    /// 因此这里按魔数分派：gzip 用 gunzip、zlib 用 inflate_zlib，
    /// 都不是时按未压缩处理。
    ///
    /// 参数含义见 load_from_bytes。
    pub fn load(path: &str, is_overlay: bool) -> AppResult<Self> {
        let bytes = std::fs::read(path)?;
        let raw = Self::decompress(&bytes)?;
        Self::load_from_bytes(&raw, is_overlay)
    }

    /// 按容器魔数解压：gzip（1f 8b）或 zlib（78 xx），都不是则原样返回。
    fn decompress(bytes: &[u8]) -> AppResult<Vec<u8>> {
        if bytes.len() >= 2 && bytes[0] == 0x1f && bytes[1] == 0x8b {
            return crate::nbt::gunzip(bytes);
        }
        // zlib 头：CMF 低 4 位为 8（deflate），且 (CMF*256+FLG) 能被 31 整除
        if bytes.len() >= 2 && bytes[0] & 0x0f == 8 {
            let hdr = bytes[0] as u16 * 256 + bytes[1] as u16;
            if hdr % 31 == 0 {
                return crate::nbt::inflate_zlib(bytes);
            }
        }
        Ok(bytes.to_vec())
    }

    /// 把模型的贴图字符串表下标解析成指定版本的贴图池下标。
    ///
    /// 模型跨版本共享，而路径到池下标的映射按版本不同，解析必须发生在
    /// 知道版本之后。找不到对应贴图时返回 NO_TEXTURE，下游据此跳过该面，
    /// 而不是拿一个错误的索引去取完全无关的贴图。
    ///
    /// 查找顺序与 legacy-cpp 的 FindTexture 一致：
    /// 本版本 -> 挂在该版本上的叠加包 -> 全局。
    /// 少了叠加包这一层，模组导入的贴图永远用不上——模组的方块定义
    /// 引用的贴图只存在于模组包里。
    pub fn texture_pool_index(&self, version_idx: usize, tex_str_idx: u32) -> u32 {
        if tex_str_idx == NO_TEXTURE {
            return NO_TEXTURE;
        }
        let Some(path) = self.strings.get(tex_str_idx as usize) else {
            return NO_TEXTURE;
        };
        if let Some(i) = self
            .versions
            .get(version_idx)
            .and_then(|v| v.texture_by_path.get(path))
        {
            return *i;
        }
        // 叠加包：按挂载顺序查找，先挂上的优先
        for ov in self.overlays_of(version_idx) {
            if let Some(i) = self.versions[ov].texture_by_path.get(path) {
                return *i;
            }
        }
        // 全局回退：某个贴图只被其他版本登记过时仍可命中
        for v in &self.versions {
            if let Some(i) = v.texture_by_path.get(path) {
                return *i;
            }
        }
        NO_TEXTURE
    }

    /// 挂载在某个基础版本上的叠加包下标，按挂载顺序（先挂的在前）。
    ///
    /// 叠加包挂到 dataVersion 最接近的基础版本上。这个关系在载入后确定，
    /// 因为它取决于「库中实际存在哪些基础版本」，而单个包文件里没有
    /// 这个信息。每次调用重新计算：库是只读的，且版本数不大。
    pub fn overlays_of(&self, version_idx: usize) -> Vec<usize> {
        let Some(base) = self.versions.get(version_idx) else {
            return Vec::new();
        };
        if base.is_overlay {
            // 叠加包自身不承载其他叠加包
            return Vec::new();
        }
        self.versions
            .iter()
            .enumerate()
            .filter(|(_, v)| v.is_overlay)
            .filter(|(_, v)| self.nearest_base(v.data_version) == Some(version_idx))
            .map(|(i, _)| i)
            .collect()
    }

    /// dataVersion 最接近的基础版本下标（不含叠加包）
    fn nearest_base(&self, data_version: i32) -> Option<usize> {
        self.versions
            .iter()
            .enumerate()
            .filter(|(_, v)| !v.is_overlay)
            .min_by_key(|(_, v)| (v.data_version as i64 - data_version as i64).abs())
            .map(|(i, _)| i)
    }

    /// 按 dataVersion 选最接近的版本（下标）。
    ///
    /// 差值按 i64 计算：两个 i32 相减可能溢出，极端取值下
    /// （dataVersion 经截断后接近 i32 边界）会让距离算成负数或 panic。
    /// legacy-cpp 同样显式提升为 64 位（blockdb.cpp:366）。
    ///
    /// 叠加包不参与选择（与 legacy-cpp 的 PickVersion 一致）：它们不是
    /// 某个游戏版本的定义，而是叠加在某个基础版本之上的一层。
    pub fn pick_version(&self, data_version: i32) -> Option<usize> {
        self.nearest_base(data_version)
    }

    /// 把另一个方块库并入自身。
    ///
    /// 用途：用户导入的资源包是独立的 pack 文件，要合并进基础库后
    /// 才能参与检索与渲染。
    ///
    /// 合并必须平移所有池下标：字符串、语言表、贴图、模型、像素缓冲
    /// 各自被追加，而**已有的下标是相对各自池的**。不平移会让叠加包的
    /// 方块指向基础库里毫不相干的模型与贴图——不崩溃，只是画面全错。
    ///
    /// 各层的平移量：
    ///   - 模型面里的 `texture`（字符串表下标） += 字符串池原长度
    ///   - 版本里 `texture_by_path` 的值（贴图池下标） += 贴图池原长度
    ///   - 版本的 `lang_idx`（语言表下标） += 语言表原长度
    ///   - 变体的 `model_idx`（模型池下标） += 模型池原长度
    ///   - 贴图记录的 `offset`（像素缓冲偏移） += 像素缓冲原长度
    pub fn merge_from(&mut self, other: &BlockDb, is_overlay: bool) -> AppResult<()> {
        if other.versions.is_empty() {
            return Err(AppError::Parse("资源包里没有版本数据".into()));
        }
        let str_base = self.strings.len() as u32;
        let lang_base = self.langs.len() as u32;
        let tex_base = self.textures.len() as u32;
        let model_base = self.models.len() as u32;
        let pix_base = self.pixels.len() as u32;

        // 字符串池：语言表与模型面都引用它
        self.strings.extend(other.strings.iter().cloned());

        // 语言表：LangEntry 里存的是字符串本身，直接追加
        self.langs.extend(other.langs.iter().cloned());

        // 贴图池：像素偏移要加上现有缓冲长度
        for t in &other.textures {
            let mut t = t.clone();
            t.offset = t.offset.saturating_add(pix_base);
            self.textures.push(t);
        }
        self.pixels.extend_from_slice(&other.pixels);

        // 模型池：面里的贴图是字符串表下标，必须平移
        for m in &other.models {
            let mut m = m.clone();
            for e in &mut m.elements {
                for f in &mut e.faces {
                    if f.texture != NO_TEXTURE {
                        f.texture = f.texture.saturating_add(str_base);
                    }
                }
            }
            self.models.push(m);
        }

        // 版本
        for v in &other.versions {
            let mut v = v.clone();
            // 语言表要重新取出：读取端就是按 lang_idx 从语言表池里取的，
            // 只平移下标而不重挂，display_name 会拿不到中文名。
            //
            // 必须用**平移前**的原始下标去 other.langs 里取，不能先加法再
            // 反向减回来：那样一旦 lang_idx 极大，saturating_add 会饱和到
            // u32::MAX，减回去得到的就不是原值，取到的表也就错了。
            let local_idx = v.lang_idx as usize;
            v.langs = other.langs.get(local_idx).cloned().ok_or_else(|| {
                // 越界说明包里的 lang_idx 与语言表数量对不上。这里报错而不是
                // `unwrap_or_default()`：静默给一张空表会让该版本的方块全都
                // 显示不出中文名，而用户看不出是数据坏了还是本来就没有名字。
                AppError::Corrupt(format!(
                    "版本 {} 的语言表下标 {local_idx} 超出该包的语言表数量 {}",
                    v.name,
                    other.langs.len()
                ))
            })?;
            // 加上基数后不会溢出：两侧都受 MAX_LANGS（100_000）限制，
            // 上界是 200_000，远小于 u32::MAX。因此这里用普通加法，
            // 不为不可能发生的边界添加保护逻辑。
            v.lang_idx += lang_base;
            v.is_overlay = is_overlay;
            for idx in v.texture_by_path.values_mut() {
                *idx = idx.saturating_add(tex_base);
            }
            for b in &mut v.blocks {
                for var in b.variants.iter_mut().chain(b.multipart.iter_mut()) {
                    var.model_idx = var.model_idx.saturating_add(model_base);
                }
            }
            self.versions.push(v);
        }
        Ok(())
    }

    /// 取某个贴图的像素切片
    #[cfg(test)]
    pub fn texture_pixels(&self, idx: usize) -> Option<&[u8]> {
        let t = self.textures.get(idx)?;
        let start = t.offset as usize;
        self.pixels.get(start..start + t.len as usize)
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

/// 解压 zlib 数据，并校验解压结果恰好等于声明的长度。
///
/// `expect` 来自文件里记录的原始长度，是**不可信输入**：解压展开比同样由
/// 文件决定。若先解压再比对，一个声明很小、实际能展开数 GB 的段会直接把
/// 内存吃光——原先的 `.min(64 MiB)` 只限制预分配容量，不限制增长。
///
/// 这里用 take 限流：最多读 expect + 1 字节，多出的 1 字节用于区分
/// 「恰好符合声明」与「超出声明」。
fn decompress(data: &[u8], expect: usize) -> AppResult<Vec<u8>> {
    use std::io::Read;
    let limit = expect.saturating_add(1) as u64;
    let mut d = flate2::read::ZlibDecoder::new(data).take(limit);
    let mut out = Vec::with_capacity(expect.min(64 * 1024 * 1024));
    d.read_to_end(&mut out)?;
    if out.len() != expect {
        return Err(AppError::Corrupt(format!(
            "解压后长度 {} 与声明的 {} 不符",
            out.len(),
            expect
        )));
    }
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
        build_minimal_pack_full(data_version, 0)
    }

    /// 构造最小 pack，可指定 dataVersion 与版本段的语言表下标。
    ///
    /// 语言表下标做成参数，是为了能构造「越界」的包而不必猜字节偏移——
    /// 手算偏移的测试在布局变动后会静默失效（改错了位置却仍指向某个 u32，
    /// 于是校验的是别的字段）。
    fn build_minimal_pack_full(data_version: i32, lang_idx: u32) -> Vec<u8> {
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
        u32f(&mut w, lang_idx); // langTableIdx
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

    /// 版本段的语言表下标越界时必须报错，而不是静默给空语言表。
    ///
    /// 静默降级时，该版本的所有方块都显示不出中文名，而用户无从判断是
    /// 数据坏了还是本来就没有名字。lang_idx 来自文件，是不可信输入。
    #[test]
    fn out_of_range_lang_index_is_rejected_on_load() {
        // 包内只有 1 张语言表，下标却写 5
        let bad = build_minimal_pack_full(3465, 5);
        let err = BlockDb::load_from_bytes(&bad, false).expect_err("越界下标应报错");
        let msg = err.to_string();
        assert!(
            msg.contains("语言表下标"),
            "错误信息应指出语言表下标问题，实际：{msg}"
        );
        // 同一份数据、下标合法时应能正常解析，确认失败确实来自下标而非别的字段
        let ok = build_minimal_pack_full(3465, 0);
        assert!(
            BlockDb::load_from_bytes(&ok, false).is_ok(),
            "下标合法时不应报错"
        );
    }

    #[test]
    fn parses_minimal_pack() {
        let db = BlockDb::load_from_bytes(&build_minimal_pack(), false).expect("解析失败");
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
        // 断言错误内容而不是只断言「出错了」：只断言 is_err 时，
        // 任何原因导致的失败都会让这条通过（例如改动布局后别处先报错）
        let e = BlockDb::load_from_bytes(&b, false).unwrap_err().to_string();
        assert!(e.contains("魔数"), "应指出魔数不匹配，实际：{e}");
    }

    #[test]
    fn unsupported_format_version_is_rejected() {
        let mut b = assert_baseline_parses();
        b[8..12].copy_from_slice(&99u32.to_le_bytes());
        let e = BlockDb::load_from_bytes(&b, false).unwrap_err().to_string();
        assert!(e.contains("格式版本"), "错误信息应说明版本不支持：{e}");
    }

    /// 截断的方块库必须返回错误，而不是 panic，也不能「加载成功」。
    ///
    /// 原实现是 `let _ = ...`——返回值被丢弃，只验证了「不 panic」；
    /// 若读取端回归成「截断时也返回 Ok」，旧写法照样通过。同文件下方
    /// 已记录过「踩过『因为错误的原因通过』这个坑」，这里把断言补上。
    #[test]
    fn truncated_data_errors_instead_of_panicking() {
        let full = build_minimal_pack();
        for cut in [10usize, 40, 120, 200, full.len() - 1] {
            let cut = cut.min(full.len());
            let r = BlockDb::load_from_bytes(&full[..cut], false);
            assert!(
                r.is_err(),
                "截断到 {cut} 字节（完整 {} 字节）时应报错，实际加载成功",
                full.len()
            );
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
        // 断言是「截断」这一类错误，而不是碰巧因为别的原因失败。
        // 写成「不足 / 长度 / 越界」时这条会失败并暴露出实际措辞是
        // 「字符串缺少终止符，文件已截断」——只断言 is_err 就看不到这一点。
        let e = BlockDb::load_from_bytes(&b, false).unwrap_err().to_string();
        assert!(
            e.contains("截断") || e.contains("终止符") || e.contains("不足"),
            "应指出数据被截断，实际：{e}"
        );
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

    /// 随程序分发的内置方块库必须能正常载入。
    ///
    /// 这是全应用最关键的资源：它一旦损坏、容器不对或与读取端格式不兼容，
    /// 所有投影都会显示不出方块与贴图，而且是在用户那里才暴露。
    /// 此前没有任何测试读这份真实文件——测试都用手工构造的小样本，
    /// 因此「构造出来的能读」并不等于「实际发布的那份能读」。
    ///
    /// 这里同时是校验规则的回归防线：读取端新增了「版本引用的语言表下标
    /// 必须落在语言表池内」，若该规则对真实数据误报，这条测试会立刻失败。
    #[test]
    fn bundled_block_database_loads() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("blocks.dat.z");
        let db = BlockDb::load(path.to_str().unwrap(), false)
            .unwrap_or_else(|e| panic!("内置方块库 {} 无法载入：{e}", path.display()));

        assert!(!db.versions.is_empty(), "应含版本");
        assert!(!db.textures.is_empty(), "应含贴图");
        assert!(!db.models.is_empty(), "应含模型");
        assert!(!db.pixels.is_empty(), "像素池不应为空");
        assert!(!db.langs.is_empty(), "应含语言表池");

        // 每个版本引用的语言表下标都必须落在池内。
        //
        // 注意不能断言「每个版本的 langs 都非空」：语言表可以是**空表**
        // （打包时确实没有该版本的译名），那时下标合法但解析出来就是空的。
        // 运行期新建的包正是这种情况（pack_build 会放一张空表）。
        for (i, v) in db.versions.iter().enumerate() {
            assert!(
                (v.lang_idx as usize) < db.langs.len(),
                "版本 {i}（{}）的语言表下标 {} 超出语言表数量 {}",
                v.name,
                v.lang_idx,
                db.langs.len()
            );
        }

        // 内置库必须真的带译名，否则界面全是英文标识符。
        // 用行为断言而不是「条目数大于某个数」：直接看 display_name 是否
        // 能把方块名翻成别的东西，这才是用户能感知的结果。
        let translated = db.versions.iter().any(|v| {
            v.blocks.iter().any(|b| {
                let shown = v.display_name(&b.name);
                !shown.is_empty() && shown != b.name
            })
        });
        assert!(
            translated,
            "内置方块库应至少能把一个方块显示成译名，否则贴图与方块清单都会显示英文标识符"
        );

        // 方块与贴图的引用必须自洽：这是「能载入」之外的第二层保障
        for (i, v) in db.versions.iter().enumerate() {
            for b in &v.blocks {
                for var in b.variants.iter().chain(b.multipart.iter()) {
                    assert!(
                        (var.model_idx as usize) < db.models.len(),
                        "版本 {i} 的方块 {} 引用了不存在的模型 {}",
                        b.name,
                        var.model_idx
                    );
                }
            }
        }
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

    // ---- 容器格式分派 ----

    /// 内置方块库用 zlib 容器（与 legacy-cpp 的 uncompress 一致），
    /// 而 .litematic 用 gzip。两者容器不同，只按一种解压会让内置方块库
    /// 在发布版整块不可用——这条测试正是为此存在。
    #[test]
    fn decompress_accepts_zlib_container() {
        let payload = b"PMBLKDB\0some payload bytes";
        let packed = crate::nbt::gzip(payload, 6).unwrap();
        // 先把 gzip 解出来，再用 zlib 重新压——本地编码器产出 stored 块，
        // 仍能覆盖「按魔数分派」这一行为本身
        let plain = crate::nbt::gunzip(&packed).unwrap();
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        use std::io::Write;
        z.write_all(&plain).unwrap();
        let zb = z.finish().unwrap();
        assert_eq!(&zb[..1], &[0x78], "zlib 头应以 78 开头");
        assert_eq!(BlockDb::decompress(&zb).unwrap(), payload);
    }

    #[test]
    fn decompress_accepts_gzip_container() {
        let payload = b"litematic-like payload";
        let packed = crate::nbt::gzip(payload, 6).unwrap();
        assert_eq!(&packed[..2], &[0x1f, 0x8b], "gzip 头");
        assert_eq!(BlockDb::decompress(&packed).unwrap(), payload);
    }

    #[test]
    fn decompress_passes_through_uncompressed() {
        // 既非 gzip 也非 zlib：按未压缩处理，交给后续解析给出明确错误
        let raw = b"PMBLKDB\0";
        assert_eq!(BlockDb::decompress(raw).unwrap(), raw);
    }

    #[test]
    fn decompress_does_not_misread_plain_bytes_as_zlib() {
        // 首字节低 4 位恰为 8、但头校验不通过的数据不应被当成 zlib
        let raw = b"\x78\x00not-a-zlib-stream";
        assert_eq!(BlockDb::decompress(raw).unwrap(), raw, "头校验不通过应按未压缩处理");
    }


    // ---- 叠加包合并 ----

    /// 造一个最小可用的库：一个基础版本 + 一张贴图 + 一个模型 + 一个方块
    fn tiny_db(name: &str, dv: i32, block: &str, tex_path: &str, alpha: u8) -> BlockDb {
        let strings = vec!["1.20.1".to_string(), tex_path.to_string()];
        let model = Model {
            translucent: false,
            elements: vec![ModelElement {
                from: [0.0, 0.0, 0.0],
                to: [16.0, 16.0, 16.0],
                has_rotation: false,
                rot_kind: 0,
                origin: [0.0; 3],
                axis: 0,
                angle: 0.0,
                rescale: false,
                rot_vec: [0.0; 3],
                face_mask: 0b1,
                // 贴图字段是**字符串表下标**：这里指向 tex_path
                faces: vec![ModelFace {
                    texture: 1,
                    uv: [0.0, 0.0, 16.0, 16.0],
                    cullface: 0xFF,
                    tintindex: -1,
                    rotation: 0,
                }],
            }],
        };
        let version = VersionDef {
            name: name.into(),
            data_version: dv,
            lang_idx: 0,
            texture_by_path: HashMap::from([(tex_path.to_string(), 0u32)]),
            blocks: vec![BlockDef {
                name: block.into(),
                variants: vec![Variant {
                    when: String::new(),
                    model_idx: 0,
                    rot_x_div90: 0,
                    rot_y_div90: 0,
                    uvlock: false,
                }],
                multipart: Vec::new(),
            }],
            langs: Vec::new(),
            is_overlay: false,
        };
        BlockDb {
            strings,
            langs: vec![vec![LangEntry {
                block_name: block.into(),
                en: "EN".into(),
                zh: "中".into(),
            }]],
            textures: vec![TextureInfo { w: 1, h: 1, offset: 0, len: 4, has_alpha: alpha != 255 }],
            models: vec![model],
            versions: vec![version],
            pixels: vec![10, 20, 30, alpha],
        }
    }

    #[test]
    fn merge_offsets_model_texture_string_index() {
        let mut base = tiny_db("1.20.1", 3465, "minecraft:stone", "block/stone", 255);
        let pack = tiny_db("mymod", 3465, "mymod:thing", "block/thing", 128);
        base.merge_from(&pack, true).unwrap();

        // 叠加包的模型面原本指向自己的字符串表下标 1，
        // 合并后必须指向基础库字符串表里的 "block/thing"
        let m = base.models.last().unwrap();
        let idx = m.elements[0].faces[0].texture as usize;
        assert_eq!(
            base.strings[idx], "block/thing",
            "模型面的贴图字符串下标必须平移，否则指向基础库里无关的字符串"
        );
    }

    /// 语言表**存在但为空**是合法状态，合并不应报错。
    ///
    /// 运行期新建的资源包就是这样：pack_build 会放一张空语言表
    /// （没有译名，但下标有效）。语言表越界校验若把「表是空的」误判成
    /// 「下标越界」，这类包就再也合并不进来。
    #[test]
    fn merge_accepts_empty_but_valid_lang_table() {
        let mut base = tiny_db("1.20.1", 3465, "minecraft:stone", "block/stone", 255);
        let mut pack = tiny_db("mymod", 3465, "mymod:thing", "block/thing", 128);
        // 池里保留一张空表；版本仍指向下标 0
        pack.langs = vec![Vec::new()];
        pack.versions[0].lang_idx = 0;

        base.merge_from(&pack, true)
            .expect("空语言表是合法状态，不应报错");

        let merged = base.versions.last().unwrap();
        assert_eq!(merged.lang_idx as usize, base.langs.len() - 1, "下标应指向并入的那张空表");
        assert!(merged.langs.is_empty(), "空表解析出来就是空的");
    }

    #[test]
    fn merge_offsets_texture_pool_index_and_pixels() {
        let mut base = tiny_db("1.20.1", 3465, "minecraft:stone", "block/stone", 255);
        let pack = tiny_db("mymod", 3465, "mymod:thing", "block/thing", 128);
        base.merge_from(&pack, true).unwrap();

        assert_eq!(base.textures.len(), 2);
        // 叠加包的纹理偏移原本为 0，合并后必须落在自己的像素段上
        let t = &base.textures[1];
        assert_eq!(t.offset, 4, "贴图像素偏移必须加上原缓冲长度");
        assert_eq!(
            base.texture_pixels(1).unwrap(),
            &[10, 20, 30, 128],
            "必须取到叠加包自己的像素，而不是基础库的前 4 字节"
        );
        assert!(t.has_alpha, "alpha 应保留");
    }

    #[test]
    fn merge_offsets_variant_model_index() {
        let mut base = tiny_db("1.20.1", 3465, "minecraft:stone", "block/stone", 255);
        let pack = tiny_db("mymod", 3465, "mymod:thing", "block/thing", 128);
        base.merge_from(&pack, true).unwrap();

        assert_eq!(base.models.len(), 2);
        let ov = base.versions.last().unwrap();
        assert_eq!(
            ov.blocks[0].variants[0].model_idx, 1,
            "变体的模型下标必须平移，否则模组方块会用基础库的模型"
        );
    }

    #[test]
    fn merge_marks_version_as_overlay_and_keeps_lang_index() {
        let mut base = tiny_db("1.20.1", 3465, "minecraft:stone", "block/stone", 255);
        let pack = tiny_db("mymod", 3465, "mymod:thing", "block/thing", 128);
        base.merge_from(&pack, true).unwrap();

        assert!(base.versions[0].is_overlay == false, "基础版本不受影响");
        assert!(base.versions[1].is_overlay, "并入的版本应标记为叠加包");
        assert_eq!(base.langs.len(), 2);
        // 叠加包的语言表下标原本为 0，合并后应指向 1
        assert_eq!(base.versions[1].lang_idx, 1);
        // 版本上的语言表也要挂对
        assert_eq!(base.versions[1].langs[0].zh, "中");
    }

    /// 语言表下标越界时必须报错，而不是静默给一张空表。
    ///
    /// 原先用 `unwrap_or_default()`：下标取不到时该版本会拿到空语言表，
    /// 于是它的方块全都显示不出中文名，而用户无从判断是数据坏了还是
    /// 本来就没有名字。多语言表下标是包里的字段，属于不可信输入。
    #[test]
    fn merge_reports_out_of_range_lang_index() {
        let mut base = tiny_db("1.20.1", 3465, "minecraft:stone", "block/stone", 255);
        let mut pack = tiny_db("mymod", 3465, "mymod:thing", "block/thing", 128);
        // 把下标指到语言表数量之外
        pack.versions[0].lang_idx = pack.langs.len() as u32 + 3;
        let err = base.merge_from(&pack, true).expect_err("越界下标应报错");
        let msg = err.to_string();
        assert!(
            msg.contains("超出") && msg.contains("语言表"),
            "错误信息应说明语言表下标越界，实际：{msg}"
        );
    }

    /// 下标越界与溢出都走同一条报错路径，这里不再单列「溢出」用例。
    ///
    /// 加上基数后的溢出其实**不可达**：lang_idx 与语言表数量都受
    /// MAX_LANGS（100_000）限制，两者之和的上界是 200_000。原先写过一个
    /// 名为「溢出」的测试，但它的输入（u32::MAX）只会触发越界分支，
    /// 断言又是 `超出 || 溢出` 的析取式——名字与实测内容不符，等于没测。
    /// 需要覆盖的是越界分支，见 merge_reports_out_of_range_lang_index。

    #[test]
    fn merged_pack_block_is_findable() {        let mut base = tiny_db("1.20.1", 3465, "minecraft:stone", "block/stone", 255);
        let pack = tiny_db("mymod", 3465, "mymod:thing", "block/thing", 128);
        base.merge_from(&pack, true).unwrap();

        let ov = base.versions.last().unwrap();
        assert!(
            crate::block_model::find_block(&base, ov, "mymod:thing").is_some(),
            "合并后模组方块应能查到"
        );
    }

    #[test]
    fn overlay_attaches_to_nearest_base_version() {
        // 叠加包的 dataVersion 与基础版本不完全相同：应挂到最接近的那个
        let mut base = tiny_db("1.20.1", 3465, "minecraft:stone", "block/stone", 255);
        let mut second = tiny_db("1.21", 3953, "minecraft:deepslate", "block/deepslate", 255);
        second.versions[0].blocks[0].name = "minecraft:deepslate".into();
        base.merge_from(&second, false).unwrap();

        let pack = tiny_db("mymod", 3400, "mymod:thing", "block/thing", 255);
        base.merge_from(&pack, true).unwrap();

        // 3400 离 3465 更近（差 65），离 3953 差 553
        let ov_idx = base.versions.len() - 1;
        let attached = base.overlays_of(0);
        assert!(attached.contains(&ov_idx), "应挂到 1.20.1：{attached:?}");
        assert!(!base.overlays_of(1).contains(&ov_idx), "不应挂到 1.21");
    }

    #[test]
    fn texture_lookup_falls_back_to_overlay() {
        let mut base = tiny_db("1.20.1", 3465, "minecraft:stone", "block/stone", 255);
        let pack = tiny_db("mymod", 3465, "mymod:thing", "block/thing", 128);
        base.merge_from(&pack, true).unwrap();

        // "block/thing" 只登记在叠加包里，基础版本查不到；
        // 查找必须能落到叠加包上，否则模组贴图永远用不上
        let sidx = base
            .strings
            .iter()
            .position(|s| s == "block/thing")
            .expect("字符串表里应有该路径") as u32;
        let idx = base.texture_pool_index(0, sidx);
        assert_ne!(idx, NO_TEXTURE, "应通过叠加包找到贴图");
        assert_eq!(base.texture_pixels(idx as usize).unwrap(), &[10, 20, 30, 128]);
    }

    #[test]
    fn texture_lookup_prefers_own_version_over_overlay() {
        // 同名路径在本版本与叠加包里都有：必须用本版本的
        let mut base = tiny_db("1.20.1", 3465, "minecraft:stone", "block/shared", 255);
        base.pixels = vec![1, 1, 1, 255];
        let pack = tiny_db("mymod", 3465, "mymod:thing", "block/shared", 128);
        base.merge_from(&pack, true).unwrap();

        let sidx = base
            .strings
            .iter()
            .position(|s| s == "block/shared")
            .unwrap() as u32;
        let idx = base.texture_pool_index(0, sidx);
        assert_eq!(idx, 0, "应命中本版本登记的贴图");
        assert_eq!(base.texture_pixels(idx as usize).unwrap(), &[1, 1, 1, 255]);
    }

    #[test]
    fn overlay_does_not_participate_in_version_picking() {
        let mut base = tiny_db("1.20.1", 3465, "minecraft:stone", "block/stone", 255);
        let pack = tiny_db("mymod", 9999, "mymod:thing", "block/thing", 255);
        base.merge_from(&pack, true).unwrap();

        let picked = base.pick_version(9999).expect("应选出一个基础版本");
        assert!(
            !base.versions[picked].is_overlay,
            "叠加包不应参与版本选择（与 legacy-cpp 一致）"
        );
        assert_eq!(picked, 0);
    }

    #[test]
    fn merge_rejects_pack_without_versions() {
        let mut base = tiny_db("1.20.1", 3465, "minecraft:stone", "block/stone", 255);
        let mut empty = tiny_db("x", 1, "x", "x", 255);
        empty.versions.clear();
        let e = base.merge_from(&empty, true).unwrap_err().to_string();
        assert!(e.contains("没有版本数据"), "应说明原因：{e}");
        assert_eq!(base.versions.len(), 1, "失败时不应留下部分合并的结果");
    }

    #[test]
    fn merging_non_overlay_adds_a_base_version() {
        // 版本包（kind=1）并入的是基础版本，应参与版本选择
        let mut base = tiny_db("1.20.1", 3465, "minecraft:stone", "block/stone", 255);
        let pack = tiny_db("1.21", 3953, "minecraft:deepslate", "block/deepslate", 255);
        base.merge_from(&pack, false).unwrap();

        assert!(!base.versions[1].is_overlay, "版本包不应标记为叠加包");
        assert_eq!(base.pick_version(3953), Some(1), "新基础版本应可被选中");
        assert!(base.overlays_of(1).is_empty());
    }

}
