//! 基础版本库：方块与实体贴图的底座数据，以及本机 Minecraft 版本的发现。
//!
//! 方块库与实体贴图库不随安装包分发，而是由用户选择本机已装的客户端
//! jar 或在线下载，逐版本构建成单版本包，存放在应用数据目录的 `base/`
//! 下。本模块负责三件事：给出「支持补齐哪些正式版」的清单、把磁盘上的
//! 单版本包合成一个基础方块库、扫描 .minecraft 找出已装版本。

use crate::blockdb::BlockDb;
use crate::error::{AppError, AppResult};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// 支持补齐的正式版：(数据版本, 版本号)，按数据版本严格升序。
///
/// 清单取自 legacy-cpp/data/pack_report.json 的构建结果，即历史上实际
/// 能完整构建方块库的正式版集合。不能从 mcver::DATA_VERSIONS 反推：
/// 快照也占数据版本号，一个数据版本可能对应多个版本号，且 1.20.3/
/// 1.20.5 与 26.x 段的正式版在那里没有可区分的条目。
pub const SUPPORTED_RELEASES: &[(i32, &str)] = &[
    (1631, "1.13.2"),
    (1952, "1.14"),
    (1957, "1.14.1"),
    (1963, "1.14.2"),
    (1968, "1.14.3"),
    (1976, "1.14.4"),
    (2225, "1.15"),
    (2227, "1.15.1"),
    (2230, "1.15.2"),
    (2566, "1.16"),
    (2567, "1.16.1"),
    (2578, "1.16.2"),
    (2580, "1.16.3"),
    (2584, "1.16.4"),
    (2586, "1.16.5"),
    (2724, "1.17"),
    (2730, "1.17.1"),
    (2860, "1.18"),
    (2865, "1.18.1"),
    (2975, "1.18.2"),
    (3105, "1.19"),
    (3117, "1.19.1"),
    (3120, "1.19.2"),
    (3218, "1.19.3"),
    (3337, "1.19.4"),
    (3463, "1.20"),
    (3465, "1.20.1"),
    (3578, "1.20.2"),
    (3698, "1.20.3"),
    (3700, "1.20.4"),
    (3837, "1.20.5"),
    (3839, "1.20.6"),
    (3953, "1.21"),
    (3955, "1.21.1"),
    (4080, "1.21.2"),
    (4082, "1.21.3"),
    (4189, "1.21.4"),
    (4325, "1.21.5"),
    (4435, "1.21.6"),
    (4438, "1.21.7"),
    (4440, "1.21.8"),
    (4554, "1.21.9"),
    (4556, "1.21.10"),
    (4671, "1.21.11"),
    (4786, "26.1"),
    (4788, "26.1.1"),
    (4790, "26.1.2"),
    (4903, "26.2"),
    (5023, "26.3"),
];

/// 基础库的存放根目录（应用数据目录下的 `base/`）。
///
/// 与界面状态同样放系统应用数据目录而不是程序目录：程序可能装在
/// Program Files 这类只读位置，而基础库是运行时才下载/构建出来的，
/// 体积可达数百 MB，必须落在可写且有足够空间的位置。
pub fn base_dir(app: &tauri::AppHandle) -> AppResult<PathBuf> {
    use tauri::Manager;
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Parse(format!("无法定位应用数据目录：{e}")))?;
    Ok(dir.join("base"))
}

/// 单版本方块包的存放目录。
pub fn base_blocks_dir(app: &tauri::AppHandle) -> AppResult<PathBuf> {
    Ok(base_dir(app)?.join("blocks"))
}

/// 实体贴图库文件的位置。
pub fn base_entities_path(app: &tauri::AppHandle) -> AppResult<PathBuf> {
    Ok(base_dir(app)?.join("entities.dat.z"))
}

/// 已下载客户端 jar 的缓存目录。
///
/// 下载落盘走「写临时文件再改名」，所以目录里出现 `<版本>.jar` 就一定是
/// 完整文件，下次补同版本时直接复用，不必重新下载几十 MB。
pub fn base_jars_dir(app: &tauri::AppHandle) -> AppResult<PathBuf> {
    Ok(base_dir(app)?.join("jars"))
}

