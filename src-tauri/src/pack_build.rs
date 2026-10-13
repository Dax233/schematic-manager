//! 从 Minecraft 客户端/模组 jar 构建内部资源包。
//!
//! 串联各层：jar 索引 -> blockstate 解析 -> 模型继承链 -> 模型转换
//! -> 贴图解码与去重 -> 组装方块库 -> 序列化。
//! 对照 legacy-cpp/packbuild.cpp 的 BuildFromJar 主流程。
//!
//! 不直接接触文件系统：内容通过 `JarSource` 提供，因此可以用内存中的
//! 假 jar 完整测试整条链路，不必依赖真实的客户端 jar。

use crate::blockdb::{BlockDb, BlockDef, LangEntry, Model, TextureInfo, Variant, VersionDef};
use crate::blockstate::parse_blockstate;
use crate::error::{AppError, AppResult};
use crate::jar_index::{self, JarIndex};
use crate::blockstate_chain::{build_chain, Chain};
use crate::pool::{model_key, ContentPool};
use std::collections::HashMap;

/// jar 内容来源
pub trait JarSource {
    fn entries(&self) -> Vec<String>;
    fn read(&self, name: &str) -> AppResult<Vec<u8>>;
}

/// 构建结果统计
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BuildStats {
    pub namespaces: usize,
    pub blocks: usize,
    pub models: usize,
    pub textures: usize,
    /// 解析失败而被跳过的 blockstate 数
    pub failed_blockstates: usize,
    /// 因无法解析模型而被跳过的方块数
    pub failed_blocks: usize,
}

/// 构建产物
#[derive(Debug, Clone, PartialEq)]
pub struct BuiltPack {
    pub data: Vec<u8>,
    pub stats: BuildStats,
}

/// 贴图池的一项
#[derive(Debug, Clone, PartialEq, Eq)]
struct TexEntry {
    w: u16,
    h: u16,
    rgba: Vec<u8>,
    has_alpha: bool,
}

/// 字符串池：保持插入顺序并记录下标
#[derive(Default, Debug)]
struct StringPool {
    items: Vec<String>,
    index: HashMap<String, u32>,
}

impl StringPool {
    fn intern(&mut self, s: &str) -> u32 {
        if let Some(i) = self.index.get(s) {
            return *i;
        }
        let i = self.items.len() as u32;
        self.items.push(s.to_string());
        self.index.insert(s.to_string(), i);
        i
    }
    fn get(&self, s: &str) -> Option<u32> {
        self.index.get(s).copied()
    }
}

