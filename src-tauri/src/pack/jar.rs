//! 以 jar/zip 文件为内容来源，供资源包构建使用。
//!
//! 把 `pack_build::JarSource` 落到真实文件上：打开 zip、读中央目录，
//! 按需解压条目。PNG 贴图也在这里解码成像素供构建使用。
//!
//! 与 pack_build 分开是为了让构建逻辑不依赖文件系统：构建侧只认
//! JarSource 契约，因此可用内存假 jar 完整测试；这里只负责把文件接上。

use crate::error::{AppError, AppResult};
use crate::pack::build::JarSource;
use crate::format::zip::Archive;
use std::collections::HashMap;
use std::path::Path;

/// 一个已打开的 jar 文件。
///
/// 整个文件的字节常驻内存：Java 的 jar 通常几十 MB，而构建过程会反复
/// 读取条目（每个 blockstate 与其父模型链）。逐条 seek 读盘在机械盘上
/// 会明显拖慢导入，一次性读入更简单也更快。
#[derive(Debug)]
pub struct JarFile {
    bytes: Vec<u8>,
    archive: Archive,
    /// 条目名 -> 内容，已读取过的缓存
    cache: HashMap<String, Vec<u8>>,
    /// jar 之外的语言文件：路径 -> 内容
    ///
    /// 官方客户端 jar 只带 `en_us.json`，中文等语言随资源索引下载后存在
    /// `.minecraft/assets/objects` 里，构建基础库时要一并喂进来，
    /// 否则方块名整列没有中文。
    extra_langs: HashMap<String, Vec<u8>>,
}

impl JarFile {
    /// 打开一个 jar/zip 文件
    pub fn open(path: &str) -> AppResult<Self> {
        Self::open_with_langs(path, Vec::new())
    }

    /// 打开一个 jar 并附带 jar 之外的语言文件（路径 -> 内容）
    pub fn open_with_langs(
        path: &str,
        extra_langs: Vec<(String, Vec<u8>)>,
    ) -> AppResult<Self> {
        let bytes = std::fs::read(path).map_err(|e| {
            AppError::Parse(format!("无法读取 {path}：{e}"))
        })?;
        if bytes.len() < 22 {
            return Err(AppError::Parse(format!(
                "{path} 过小，不像是 jar/zip 文件"
            )));
        }
        let archive = crate::format::zip::open(&bytes)?;
        Ok(Self {
            bytes,
            archive,
            cache: HashMap::new(),
            extra_langs: extra_langs.into_iter().collect(),
        })
    }

    /// 条目的原始（未解压）内容
    pub fn raw(&self, name: &str) -> AppResult<Vec<u8>> {
        let e = self
            .archive
            .find(name)
            .ok_or_else(|| AppError::Parse(format!("jar 里没有条目 {name}")))?;
        crate::format::zip::read(&self.bytes, e)
    }

    /// 把 PNG 贴图解码成 (宽, 高, RGBA)
    pub fn decode_texture(&self, path: &str) -> Option<(u16, u16, Vec<u8>)> {
        let entry = self.texture_entry(path)?;
        let bytes = self.raw(&entry).ok()?;
        let img = crate::format::png::decode(&bytes).ok()?;
        let w = u16::try_from(img.width).ok()?;
        let h = u16::try_from(img.height).ok()?;
        if img.rgba.len() != w as usize * h as usize * 4 {
            return None;
        }
        Some((w, h, img.rgba))
    }

    /// 贴图路径（`ns:block/stone` 或 `block/stone`）对应的条目名
    fn texture_entry(&self, path: &str) -> Option<String> {
        let (ns, p) = match path.split_once(':') {
            Some((n, p)) => (n, p),
            // 没有命名空间时按 minecraft 处理：模型里的贴图引用常省略前缀
            None => ("minecraft", path),
        };
        if ns.is_empty() || p.is_empty() {
            return None;
        }
        let exact = format!("assets/{ns}/textures/{p}.png");
        if self.archive.find(&exact).is_some() {
            return Some(exact);
        }
        // 少见的扩展名大小写差异
        let upper = format!("assets/{ns}/textures/{p}.PNG");
        if self.archive.find(&upper).is_some() {
            return Some(upper);
        }
        None
    }