/// 把目录里的单版本包合成一个基础方块库。
///
/// 目录里每个 `<数据版本>-<版本号>.dat.z` 是一个单版本包（补版本时由
/// 客户端 jar 构建）。合成顺序按文件名里的数据版本升序，保证结果与文件
/// 写入先后无关。目录不存在或没有包时返回空库——首次启动就是这种
/// 情况，由前端引导用户去补版本，而不是报错。
///
/// 单个文件无法解析（文件名不符约定或内容损坏）不中断整库：其余版本
/// 仍应可用，失败项以 (文件名, 原因) 返回供界面显示。
pub fn load_base_block_db(dir: &Path) -> AppResult<(BlockDb, Vec<(String, String)>)> {
    let mut db = BlockDb::empty();
    let mut failures: Vec<(String, String)> = Vec::new();
    let mut packs: Vec<(i32, PathBuf, String)> = Vec::new();

    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        // 目录不存在就是「还没补过版本」的正常状态
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((db, failures)),
        Err(e) => {
            return Err(AppError::Parse(format!(
                "读取基础库目录 {} 失败：{e}",
                dir.display()
            )))
        }
    };
    for entry in rd {
        let entry = entry
            .map_err(|e| AppError::Parse(format!("读取基础库目录 {} 失败：{e}", dir.display())))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        // 构建中的临时文件等不带 .dat.z 后缀，不属于版本包
        if !path.is_file() || !name.ends_with(".dat.z") {
            continue;
        }
        let stem = &name[..name.len() - ".dat.z".len()];
        let Some(dash) = stem.find('-') else {
            failures.push((name, "文件名应为 <数据版本>-<版本号>.dat.z".into()));
            continue;
        };
        match stem[..dash].parse::<i32>() {
            Ok(dv) => packs.push((dv, path, name)),
            Err(_) => failures.push((name, "文件名应以数据版本开头".into())),
        }
    }
    packs.sort_by_key(|(dv, _, _)| *dv);
    for (_, path, name) in packs {
        match BlockDb::load(&path.to_string_lossy(), false) {
            Ok(pack) => {
                if let Err(e) = db.merge_from(&pack, false) {
                    failures.push((name, format!("合并失败：{e}")));
                }
            }
            Err(e) => failures.push((name, format!("载入失败：{e}"))),
        }
    }
    Ok((db, failures))
}

/// 扫描到的本机已装版本。
#[derive(Debug)]
pub struct McVersionHit {
    /// 版本 id，取自 jar 内 version.json 的 `id`（如 `1.20.1`）
    pub id: String,
    /// 从 jar 内 version.json 读到的数据版本
    pub data_version: i32,
    /// 客户端 jar 的完整路径
    pub jar_path: PathBuf,
}

#[derive(Debug)]
pub struct McScan {
    pub versions: Vec<McVersionHit>,
    /// 看着像客户端却读不出版本信息的文件；逐条说明原因，不静默跳过
    pub warnings: Vec<String>,
}

/// 递归走查的深度上限。
///
/// 用户可以选任意目录（甚至整块盘）：没有上限时一次扫描可能走上几十分钟，
/// 界面看起来就是卡死。真实客户端的存放层级很浅（`.minecraft/versions/<id>/`），
/// 六层足以覆盖各家启动器的目录结构。
const SCAN_MAX_DEPTH: usize = 6;

/// 一次扫描最多检查多少个 jar。
///
/// 与深度上限同理：在一个装满模组的目录里逐个解压几千个包会长时间卡住。
const SCAN_MAX_JARS: usize = 2000;