/// 从 jar 构建资源包。
///
/// `data_version` 为 0 时尝试从 version.json 读取；模组包应显式给出，
/// 因为「叠加到哪个游戏版本」在 jar 里没有。
pub fn build<S, L>(
    src: &S,
    pack_name: &str,
    data_version: i32,
    is_overlay: bool,
    mut load_texture: L,
) -> AppResult<BuiltPack>
where
    S: JarSource,
    L: FnMut(&str) -> Option<(u16, u16, Vec<u8>)>,
{
    let names = src.entries();
    let index = jar_index::build(&names);
    if index.is_empty() {
        return Err(AppError::Parse(
            "jar 里没有找到任何资源（不是客户端 jar 或模组 jar？）".into(),
        ));
    }

    let (mut version_id, mut dv) = (String::new(), data_version);
    if let Ok(raw) = src.read("version.json") {
        if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&raw) {
            if let Some(id) = v.get("id").and_then(|x| x.as_str()) {
                version_id = id.to_string();
            }
            if dv <= 0 {
                if let Some(n) = v.get("world_version").and_then(|x| x.as_i64()) {
                    // 用 try_from 而不是 `n as i32`：后者静默截断，而截断结果
                    // 可能落在一个**看似合法**的版本号上。例如
                    // 4294970761 as i32 == 3465，正好是 1.20.1 的编号，
                    // 于是损坏的版本号会被当成合法版本，把资源包归到错误的
                    // 游戏版本上，后续的版本匹配与转换目标都会跟着错。
                    // 超出 i32 时按「没有版本号」处理（dv 保持原值），
                    // 与字段缺失一致，界面显示为未知版本。
                    if let Ok(v32) = i32::try_from(n) {
                        dv = v32;
                    }
                }
            }
        }
    }
    if version_id.is_empty() {
        version_id = pack_name.to_string();
    }

    let mut stats = BuildStats::default();
    let mut strings = StringPool::default();
    let mut models: ContentPool<Model> = ContentPool::new();
    let mut textures: ContentPool<TexEntry> = ContentPool::new();
    // 贴图路径 -> 贴图池下标
    let mut tex_by_path: HashMap<String, u32> = HashMap::new();
    let mut blocks: Vec<BlockDef> = Vec::new();

    let namespaces: Vec<String> = index
        .blockstate_namespaces()
        .into_iter()
        .map(|s| s.to_string())
        .collect();
    stats.namespaces = namespaces.len();
    if namespaces.is_empty() {
        return Err(AppError::Parse("jar 里没有找到任何 blockstates".into()));
    }

    for ns in &namespaces {
        let Some(assets) = index.namespaces.get(ns) else { continue };
        let bs_entries: Vec<String> = assets.blockstates.clone();

        for bs_entry in &bs_entries {
            let Ok(raw) = src.read(bs_entry) else {
                stats.failed_blockstates += 1;
                continue;
            };
            let Ok(def) = parse_blockstate(&raw) else {
                stats.failed_blockstates += 1;
                continue;
            };
            if def.is_empty() {
                stats.failed_blockstates += 1;
                continue;
            }
            let Some(block_name) = block_name_of(bs_entry) else {
                stats.failed_blockstates += 1;
                continue;
            };
            let qualified = format!("{ns}:{block_name}");

            // 先把本方块引用到的模型（含父链）全部解析出来。
            // 分两步是因为登记字符串与查表借用冲突：
            // 先得到「模型 -> 已解析的转换结果 + 用到的贴图路径」，
            // 再统一登记，避免边查边写。
            // ---- 两遍处理 ----
            // 第一遍：解析本方块引用到的全部模型（含父链），得到
            // 「模型链 + 转换所需信息」，并把用到的贴图路径登记进字符串池
            // 与贴图池。只有先登记完，第二遍转换才能查到下标。
            let mut resolved: Vec<(bool, crate::blockstate::VariantEntry, Chain, crate::blockstate::ModelDef)> =
                Vec::new();
            for (is_multipart, list) in [(false, &def.variants), (true, &def.multipart)] {
                for v in list {
                    let Some((chain, mdef)) = load_model(src, &index, ns, &v.model) else {
                        continue;
                    };
                    let elements = if chain.elements.is_empty() {
                        &mdef.elements
                    } else {
                        &chain.elements
                    };
                    // 登记该模型用到的一切贴图
                    for path in
                        crate::model_pack::collect_texture_paths(&chain, elements, ns)
                    {
                        intern_texture(&path, src, ns, &mut strings, &mut textures, &mut tex_by_path, &mut load_texture);
                    }
                    resolved.push((is_multipart, v.clone(), chain, mdef));
                }
            }
            if resolved.is_empty() {
                stats.failed_blocks += 1;
                continue;
            }

            // 第二遍：此时路径到字符串下标已完整，可以转换
            let mut variants: Vec<Variant> = Vec::new();
            let mut multipart: Vec<Variant> = Vec::new();
            for (is_multipart, v, chain, mdef) in &resolved {
                let Some(converted) = crate::model_pack::convert_model(
                    mdef,
                    chain,
                    |face_ref| {
                        let path = crate::model_pack::resolve_face_texture_path(chain, face_ref, ns)?;
                        let idx = strings.get(&path)?;
                        Some((idx, path))
                    },
                ) else {
                    continue;
                };
                let mi = models.intern(&model_key(&converted.model), converted.model);
                let entry = Variant {
                    when: v.when.clone(),
                    model_idx: mi as u32,
                    rot_x_div90: (v.rot_x_div90 % 4) as u8,
                    rot_y_div90: (v.rot_y_div90 % 4) as u8,
                    uvlock: v.uvlock,
                };
                if *is_multipart {
                    multipart.push(entry);
                } else {
                    variants.push(entry);
                }
            }
            if variants.is_empty() && multipart.is_empty() {
                stats.failed_blocks += 1;
                continue;
            }
            strings.intern(&qualified);
            blocks.push(BlockDef { name: qualified, variants, multipart });
        }
    }

    if blocks.is_empty() {
        return Err(AppError::Parse(
            "没有解析出任何方块（jar 内的 blockstates 都无法解析？）".into(),
        ));
    }

    stats.blocks = blocks.len();
    stats.models = models.len();
    stats.textures = textures.len();

    // ---- 组装方块库 ----
    let mut tex_infos: Vec<TextureInfo> = Vec::with_capacity(textures.len());
    let mut pixels: Vec<u8> = Vec::new();
    for t in textures.items() {
        tex_infos.push(TextureInfo {
            w: t.w,
            h: t.h,
            offset: pixels.len() as u32,
            len: t.rgba.len() as u32,
            has_alpha: t.has_alpha,
        });
        pixels.extend_from_slice(&t.rgba);
    }

    let version = VersionDef {
        name: version_id,
        data_version: dv,
        lang_idx: 0,
        texture_by_path: tex_by_path,
        blocks,
        langs: Vec::new(),
        is_overlay,
    };


    let db = BlockDb {
        format_version: 3,
        strings: strings.items.clone(),
        langs: vec![Vec::<LangEntry>::new()],
        textures: tex_infos,
        models: models.items().to_vec(),
        versions: vec![version],
        pixels,
    };

    let data = crate::blockdb_write::serialize(&db)?;
    Ok(BuiltPack { data, stats })
}