    /// 条目总数
    #[cfg(test)]
    pub fn entry_count(&self) -> usize {
        self.archive.entries.len()
    }
}

impl JarSource for JarFile {
    fn entries(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .archive
            .entries
            .iter()
            .filter(|e| !e.is_dir())
            .map(|e| e.name.clone())
            .collect();
        // 排序使构建结果与 zip 条目顺序无关
        v.sort();
        v
    }

    fn extra_lang_paths(&self) -> Vec<String> {
        let mut v: Vec<String> = self.extra_langs.keys().cloned().collect();
        v.sort();
        v
    }

    fn read_extra_lang(&self, name: &str) -> Option<Vec<u8>> {
        self.extra_langs.get(name).cloned()
    }

    fn read(&self, name: &str) -> AppResult<Vec<u8>> {
        if let Some(v) = self.cache.get(name) {
            return Ok(v.clone());
        }
        self.raw(name)
    }
}

/// 从 jar 文件构建资源包并落盘（不带 jar 之外的语言表）。
///
/// 返回产出信息；`out_path` 已存在时直接覆盖——导入是幂等操作，
/// 用户重复导入同一个 jar 不应产生多份副本。
///
/// 只给测试用：正式流程一律走 [`build_pack_to_with_langs`]，因为客户端 jar
/// 里只有 `en_us.json`，不额外带上中文语言表就会建出只有英文名的库。
#[cfg(test)]
pub fn build_pack_to(
    jar_path: &str,
    out_path: &str,
    pack_name: &str,
    data_version: i32,
    is_overlay: bool,
) -> AppResult<crate::pack::build::BuildStats> {
    build_pack_to_with_langs(
        jar_path,
        out_path,
        pack_name,
        data_version,
        is_overlay,
        Vec::new(),
    )
}

/// 同上，并附带 jar 之外的语言文件（路径 -> 内容）。
///
/// 官方客户端 jar 只带 `en_us.json`，中文在 `.minecraft/assets` 的对象库里；
/// 构建基础库时必须把中文喂进来，否则方块名整列没有中文。
pub fn build_pack_to_with_langs(
    jar_path: &str,
    out_path: &str,
    pack_name: &str,
    data_version: i32,
    is_overlay: bool,
    extra_langs: Vec<(String, Vec<u8>)>,
) -> AppResult<crate::pack::build::BuildStats> {
    let jar = JarFile::open_with_langs(jar_path, extra_langs)?;
    let built = crate::pack::build::build(
        &jar,
        pack_name,
        data_version,
        is_overlay,
        |path| jar.decode_texture(path),
    )?;

    if let Some(parent) = Path::new(out_path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    // 资源包文件按 zlib 压缩（与基础库的单版本包一致），
    // 读取端 BlockDb::load 按魔数分派，两种容器都可读
    let packed = compress_zlib(&built.data)?;
    std::fs::write(out_path, &packed)?;
    Ok(built.stats)
}

/// zlib 压缩
fn compress_zlib(data: &[u8]) -> AppResult<Vec<u8>> {
    use std::io::Write;
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(9));
    e.write_all(data)?;
    Ok(e.finish()?)
}