/// 扫描任意目录，递归找出其中所有能识别的 Minecraft 客户端 jar。
///
/// 不要求目录是 .minecraft：整棵子树都会走一遍，逐个 jar 读 `version.json`
/// 判断它是不是客户端，版本 id 与数据版本都取自该文件而不是目录名——jar 可能
/// 被改名、也可能被放在任意位置。
///
/// 没有 `version.json` 的 jar 是模组一类，静默跳过：扫一个 mods 目录时把它们
/// 逐条报成「失败」只会刷屏。有 `version.json` 却解析不出来的才作为失败项上报。
/// 同一个版本在多个位置出现时只保留先遇到的那个（目录项排序后走查，结果稳定）。
///
/// 结果按数据版本升序。
pub fn scan_minecraft(dir: &Path) -> AppResult<McScan> {
    let meta = std::fs::metadata(dir)
        .map_err(|e| AppError::Parse(format!("读取 {} 失败：{e}", dir.display())))?;
    if !meta.is_dir() {
        return Err(AppError::Parse(format!("{} 不是目录", dir.display())));
    }

    let mut scan = McScan { versions: Vec::new(), warnings: Vec::new() };
    let mut seen: HashSet<String> = HashSet::new();
    let mut checked = 0usize;
    let mut truncated = false;
    let mut pending: Vec<(PathBuf, usize)> = vec![(dir.to_path_buf(), 0)];

    while let Some((current, depth)) = pending.pop() {
        let Ok(rd) = std::fs::read_dir(&current) else { continue };
        // 目录项顺序由文件系统决定，先排序再走：同一台机器上结果稳定
        let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        entries.sort();

        for path in entries {
            let Ok(ft) = std::fs::symlink_metadata(&path) else { continue };
            // 符号链接一律不跟：链成环时走查不会终止
            if ft.file_type().is_symlink() {
                continue;
            }
            if ft.is_dir() {
                if depth < SCAN_MAX_DEPTH {
                    pending.push((path, depth + 1));
                } else {
                    truncated = true;
                }
                continue;
            }
            let is_jar = path
                .extension()
                .map(|e| e.eq_ignore_ascii_case("jar"))
                .unwrap_or(false);
            if !is_jar {
                continue;
            }
            if checked >= SCAN_MAX_JARS {
                truncated = true;
                break;
            }
            checked += 1;
            match read_jar_version(&path) {
                Ok(Some((id, data_version))) => {
                    if seen.insert(id.clone()) {
                        scan.versions.push(McVersionHit {
                            id,
                            data_version,
                            jar_path: path,
                        });
                    }
                }
                // 不是客户端（模组、资源包一类），静默跳过
                Ok(None) => {}
                Err(e) => scan.warnings.push(format!("{}：{e}", path.display())),
            }
        }
        if checked >= SCAN_MAX_JARS {
            break;
        }
    }

    if truncated {
        scan.warnings.push(format!(
            "为避免长时间扫描，最多走到第 {SCAN_MAX_DEPTH} 层、检查 {SCAN_MAX_JARS} 个 jar；\
             需要更深处的版本时，直接选那一层目录再扫一次"
        ));
    }
    scan.versions.sort_by_key(|v| v.data_version);
    Ok(scan)
}

/// 从客户端 jar 读出版本 id 与数据版本。
///
/// 返回 `Ok(None)` 表示这个 jar 不是客户端（里面没有 `version.json`）——
/// 那是模组的正常形态，不该当成错误。
fn read_jar_version(jar: &Path) -> AppResult<Option<(String, i32)>> {
    let jf = crate::jar_file::JarFile::open(&jar.to_string_lossy())?;
    let Ok(raw) = jf.raw("version.json") else {
        return Ok(None);
    };
    let v: serde_json::Value = serde_json::from_slice(&raw)
        .map_err(|e| AppError::Parse(format!("version.json 解析失败：{e}")))?;
    let id = v
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::Parse("version.json 里没有 id".into()))?
        .to_string();
    let n = v
        .get("world_version")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| AppError::Parse("version.json 里没有 world_version".into()))?;
    let data_version = i32::try_from(n)
        .map_err(|_| AppError::Parse(format!("world_version {n} 超出范围")))?;
    Ok(Some((id, data_version)))
}

/// 从本机资源库里找出指定版本的语言文件（路径 -> 内容）。
///
/// 客户端 jar 只带 `en_us.json`，其余语言随资源索引下载后存在
/// `.minecraft/assets/objects/<hash 前两位>/<hash>` 里，因此中文要从这里补。
/// 索引文件本身给出「资源路径 -> hash」的映射。
///
/// 目标版本的索引缺失时（例如只装过别的版本、或索引被清理过）退而用任意
/// 一个能查到该语言的索引：语言表只是取名字用的辅助数据，方块名跨版本相当
/// 稳定，比整列没有中文更有用。对应地，查不到就不提供，构建侧会回落到英文。
pub fn find_lang_files(
    mc_dir: &Path,
    version_id: &str,
) -> Vec<(String, Vec<u8>)> {
    const LANGS: [&str; 2] = ["zh_cn", "en_us"];

    let indexes = mc_dir.join("assets").join("indexes");
    let objects = mc_dir.join("assets").join("objects");
    if !indexes.is_dir() || !objects.is_dir() {
        return Vec::new();
    }

    // 先试该版本自己声明的索引，再退到本机任意索引
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(id) = asset_index_id(mc_dir, version_id) {
        candidates.push(indexes.join(format!("{id}.json")));
    }
    if let Ok(rd) = std::fs::read_dir(&indexes) {
        let mut rest: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().map(|x| x == "json").unwrap_or(false))
            .collect();
        rest.sort();
        candidates.extend(rest);
    }

    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    for idx in candidates {
        let Ok(raw) = std::fs::read(&idx) else { continue };
        let Ok(json) = serde_json::from_slice::<serde_json::Value>(&raw) else {
            continue;
        };
        let Some(objs) = json.get("objects").and_then(|v| v.as_object()) else {
            continue;
        };
        for lang in LANGS {
            let path = format!("assets/minecraft/lang/{lang}.json");
            if out.iter().any(|(p, _)| p == &path) {
                continue;
            }
            let Some(hash) = objs
                .get(&format!("minecraft/lang/{lang}.json"))
                .and_then(|v| v.get("hash"))
                .and_then(|v| v.as_str())
            else {
                continue;
            };
            if hash.len() < 2 {
                continue;
            }
            let obj = objects.join(&hash[..2]).join(hash);
            let Ok(bytes) = std::fs::read(&obj) else { continue };
            out.push((path, bytes));
        }
        if out
            .iter()
            .any(|(p, _)| p.ends_with("zh_cn.json"))
        {
            break;
        }
    }
    out
}