/// 加载模型链：返回（已合并的继承链, 模型自身定义）
fn load_model<S: JarSource>(
    src: &S,
    index: &JarIndex,
    ns: &str,
    raw_id: &str,
) -> Option<(Chain, crate::blockstate::ModelDef)> {
    let assets = index.namespaces.get(ns)?;
    // 父链的命名空间沿用当前模型的：parent 常写作 "block/cube"，不带前缀
    let mut load = |id: &str| -> Option<crate::blockstate::ModelDef> {
        let entry = assets
            .models
            .get(id)
            .cloned()
            // minecraft 的短名在其他命名空间里也可能被引用，
            // 因此在全部命名空间中回退查找
            .or_else(|| index.namespaces.values().find_map(|a| a.models.get(id).cloned()))?;
        let bytes = src.read(&entry).ok()?;
        crate::blockstate::parse_model(&bytes).ok()
    };
    let chain = build_chain(raw_id, ns, &mut load);
    let def = load(raw_id)?;
    Some((chain, def))
}

/// 登记一张贴图：解码、按内容去重、记录路径到池下标与字符串下标。
///
/// 贴图加载失败时只登记字符串下标（模型面仍需引用该路径），
/// 但不产生贴图记录——这样缺失的贴图在渲染时表现为占位，
/// 而不是让整个模型解析失败。
fn intern_texture<S: JarSource, L>(
    path: &str,
    src: &S,
    ns: &str,
    strings: &mut StringPool,
    textures: &mut ContentPool<TexEntry>,
    tex_by_path: &mut HashMap<String, u32>,
    load_texture: &mut L,
) where
    S: JarSource,
    L: FnMut(&str) -> Option<(u16, u16, Vec<u8>)>,
{
    // 字符串表始终登记：模型面存的是它的下标
    strings.intern(path);
    if tex_by_path.contains_key(path) {
        return;
    }
    // 贴图条目路径：<ns>:<path> -> assets/<ns>/textures/<path>.png
    let entry = texture_entry_of(path, ns).or_else(|| index_texture_entry(src, path));
    let Some(entry) = entry else { return };
    let decoded = src
        .read(&entry)
        .ok()
        .and_then(|bytes| crate::png::decode(&bytes).ok())
        .map(|img| (img.width as u16, img.height as u16, img.rgba));
    // 优先用调用方提供的加载器（测试可注入），否则用 jar 内解码结果
    let data = load_texture(path).or(decoded);
    let Some((w, h, rgba)) = data else { return };
    if w == 0 || h == 0 || rgba.len() != w as usize * h as usize * 4 {
        return; // 尺寸与像素不一致，宁可丢弃也不要写出错数据
    }
    let has_alpha = rgba.chunks_exact(4).any(|p| p[3] != 255);
    let mut key = format!("{w}x{h}:").into_bytes();
    key.extend_from_slice(&rgba);
    let idx = textures.intern(&key, TexEntry { w, h, rgba, has_alpha }) as u32;
    tex_by_path.insert(path.to_string(), idx);
}