// 造 zip / PNG 的辅助不只本文件的测试用：基础库扫描、实体贴图打包等
// 测试同样需要造假 jar 与假贴图，经由此处对其它测试模块开放。
#[cfg(test)]
pub(crate) use tests::{make_png, make_zip};

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一个最小 zip（stored，无需压缩）
    pub(crate) fn make_zip(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        let mut central: Vec<u8> = Vec::new();
        let mut count = 0u16;
        for (name, data) in files {
            let crc = crate::format::zip::crc32(data);
            let off = out.len() as u32;
            out.extend_from_slice(&0x04034b50u32.to_le_bytes());
            out.extend_from_slice(&20u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes()); // stored
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(data);

            central.extend_from_slice(&0x02014b50u32.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&crc.to_le_bytes());
            central.extend_from_slice(&(data.len() as u32).to_le_bytes());
            central.extend_from_slice(&(data.len() as u32).to_le_bytes());
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u32.to_le_bytes());
            central.extend_from_slice(&off.to_le_bytes());
            central.extend_from_slice(name.as_bytes());
            count += 1;
        }
        let cd_off = out.len() as u32;
        let cd_size = central.len() as u32;
        out.extend_from_slice(&central);
        out.extend_from_slice(&0x06054b50u32.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&cd_size.to_le_bytes());
        out.extend_from_slice(&cd_off.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    pub(crate) fn make_png(w: u32, h: u32, alpha: u8) -> Vec<u8> {
        fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
            out.extend_from_slice(&(body.len() as u32).to_be_bytes());
            out.extend_from_slice(kind);
            out.extend_from_slice(body);
            let mut c = kind.to_vec();
            c.extend_from_slice(body);
            out.extend_from_slice(&crate::format::zip::crc32(&c).to_be_bytes());
        }
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&w.to_be_bytes());
        ihdr.extend_from_slice(&h.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        chunk(&mut png, b"IHDR", &ihdr);
        let mut raw = Vec::new();
        for y in 0..h {
            raw.push(0);
            for x in 0..w {
                raw.extend_from_slice(&[(x * 40) as u8, (y * 40) as u8, 9, alpha]);
            }
        }
        use std::io::Write;
        let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(&raw).unwrap();
        let comp = e.finish().unwrap();
        chunk(&mut png, b"IDAT", &comp);
        chunk(&mut png, b"IEND", &[]);
        png
    }

    /// 测试用的临时目录。
    ///
    /// 落在 target 目录下而不是系统临时目录：部分受限环境不允许写系统
    /// 临时目录，而这类环境与「代码是否正确」无关，测试不应因此失败。
    /// target 目录由构建过程创建，必然可写。
    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join(format!(
                "jar_{}_{}",
                name,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        std::fs::create_dir_all(&d).expect("应能在 target 下建立测试目录");
        d
    }

    #[test]
    fn opens_and_lists_entries() {
        let zip = make_zip(&[("a.txt", b"hello"), ("assets/x/y.json", b"{}")]);
        let d = tmp("list");
        let p = d.join("t.jar");
        std::fs::write(&p, &zip).unwrap();

        let jar = JarFile::open(p.to_str().unwrap()).unwrap();
        assert_eq!(jar.entry_count(), 2);
        let names = JarSource::entries(&jar);
        assert_eq!(names, vec!["a.txt", "assets/x/y.json"]);
        assert_eq!(jar.read("a.txt").unwrap(), b"hello");
    }

    #[test]
    fn missing_entry_is_reported_with_name() {
        let zip = make_zip(&[("a.txt", b"x")]);
        let d = tmp("missing");
        let p = d.join("t.jar");
        std::fs::write(&p, &zip).unwrap();
        let jar = JarFile::open(p.to_str().unwrap()).unwrap();
        let e = jar.read("nope.txt").unwrap_err().to_string();
        assert!(e.contains("nope.txt"), "错误里应指出缺失的条目名：{e}");
    }

    #[test]
    fn rejects_non_zip_file() {
        let d = tmp("bad");
        let p = d.join("t.jar");
        std::fs::write(&p, b"this is definitely not a zip archive at all").unwrap();
        assert!(JarFile::open(p.to_str().unwrap()).is_err());
    }

    #[test]
    fn rejects_missing_file_with_path_in_message() {
        let e = JarFile::open("C:/definitely/not/here.jar").unwrap_err().to_string();
        assert!(e.contains("here.jar"), "错误里应包含路径：{e}");
    }

    #[test]
    fn decodes_texture_with_explicit_namespace() {
        let png = make_png(2, 2, 255);
        let zip = make_zip(&[("assets/minecraft/textures/block/stone.png", &png)]);
        let d = tmp("tex");
        let p = d.join("t.jar");
        std::fs::write(&p, &zip).unwrap();
        let jar = JarFile::open(p.to_str().unwrap()).unwrap();

        let (w, h, rgba) = jar.decode_texture("minecraft:block/stone").expect("应解出贴图");
        assert_eq!((w, h), (2, 2));
        assert_eq!(rgba.len(), 16);
    }

    #[test]
    fn decodes_texture_without_namespace_as_minecraft() {
        // 模型里的贴图引用常省略命名空间
        let png = make_png(1, 1, 255);
        let zip = make_zip(&[("assets/minecraft/textures/block/x.png", &png)]);
        let d = tmp("texns");
        let p = d.join("t.jar");
        std::fs::write(&p, &zip).unwrap();
        let jar = JarFile::open(p.to_str().unwrap()).unwrap();
        assert!(jar.decode_texture("block/x").is_some(), "无前缀应按 minecraft 处理");
    }

    #[test]
    fn missing_texture_returns_none() {
        let zip = make_zip(&[("a.txt", b"x")]);
        let d = tmp("notex");
        let p = d.join("t.jar");
        std::fs::write(&p, &zip).unwrap();
        let jar = JarFile::open(p.to_str().unwrap()).unwrap();
        assert!(jar.decode_texture("minecraft:block/nope").is_none());
    }

    #[test]
    fn corrupt_png_returns_none_not_panic() {
        let zip = make_zip(&[("assets/minecraft/textures/block/bad.png", b"not a png")]);
        let d = tmp("badpng");
        let p = d.join("t.jar");
        std::fs::write(&p, &zip).unwrap();
        let jar = JarFile::open(p.to_str().unwrap()).unwrap();
        assert!(
            jar.decode_texture("minecraft:block/bad").is_none(),
            "损坏的 PNG 应返回 None 而不是崩溃"
        );
    }

    #[test]
    fn builds_pack_from_real_jar_file_and_writes_it() {
        let png = make_png(2, 2, 255);
        let zip = make_zip(&[
            ("version.json", br##"{"id":"1.20.1","world_version":3465}"##),
            (
                "assets/minecraft/blockstates/stone.json",
                br##"{"variants":{"":{"model":"block/stone"}}}"##,
            ),
            (
                "assets/minecraft/models/block/stone.json",
                br##"{"textures":{"all":"block/stone"},"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":{"up":{"texture":"#all"}}}]}"##,
            ),
            ("assets/minecraft/textures/block/stone.png", &png),
        ]);
        let d = tmp("build");
        let jar_path = d.join("client.jar");
        std::fs::write(&jar_path, &zip).unwrap();
        let out = d.join("test.pack");

        let stats = build_pack_to(
            jar_path.to_str().unwrap(),
            out.to_str().unwrap(),
            "test",
            0,
            false,
        )
        .expect("构建失败");

        assert_eq!(stats.blocks, 1);
        assert_eq!(stats.models, 1);
        assert_eq!(stats.textures, 1, "贴图应从 jar 内解码并登记");
        assert!(out.is_file(), "产出文件应已落盘");

        // 真正读回一次，确认写出的文件可用
        let db = crate::block::db::BlockDb::load(out.to_str().unwrap(), false).expect("读回失败");
        assert_eq!(db.versions.len(), 1);
        assert_eq!(db.versions[0].name, "1.20.1");
        assert_eq!(db.versions[0].blocks[0].name, "minecraft:stone");
        assert_eq!(db.textures.len(), 1);
        assert_eq!(db.models.len(), 1);
    }

    /// 联网验证「补齐语言表后，库里存的确实是中文名」。
    ///
    /// 整条链路走一遍：从资源索引下载 zh_cn → 建包 → 读回 → 取显示名称。
    /// 只测下载或只测建包都不够：前者证明不了名字进了库，后者用的是造出来的
    /// 语言表、证明不了真实语言表能被认出来（键名如 `block.minecraft.stone`
    /// 必须与构建侧的查表方式对得上）。
    ///
    /// 默认不跑（`cargo test -- --ignored` 手动执行）：单元测试不该依赖网络。
    #[test]
    #[ignore = "需要联网，手动运行 cargo test -- --ignored"]
    fn downloaded_lang_file_makes_the_pack_store_chinese_names() {
        let manifest = crate::mc::download::fetch_manifest().expect("应能取到版本清单");
        let req = crate::mc::download::LangDownload {
            version_id: "1.21.1",
            source: crate::mc::download::AssetSource::Official,
            mc_dir: None,
            manifest: Some(&manifest),
            langs: &["zh_cn"],
        };
        let langs = crate::mc::download::download_lang_files(&req).expect("应能下载中文语言表");
        assert_eq!(langs.len(), 1, "应当只下到请求的那一个语言表");

        let png = make_png(2, 2, 255);
        let zip = make_zip(&[
            ("version.json", br##"{"id":"1.21.1","world_version":3955}"##),
            (
                "assets/minecraft/blockstates/stone.json",
                br##"{"variants":{"":{"model":"block/stone"}}}"##,
            ),
            (
                "assets/minecraft/models/block/stone.json",
                br##"{"textures":{"all":"block/stone"},"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":{"up":{"texture":"#all"}}}]}"##,
            ),
            ("assets/minecraft/textures/block/stone.png", &png),
            // 真实客户端 jar 里带的就是这份英文表，中文另外从资源库补
            (
                "assets/minecraft/lang/en_us.json",
                br##"{"block.minecraft.stone":"Stone"}"##,
            ),
        ]);
        let d = tmp("lang_e2e");
        let jar_path = d.join("client.jar");
        std::fs::write(&jar_path, &zip).unwrap();
        let out = d.join("lang.pack");

        build_pack_to_with_langs(
            jar_path.to_str().unwrap(),
            out.to_str().unwrap(),
            "1.21.1",
            3955,
            false,
            langs,
        )
        .expect("构建失败");

        let db = crate::block::db::BlockDb::load(out.to_str().unwrap(), false).expect("读回失败");
        let name = db.versions[0].display_name("minecraft:stone");
        assert_eq!(name, "石头", "建好的库里应当存着中文名，实际是：{name}");

        // 英文那套也要在：界面语言切到英语时靠它
        let en = db.versions[0]
            .langs
            .iter()
            .find(|e| e.block_name == "minecraft:stone")
            .map(|e| e.en.clone())
            .unwrap_or_default();
        assert_eq!(en, "Stone", "英文名同样要留着，实际是：{en}");
    }

    #[test]
    fn rebuild_overwrites_existing_file() {
        // 重复导入同一个 jar 不应产生多份副本
        let mut files: Vec<(&str, &[u8])> = vec![
            (
                "assets/minecraft/blockstates/a.json",
                br##"{"variants":{"":{"model":"block/a"}}}"##,
            ),
            (
                "assets/minecraft/models/block/a.json",
                br##"{"textures":{"all":"block/a"},"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":{"up":{"texture":"#all"}}}]}"##,
            ),
        ];
        let png = make_png(1, 1, 255);
        files.push(("assets/minecraft/textures/block/a.png", &png));
        let zip = make_zip(&files);

        let d = tmp("overwrite");
        let jar_path = d.join("c.jar");
        std::fs::write(&jar_path, &zip).unwrap();
        let out = d.join("o.pack");

        build_pack_to(jar_path.to_str().unwrap(), out.to_str().unwrap(), "x", 0, false).unwrap();
        let first = std::fs::metadata(&out).unwrap().len();
        build_pack_to(jar_path.to_str().unwrap(), out.to_str().unwrap(), "x", 0, false).unwrap();
        let second = std::fs::metadata(&out).unwrap().len();
        assert_eq!(first, second, "重复导入应覆盖为相同内容，而不是追加");
        // 目录里只有一个产出文件
        let n = std::fs::read_dir(&d)
            .unwrap()
            .flatten()
            .filter(|e| e.path().extension().map(|x| x == "pack").unwrap_or(false))
            .count();
        assert_eq!(n, 1);
    }

    #[test]
    fn output_directory_is_created_when_absent() {
        let png = make_png(1, 1, 255);
        let zip = make_zip(&[
            (
                "assets/minecraft/blockstates/a.json",
                br##"{"variants":{"":{"model":"block/a"}}}"##,
            ),
            (
                "assets/minecraft/models/block/a.json",
                br##"{"textures":{"all":"block/a"},"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":{"up":{"texture":"#all"}}}]}"##,
            ),
            ("assets/minecraft/textures/block/a.png", &png),
        ]);
        let d = tmp("mkdir");
        let jar_path = d.join("c.jar");
        std::fs::write(&jar_path, &zip).unwrap();
        // 多级不存在的目录
        let out = d.join("deep").join("nested").join("o.pack");
        build_pack_to(jar_path.to_str().unwrap(), out.to_str().unwrap(), "x", 0, false)
            .expect("应自动建立输出目录");
        assert!(out.is_file());
    }

    #[test]
    fn overlay_flag_and_explicit_version_are_passed_through() {
        let png = make_png(1, 1, 255);
        let zip = make_zip(&[
            ("version.json", br##"{"id":"1.20.1","world_version":3465}"##),
            (
                "assets/minecraft/blockstates/a.json",
                br##"{"variants":{"":{"model":"block/a"}}}"##,
            ),
            (
                "assets/minecraft/models/block/a.json",
                br##"{"textures":{"all":"block/a"},"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":{"up":{"texture":"#all"}}}]}"##,
            ),
            ("assets/minecraft/textures/block/a.png", &png),
        ]);
        let d = tmp("overlay");
        let jar_path = d.join("mod.jar");
        std::fs::write(&jar_path, &zip).unwrap();
        let out = d.join("m.pack");
        build_pack_to(
            jar_path.to_str().unwrap(),
            out.to_str().unwrap(),
            "mymod",
            4444,
            true,
        )
        .unwrap();

        let db = crate::block::db::BlockDb::load(out.to_str().unwrap(), true).unwrap();
        assert_eq!(db.versions[0].data_version, 4444, "模组包应使用指定的数据版本");
        assert!(db.versions[0].is_overlay);
    }

    #[test]
    fn packed_output_is_zlib() {
        // 产物与基础库单版本包用同一种容器，读取端才能按同一路径读
        let png = make_png(1, 1, 255);
        let zip = make_zip(&[
            (
                "assets/minecraft/blockstates/a.json",
                br##"{"variants":{"":{"model":"block/a"}}}"##,
            ),
            (
                "assets/minecraft/models/block/a.json",
                br##"{"textures":{"all":"block/a"},"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":{"up":{"texture":"#all"}}}]}"##,
            ),
            ("assets/minecraft/textures/block/a.png", &png),
        ]);
        let d = tmp("zlib");
        let jar_path = d.join("c.jar");
        std::fs::write(&jar_path, &zip).unwrap();
        let out = d.join("o.pack");
        build_pack_to(jar_path.to_str().unwrap(), out.to_str().unwrap(), "x", 0, false).unwrap();

        let bytes = std::fs::read(&out).unwrap();
        assert_eq!(bytes[0], 0x78, "应以 zlib 头开头（{:02x}）", bytes[0]);
        let hdr = bytes[0] as u16 * 256 + bytes[1] as u16;
        assert_eq!(hdr % 31, 0, "zlib 头校验应通过");
    }
}