/// 读版本 json 里声明的资源索引 id（`assetIndex.id`）。
fn asset_index_id(mc_dir: &Path, version_id: &str) -> Option<String> {
    let path = mc_dir
        .join("versions")
        .join(version_id)
        .join(format!("{version_id}.json"));
    let raw = std::fs::read(path).ok()?;
    let json: serde_json::Value = serde_json::from_slice(&raw).ok()?;
    json.get("assetIndex")?
        .get("id")?
        .as_str()
        .map(|s| s.to_string())
}

/// 官方启动器的默认 .minecraft 位置；环境变量缺失时返回 None。
///
/// 只按平台惯例拼路径，不判断目录是否存在：是否可用由调用方决定——
/// 对话框只把真实存在的目录当默认值预填。
pub fn default_minecraft_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA").map(|v| PathBuf::from(v).join(".minecraft"))
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME").map(|v| {
            PathBuf::from(v)
                .join("Library")
                .join("Application Support")
                .join("minecraft")
        })
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::env::var_os("HOME").map(|v| PathBuf::from(v).join(".minecraft"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::Write;

    /// 测试目录落在 target 下：部分受限环境不允许写系统临时目录
    ///（与 jar_file::tests::tmp 同一考虑），而 target 必然可写。
    fn test_dir(tag: &str) -> PathBuf {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join(format!(
                "base_db_{}_{}",
                tag,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        std::fs::create_dir_all(&d).expect("应能在 target 下建立测试目录");
        d
    }

    /// 造一个最小单版本 pack 文件（zlib 压缩，与真实产物一致）。
    fn write_pack(dir: &Path, name: &str, dv: i32, version_name: &str) {
        let db = BlockDb {
            strings: Vec::new(),
            langs: Vec::new(),
            textures: Vec::new(),
            models: Vec::new(),
            versions: vec![crate::blockdb::VersionDef {
                name: version_name.into(),
                data_version: dv,
                lang_idx: 0,
                texture_by_path: HashMap::new(),
                blocks: Vec::new(),
                langs: Vec::new(),
                is_overlay: false,
            }],
            pixels: Vec::new(),
        };
        let raw = crate::blockdb_write::serialize(&db).expect("序列化失败");
        let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(9));
        e.write_all(&raw).expect("压缩失败");
        let packed = e.finish().expect("压缩失败");
        std::fs::write(dir.join(name), packed).expect("写 pack 失败");
    }

    #[test]
    fn finds_language_files_through_the_asset_index() {
        // 中文不在客户端 jar 里，而在 .minecraft/assets 的对象库中，
        // 靠资源索引查到 hash 才能取到。找不到就会整列没有中文名。
        let mc = test_dir("lang");
        let versions = mc.join("versions").join("1.21.11");
        std::fs::create_dir_all(&versions).unwrap();
        std::fs::write(
            versions.join("1.21.11.json"),
            br#"{"id":"1.21.11","assetIndex":{"id":"29"}}"#,
        )
        .unwrap();

        let indexes = mc.join("assets").join("indexes");
        std::fs::create_dir_all(&indexes).unwrap();
        std::fs::write(
            indexes.join("29.json"),
            br#"{"objects":{"minecraft/lang/zh_cn.json":{"hash":"aabbcc","size":3}}}"#,
        )
        .unwrap();

        let zh = br#"{"block.minecraft.stone":"should-not-be-this"}"#;
        let obj = mc.join("assets").join("objects").join("aa");
        std::fs::create_dir_all(&obj).unwrap();
        std::fs::write(obj.join("aabbcc"), zh).unwrap();

        let got = find_lang_files(&mc, "1.21.11");
        assert_eq!(got.len(), 1, "应找到中文语言文件，实际 {got:?}");
        assert_eq!(got[0].0, "assets/minecraft/lang/zh_cn.json");
        assert_eq!(got[0].1, zh);
    }

    #[test]
    fn language_lookup_falls_back_to_another_index_and_gives_up_quietly() {
        // 目标版本的索引不在本机时（只装过别的版本、索引被清理过），
        // 退到本机任意能查到该语言的索引；一个都查不到就不提供，
        // 让构建侧回落到英文名，而不是报错中断补库
        let mc = test_dir("lang_fallback");
        std::fs::create_dir_all(mc.join("assets").join("indexes")).unwrap();
        std::fs::write(
            mc.join("assets").join("indexes").join("17.json"),
            br#"{"objects":{"minecraft/lang/zh_cn.json":{"hash":"ddeeff"}}}"#,
        )
        .unwrap();
        let obj = mc.join("assets").join("objects").join("dd");
        std::fs::create_dir_all(&obj).unwrap();
        std::fs::write(obj.join("ddeeff"), b"{}").unwrap();

        let got = find_lang_files(&mc, "9.9.9");
        assert_eq!(got.len(), 1, "应退到本机其它索引，实际 {got:?}");

        // 资源库整体不存在时返回空，且不 panic
        let empty = test_dir("lang_missing");
        assert!(find_lang_files(&empty, "1.21.11").is_empty());
    }

    #[test]
    fn scan_finds_versions_and_warns_on_broken_jars() {
        let dir = test_dir("scan");
        let versions = dir.join("versions");

        let good = versions.join("1.20.1");
        std::fs::create_dir_all(&good).unwrap();
        let jar =
            crate::jar_file::make_zip(&[("version.json", br#"{"id":"1.20.1","world_version":3465}"#
                .as_slice())]);
        std::fs::write(good.join("1.20.1.jar"), jar).unwrap();

        let broken = versions.join("1.21.1");
        std::fs::create_dir_all(&broken).unwrap();
        std::fs::write(broken.join("1.21.1.jar"), b"this is not a zip archive").unwrap();

        // 加载器目录没有同名 jar，应被跳过且不留警告
        std::fs::create_dir_all(versions.join("fabric-loader-0.15.0")).unwrap();

        let scan = scan_minecraft(&dir).expect("扫描失败");
        assert_eq!(scan.versions.len(), 1, "应只命中一个有效版本");
        let hit = &scan.versions[0];
        assert_eq!(hit.id, "1.20.1");
        assert_eq!(hit.data_version, 3465);
        assert!(hit.jar_path.is_file());
        assert_eq!(scan.warnings.len(), 1, "坏 jar 应各产生一条警告");
        assert!(
            scan.warnings[0].contains("1.21.1"),
            "警告应指出是哪个版本：{}",
            scan.warnings[0]
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_recurses_into_subdirectories_of_any_directory() {
        // 目录不要求是 .minecraft：任意目录、任意层级里的客户端 jar 都要找到。
        // 版本 id 取自 jar 内的 version.json，而不是文件夹名——jar 可能被改名。
        let dir = test_dir("scan-recursive");
        let nested = dir.join("启动器").join("实例").join("versions").join("随便起的名字");
        std::fs::create_dir_all(&nested).unwrap();
        let jar = crate::jar_file::make_zip(&[(
            "version.json",
            br#"{"id":"1.21.1","world_version":3955}"#.as_slice(),
        )]);
        std::fs::write(nested.join("client.jar"), jar).unwrap();

        // 模组：没有 version.json，应静默跳过而不是报一堆失败
        let mods = dir.join("mods");
        std::fs::create_dir_all(&mods).unwrap();
        let mod_jar = crate::jar_file::make_zip(&[(
            "fabric.mod.json",
            br#"{"id":"sodium"}"#.as_slice(),
        )]);
        std::fs::write(mods.join("sodium-0.5.jar"), mod_jar).unwrap();

        let scan = scan_minecraft(&dir).expect("扫描失败");
        assert_eq!(scan.versions.len(), 1, "应找到深层目录里的客户端 jar");
        assert_eq!(scan.versions[0].id, "1.21.1", "id 应取自 version.json");
        assert_eq!(scan.versions[0].data_version, 3955);
        assert!(scan.warnings.is_empty(), "模组不应产生警告：{:?}", scan.warnings);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_keeps_one_hit_per_version_and_sorts_by_data_version() {
        let dir = test_dir("scan-dedupe");
        for (tag, dv) in [("a", 3955), ("b", 3465)] {
            let d = dir.join(tag);
            std::fs::create_dir_all(&d).unwrap();
            let version = if dv == 3955 { "1.21.1" } else { "1.20.1" };
            let body = format!(r#"{{"id":"{version}","world_version":{dv}}}"#);
            std::fs::write(d.join("client.jar"), crate::jar_file::make_zip(&[(
                "version.json",
                body.as_bytes(),
            )]))
            .unwrap();
        }
        // 同一版本的第二个副本：不该出现两行
        let dup = dir.join("c");
        std::fs::create_dir_all(&dup).unwrap();
        std::fs::write(dup.join("copy.jar"), crate::jar_file::make_zip(&[(
            "version.json",
            br#"{"id":"1.20.1","world_version":3465}"#.as_slice(),
        )]))
        .unwrap();

        let scan = scan_minecraft(&dir).expect("扫描失败");
        assert_eq!(scan.versions.len(), 2, "同一版本只保留一条");
        let dvs: Vec<i32> = scan.versions.iter().map(|v| v.data_version).collect();
        assert_eq!(dvs, [3465, 3955], "结果按数据版本升序");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_rejects_a_missing_or_non_directory() {
        let dir = test_dir("scan-missing");
        let missing = dir.join("并不存在");
        let e = scan_minecraft(&missing).unwrap_err().to_string();
        assert!(e.contains("失败"), "目录不存在应报错：{e}");

        let file = dir.join("a.txt");
        std::fs::write(&file, b"x").unwrap();
        let e = scan_minecraft(&file).unwrap_err().to_string();
        assert!(e.contains("不是目录"), "指向文件时应说明不是目录：{e}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn base_db_merges_packs_by_data_version() {
        let dir = test_dir("merge");
        // 故意乱序写入：合成顺序必须由文件名里的数据版本决定，
        // 而不是文件写入先后
        write_pack(&dir, "3465-1.20.1.dat.z", 3465, "1.20.1");
        write_pack(&dir, "1952-1.14.dat.z", 1952, "1.14");

        let (db, failures) = load_base_block_db(&dir).expect("载入失败");
        assert!(failures.is_empty(), "不应有失败项：{failures:?}");
        let dvs: Vec<i32> = db.versions.iter().map(|v| v.data_version).collect();
        assert_eq!(dvs, [1952, 3465], "版本应按数据版本升序合成");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn base_db_survives_broken_files() {
        let dir = test_dir("broken");
        write_pack(&dir, "1952-1.14.dat.z", 1952, "1.14");
        std::fs::write(dir.join("3465-1.20.1.dat.z"), b"garbage").unwrap();
        std::fs::write(dir.join("随手丢的文件.dat.z"), b"garbage").unwrap();
        // 非 .dat.z 文件（如构建中的临时文件）不应惊动用户
        std::fs::write(dir.join("3700-1.20.4.dat.z.tmp"), b"partial").unwrap();

        let (db, failures) = load_base_block_db(&dir).expect("载入失败");
        assert_eq!(db.versions.len(), 1, "好包应照常载入");
        assert_eq!(db.versions[0].data_version, 1952);
        assert_eq!(failures.len(), 2, "两个坏文件各应有一条失败记录");
        assert!(
            failures.iter().any(|(f, _)| f == "3465-1.20.1.dat.z"),
            "失败项应指出文件名：{failures:?}"
        );
        assert!(
            failures.iter().any(|(f, _)| f == "随手丢的文件.dat.z"),
            "文件名不符约定也应指出：{failures:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_base_dir_is_an_empty_db() {
        let dir = test_dir("missing").join("不存在");
        let (db, failures) = load_base_block_db(&dir).expect("目录不存在不应报错");
        assert!(db.versions.is_empty());
        assert!(failures.is_empty());
    }

    #[test]
    fn supported_releases_are_strictly_ascending() {
        // 合成顺序与界面排序都依赖这份清单按数据版本升序
        assert!(
            SUPPORTED_RELEASES.windows(2).all(|w| w[0].0 < w[1].0),
            "SUPPORTED_RELEASES 必须按数据版本严格升序"
        );
    }
}