/// 贴图路径 -> jar 内条目路径
fn texture_entry_of(path: &str, default_ns: &str) -> Option<String> {
    let (ns, p) = match path.split_once(':') {
        Some((n, p)) => (n, p),
        None => (default_ns, path),
    };
    if ns.is_empty() || p.is_empty() {
        return None;
    }
    Some(format!("assets/{ns}/textures/{p}.png"))
}

/// 在 jar 里查该贴图的条目路径（处理大写扩展名等情况）
fn index_texture_entry<S: JarSource>(src: &S, path: &str) -> Option<String> {
    let lower = format!("assets/{}.png", path.replace(':', "/textures/"));
    let upper = format!("assets/{}.PNG", path.replace(':', "/textures/"));
    src.entries()
        .into_iter()
        .find(|e| *e == lower || *e == upper)
}

/// 从 blockstate 条目名取出方块名（不含命名空间）
fn block_name_of(entry: &str) -> Option<String> {
    let rest = entry.strip_prefix("assets/")?;
    let (_, after) = rest.split_once('/')?;
    let stem = after.strip_prefix("blockstates/")?.strip_suffix(".json")?;
    if stem.is_empty() {
        None
    } else {
        Some(stem.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 内存中的假 jar
    struct FakeJar {
        files: HashMap<String, Vec<u8>>,
    }

    impl FakeJar {
        fn new() -> Self {
            Self { files: HashMap::new() }
        }
        fn add(&mut self, name: &str, content: &str) -> &mut Self {
            self.files.insert(name.to_string(), content.as_bytes().to_vec());
            self
        }
        fn add_bytes(&mut self, name: &str, content: Vec<u8>) -> &mut Self {
            self.files.insert(name.to_string(), content);
            self
        }
    }

    impl JarSource for FakeJar {
        fn entries(&self) -> Vec<String> {
            let mut v: Vec<String> = self.files.keys().cloned().collect();
            v.sort(); // 与真实 zip 一样给出稳定顺序
            v
        }
        fn read(&self, name: &str) -> AppResult<Vec<u8>> {
            self.files
                .get(name)
                .cloned()
                .ok_or_else(|| AppError::Parse(format!("假 jar 里没有 {name}")))
        }
    }

    /// 一个自包含的最小客户端 jar
    fn minimal_jar() -> FakeJar {
        let mut j = FakeJar::new();
        j.add(
            "version.json",
            r##"{"id":"1.20.1","world_version":3465}"##,
        );
        j.add(
            "assets/minecraft/blockstates/stone.json",
            r##"{"variants":{"":{"model":"block/stone"}}}"##,
        );
        j.add(
            "assets/minecraft/models/block/stone.json",
            r##"{"textures":{"all":"block/stone"},"elements":[{
                "from":[0,0,0],"to":[16,16,16],
                "faces":{"up":{"texture":"#all","cullface":"up"}}
            }]}"##,
        );
        // 一张 2x2 的不透明 PNG
        j.add_bytes("assets/minecraft/textures/block/stone.png", tiny_png(false));
        j
    }

    /// 生成一张 NxN 的 8 位 RGBA PNG。
    /// 自行构造而非依赖外部文件：测试要能在任何环境下运行。
    fn tiny_png(with_alpha: bool) -> Vec<u8> {
        fn crc32(data: &[u8]) -> u32 {
            crate::zip::crc32(data)
        }
        fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
            out.extend_from_slice(&(body.len() as u32).to_be_bytes());
            out.extend_from_slice(kind);
            out.extend_from_slice(body);
            let mut crc_in = kind.to_vec();
            crc_in.extend_from_slice(body);
            out.extend_from_slice(&crc32(&crc_in).to_be_bytes());
        }

        let (w, h) = (2u32, 2u32);
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&w.to_be_bytes());
        ihdr.extend_from_slice(&h.to_be_bytes());
        ihdr.push(8); // 位深
        ihdr.push(6); // RGBA
        ihdr.extend_from_slice(&[0, 0, 0]); // 压缩/滤波/隔行
        chunk(&mut png, b"IHDR", &ihdr);

        // 每行：滤波字节 0 + 4 个像素
        let mut raw = Vec::new();
        for y in 0..h {
            raw.push(0);
            for x in 0..w {
                raw.extend_from_slice(&[(x * 100) as u8, (y * 100) as u8, 7, if with_alpha { 128 } else { 255 }]);
            }
        }
        let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        use std::io::Write;
        enc.write_all(&raw).unwrap();
        let comp = enc.finish().unwrap();
        assert!(!comp.is_empty(), "zlib 编码结果不应为空（原始 {len} 字节）", len = raw.len());
        assert!(comp.len() >= 6, "zlib 结果至少应含头与校验（实际 {}）", comp.len());
        chunk(&mut png, b"IDAT", &comp);
        chunk(&mut png, b"IEND", &[]);
        png
    }

    fn no_texture(_p: &str) -> Option<(u16, u16, Vec<u8>)> {
        None
    }


    /// 直接验证贴图登记本身（不经主流程），便于定位失败环节
    #[test]
    fn intern_texture_registers_for_existing_entry() {
        let mut jar = FakeJar::new();
        jar.add_bytes("assets/minecraft/textures/block/stone.png", tiny_png(false));
        let mut strings = StringPool::default();
        let mut textures: ContentPool<TexEntry> = ContentPool::new();
        let mut by_path = HashMap::new();
        intern_texture(
            "minecraft:block/stone",
            &jar,
            "minecraft",
            &mut strings,
            &mut textures,
            &mut by_path,
            &mut |_p: &str| None,
        );
        assert_eq!(textures.len(), 1, "应从 jar 内解码并登记");
        assert!(by_path.contains_key("minecraft:block/stone"));
        assert!(strings.get("minecraft:block/stone").is_some());
    }

    /// 注入的加载器应优先于 jar 内解码
    #[test]
    fn intern_texture_uses_injected_loader() {
        let jar = FakeJar::new(); // jar 里什么都没有
        let mut strings = StringPool::default();
        let mut textures: ContentPool<TexEntry> = ContentPool::new();
        let mut by_path = HashMap::new();
        intern_texture(
            "minecraft:block/x",
            &jar,
            "minecraft",
            &mut strings,
            &mut textures,
            &mut by_path,
            &mut |_p: &str| Some((1u16, 1u16, vec![1u8, 2, 3, 4])),
        );
        assert_eq!(textures.len(), 1, "注入的加载器应被采用");
    }


    /// tiny_png 生成的字节必须能被 png::decode 解出。
    /// 若这条失败，说明是测试助手构造的 PNG 有问题，而不是产线代码。
    #[test]
    fn tiny_png_is_decodable() {
        let bytes = tiny_png(false);
        eprintln!("[诊断] PNG 总长 {}，前 40 字节 {:02x?}", bytes.len(), &bytes[..40.min(bytes.len())]);
        let img = crate::png::decode(&bytes).expect("自造的 PNG 应能解码");
        assert_eq!((img.width, img.height), (2, 2));
        assert_eq!(img.rgba.len(), 16);
        // 像素值应与构造时一致
        assert_eq!(&img.rgba[0..4], &[0, 0, 7, 255]);
    }

    #[test]
    fn builds_pack_from_minimal_jar() {
        let jar = minimal_jar();
        let built = build(&jar, "test", 0, false, no_texture).expect("构建失败");
        assert_eq!(built.stats.namespaces, 1);
        assert_eq!(built.stats.blocks, 1);
        assert_eq!(built.stats.models, 1);
        assert_eq!(built.stats.failed_blocks, 0);
        assert!(!built.data.is_empty());
    }

    #[test]
    fn built_pack_is_readable_and_preserves_blocks() {
        // 端到端：构建出的字节必须能被读取器解析，且方块与模型对上
        let jar = minimal_jar();
        let built = build(&jar, "test", 0, false, no_texture).unwrap();
        let db = BlockDb::load_from_bytes(&built.data, false).expect("读取失败");

        assert_eq!(db.versions.len(), 1);
        let v = &db.versions[0];
        assert_eq!(v.name, "1.20.1", "版本名应取自 version.json");
        assert_eq!(v.data_version, 3465);
        assert_eq!(v.blocks.len(), 1);
        assert_eq!(v.blocks[0].name, "minecraft:stone");
        assert_eq!(v.blocks[0].variants.len(), 1);
        assert_eq!(db.models.len(), 1);
        assert_eq!(db.models[0].elements.len(), 1);
        // 面应指向「字符串表下标」，且该下标处的字符串是贴图路径
        let face = &db.models[0].elements[0].faces[0];
        let path = &db.strings[face.texture as usize];
        assert_eq!(path, "minecraft:block/stone");
    }

    #[test]
    fn explicit_data_version_overrides_version_json() {
        // 模组包：叠加到哪个版本由调用方指定，jar 里没有该信息
        let jar = minimal_jar();
        let built = build(&jar, "mod", 9999, true, no_texture).unwrap();
        let db = BlockDb::load_from_bytes(&built.data, true).unwrap();
        assert_eq!(db.versions[0].data_version, 9999, "显式给出的版本号应优先生效");
        assert!(db.versions[0].is_overlay, "叠加包标志应透传");
    }

    /// version.json 里的 world_version 超出 i32 时，
    /// 必须按「没有版本号」处理，不能截断成一个看着合法的值。
    ///
    /// 4294970761 是刻意挑的：`4294970761 as i32 == 3465`，正好是 1.20.1 的
    /// 编号。用 `as i32` 时这个损坏的版本号会被当成合法版本，把资源包
    /// 归到错误的游戏版本上。
    #[test]
    fn out_of_range_world_version_is_treated_as_unknown() {
        let mut jar = minimal_jar();
        jar.add("version.json", r##"{"id":"1.20.1","world_version":4294970761}"##);
        let built = build(&jar, "test", 0, false, no_texture).unwrap();
        let db = BlockDb::load_from_bytes(&built.data, false).unwrap();

        assert_eq!(
            db.versions[0].data_version, 0,
            "超出 i32 的 world_version 应视为未知，而不是截断成 3465"
        );
        // 版本名仍照常取自 id
        assert_eq!(db.versions[0].name, "1.20.1");
    }

    /// 负数 world_version 应原样保留，不能被改写成某个看着合法的正数。
    ///
    /// 本项目里 dataVersion 为负表示「打包时缺少版本号」
    /// （见 blockdb.rs 的说明与 legacy-cpp/tools/pack_blockdb.py），
    /// 因此负值属于既定的「未知」表示，不应被替换成猜测值。
    #[test]
    fn negative_world_version_stays_negative() {
        let mut jar = minimal_jar();
        jar.add("version.json", r##"{"id":"1.20.1","world_version":-5}"##);
        let built = build(&jar, "test", 0, false, no_texture).unwrap();
        let db = BlockDb::load_from_bytes(&built.data, false).unwrap();
        assert_eq!(
            db.versions[0].data_version, -5,
            "负值表示「未知」，应原样保留而不是改写成合法版本号"
        );
        assert_eq!(db.versions[0].name, "1.20.1");
    }

    /// 临界值 i32::MAX 应被接受，确认判据不是「一律拒绝大数」。
    #[test]
    fn world_version_at_i32_max_is_accepted() {
        let mut jar = minimal_jar();
        let max = i32::MAX;
        jar.add(
            "version.json",
            &format!(r##"{{"id":"1.20.1","world_version":{max}}}"##),
        );
        let built = build(&jar, "test", 0, false, no_texture).unwrap();
        let db = BlockDb::load_from_bytes(&built.data, false).unwrap();
        assert_eq!(db.versions[0].data_version, max);
    }

    #[test]
    fn texture_is_decoded_and_registered() {
        let jar = minimal_jar();
        let built = build(&jar, "test", 0, false, no_texture).unwrap();
        assert_eq!(built.stats.textures, 1, "应登记一张贴图");

        let db = BlockDb::load_from_bytes(&built.data, false).unwrap();
        assert_eq!(db.textures.len(), 1);
        let t = &db.textures[0];
        assert_eq!((t.w, t.h), (2, 2));
        assert_eq!(t.len, 16, "2x2 RGBA 应为 16 字节");
        // 像素池里应有该贴图的数据
        assert_eq!(db.pixels.len(), 16);
        // 路径映射应指向该贴图
        assert_eq!(
            db.versions[0].texture_by_path.get("minecraft:block/stone"),
            Some(&0)
        );
    }

    #[test]
    fn identical_textures_are_deduplicated() {
        // 两个方块引用内容相同的贴图：应只登记一张
        let mut jar = minimal_jar();
        jar.add(
            "assets/minecraft/blockstates/copy.json",
            r##"{"variants":{"":{"model":"block/copy"}}}"##,
        );
        // copy 的模型引用另一条路径，但 PNG 内容完全相同
        jar.add(
            "assets/minecraft/models/block/copy.json",
            r##"{"textures":{"all":"block/copy"},"elements":[{
                "from":[0,0,0],"to":[16,16,16],
                "faces":{"up":{"texture":"#all"}}
            }]}"##,
        );
        jar.add_bytes("assets/minecraft/textures/block/copy.png", tiny_png(false));

        let built = build(&jar, "test", 0, false, no_texture).unwrap();
        assert_eq!(built.stats.blocks, 2);
        assert_eq!(
            built.stats.textures, 1,
            "内容相同的两张贴图应去重为一张"
        );
    }

    #[test]
    fn identical_models_are_deduplicated() {
        // 两个方块的模型内容完全相同：应只登记一个模型
        let mut jar = minimal_jar();
        jar.add(
            "assets/minecraft/blockstates/twin.json",
            r##"{"variants":{"":{"model":"block/stone"}}}"##,
        );
        let built = build(&jar, "test", 0, false, no_texture).unwrap();
        assert_eq!(built.stats.blocks, 2);
        assert_eq!(built.stats.models, 1, "内容相同的模型应去重");
    }

    #[test]
    fn parent_chain_is_resolved() {
        // 子模型只声明 parent 与 textures，elements 继承自父模型
        let mut jar = minimal_jar();
        jar.add(
            "assets/minecraft/models/block/base.json",
            r##"{"textures":{"all":"block/stone"},"elements":[{
                "from":[0,0,0],"to":[16,16,16],
                "faces":{"up":{"texture":"#all"}}
            }]}"##,
        );
        jar.add(
            "assets/minecraft/blockstates/child.json",
            r##"{"variants":{"":{"model":"block/child"}}}"##,
        );
        jar.add(
            "assets/minecraft/models/block/child.json",
            r##"{"parent":"block/base"}"##,
        );
        let built = build(&jar, "test", 0, false, no_texture).unwrap();
        let db = BlockDb::load_from_bytes(&built.data, false).unwrap();
        // child 应通过 parent 拿到 elements
        let child = db
            .versions[0]
            .blocks
            .iter()
            .find(|b| b.name == "minecraft:child")
            .expect("应有 child 方块");
        let mi = child.variants[0].model_idx as usize;
        assert_eq!(
            db.models[mi].elements.len(),
            1,
            "父模型的 elements 应被继承"
        );
    }

    #[test]
    fn multipart_is_preserved() {
        let mut jar = minimal_jar();
        jar.add(
            "assets/minecraft/blockstates/fence.json",
            r##"{"multipart":[
                {"apply":{"model":"block/stone"}},
                {"when":{"north":"true"},"apply":{"model":"block/stone"}}
            ]}"##,
        );
        let built = build(&jar, "test", 0, false, no_texture).unwrap();
        let db = BlockDb::load_from_bytes(&built.data, false).unwrap();
        let fence = db
            .versions[0]
            .blocks
            .iter()
            .find(|b| b.name == "minecraft:fence")
            .expect("应有 fence 方块");
        assert_eq!(fence.multipart.len(), 2);
        assert_eq!(fence.multipart[0].when, "");
        assert_eq!(fence.multipart[1].when, "north=true");
    }

    #[test]
    fn jar_without_assets_is_rejected() {
        let mut jar = FakeJar::new();
        jar.add("META-INF/MANIFEST.MF", "x");
        let e = build(&jar, "test", 0, false, no_texture).unwrap_err().to_string();
        assert!(e.contains("没有找到任何资源"), "应说明 jar 内容不符：{e}");
    }

    #[test]
    fn jar_with_assets_but_no_blockstates_is_rejected() {
        let mut jar = FakeJar::new();
        jar.add("assets/minecraft/textures/block/x.png", "not really png");
        let e = build(&jar, "test", 0, false, no_texture).unwrap_err().to_string();
        assert!(e.contains("blockstates"), "应说明缺少 blockstates：{e}");
    }

    #[test]
    fn broken_blockstates_are_counted_not_fatal() {
        // 一个能解析、一个坏掉：坏掉的计入统计，整体仍然成功
        let mut jar = minimal_jar();
        jar.add("assets/minecraft/blockstates/broken.json", "{ not json");
        let built = build(&jar, "test", 0, false, no_texture).unwrap();
        assert_eq!(built.stats.failed_blockstates, 1);
        assert_eq!(built.stats.blocks, 1, "好的那个仍应产出");
    }

    #[test]
    fn all_blockstates_broken_is_an_error() {
        let mut jar = FakeJar::new();
        jar.add("assets/minecraft/blockstates/broken.json", "{ not json");
        let e = build(&jar, "test", 0, false, no_texture).unwrap_err().to_string();
        assert!(e.contains("没有解析出任何方块"), "应说明无方块产出：{e}");
    }

    #[test]
    fn missing_model_marks_block_failed() {
        // blockstate 引用了不存在的模型
        let mut jar = FakeJar::new();
        jar.add(
            "assets/minecraft/blockstates/ghost.json",
            r##"{"variants":{"":{"model":"block/ghost"}}}"##,
        );
        let e = build(&jar, "test", 0, false, no_texture).unwrap_err().to_string();
        assert!(e.contains("没有解析出任何方块"), "模型缺失应导致该方块被跳过：{e}");
    }

    #[test]
    fn injected_texture_loader_is_used() {
        // 调用方提供的加载器应被采用；这里给一张 1x1 的红色贴图
        let mut jar = minimal_jar();
        // 删掉 jar 内的贴图，只靠注入的加载器
        jar.files.remove("assets/minecraft/textures/block/stone.png");
        let built = build(&jar, "test", 0, false, |_p: &str| {
            Some((1u16, 1u16, vec![255u8, 0, 0, 255]))
        })
        .unwrap();
        assert_eq!(built.stats.textures, 1);
        let db = BlockDb::load_from_bytes(&built.data, false).unwrap();
        assert_eq!((db.textures[0].w, db.textures[0].h), (1, 1));
        assert_eq!(db.pixels, vec![255, 0, 0, 255]);
    }

    #[test]
    fn missing_texture_does_not_fail_the_model() {
        // 贴图缺失时仍应产出模型（面会指向一个无贴图的下标），
        // 只是贴图池里没有对应记录
        let mut jar = minimal_jar();
        jar.files.remove("assets/minecraft/textures/block/stone.png");
        let built = build(&jar, "test", 0, false, no_texture).unwrap();
        assert_eq!(built.stats.textures, 0, "没有贴图数据时不应登记贴图");
        assert_eq!(built.stats.blocks, 1, "方块仍应产出");
        let db = BlockDb::load_from_bytes(&built.data, false).unwrap();
        assert!(db.textures.is_empty());
        assert_eq!(db.models[0].elements.len(), 1);
    }

    #[test]
    fn block_name_comes_from_filename() {
        assert_eq!(
            block_name_of("assets/minecraft/blockstates/oak_stairs.json").unwrap(),
            "oak_stairs"
        );
        assert_eq!(
            block_name_of("assets/mymod/blockstates/deep/x.json").unwrap(),
            "deep/x"
        );
        assert!(block_name_of("assets/minecraft/models/block/x.json").is_none());
        assert!(block_name_of("assets/minecraft/blockstates/.json").is_none());
    }

    #[test]
    fn multiple_namespaces_are_merged() {
        let mut jar = minimal_jar();
        jar.add(
            "assets/mymod/blockstates/thing.json",
            r##"{"variants":{"":{"model":"mymod:block/thing"}}}"##,
        );
        jar.add(
            "assets/mymod/models/block/thing.json",
            r##"{"textures":{"all":"minecraft:block/stone"},"elements":[{
                "from":[0,0,0],"to":[8,8,8],
                "faces":{"up":{"texture":"#all"}}
            }]}"##,
        );
        let built = build(&jar, "test", 0, false, no_texture).unwrap();
        assert_eq!(built.stats.namespaces, 2);
        assert_eq!(built.stats.blocks, 2, "两个命名空间的方块应合并到一个版本里");
        let db = BlockDb::load_from_bytes(&built.data, false).unwrap();
        let names: Vec<&str> = db.versions[0].blocks.iter().map(|b| b.name.as_str()).collect();
        assert!(names.contains(&"minecraft:stone"));
        assert!(names.contains(&"mymod:thing"));
    }

    #[test]
    fn deterministic_output_for_same_jar() {
        let a = build(&minimal_jar(), "test", 0, false, no_texture).unwrap();
        let b = build(&minimal_jar(), "test", 0, false, no_texture).unwrap();
        assert_eq!(a.data, b.data, "同一 jar 两次构建应产出完全相同的字节");
    }
}
