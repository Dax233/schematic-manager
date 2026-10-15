//! 基础版本库：方块与实体贴图的底座数据，以及本机 Minecraft 版本的发现。
//!
//! 方块库与实体贴图库不随安装包分发，而是由用户选择本机已装的客户端
//! jar 或在线下载，逐版本构建成单版本包，存放在应用数据目录的 `base/`
//! 下。本模块负责三件事：给出「支持补齐哪些正式版」的清单、把磁盘上的
//! 单版本包合成一个基础方块库、扫描 .minecraft 找出已装版本。

use crate::blockdb::BlockDb;
use crate::error::{AppError, AppResult};
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
    /// 版本目录名，即版本 id（如 `1.20.1`）
    pub id: String,
    /// 从 jar 内 version.json 读到的数据版本
    pub data_version: i32,
    /// 客户端 jar 的完整路径
    pub jar_path: PathBuf,
}

#[derive(Debug)]
pub struct McScan {
    pub versions: Vec<McVersionHit>,
    /// 存在同名 jar 但读不出版本信息的目录；逐条说明原因，不静默跳过
    pub warnings: Vec<String>,
}

/// 扫描 .minecraft 目录，列出 versions/ 下每个带同名客户端 jar 的版本。
///
/// 目录本身或 versions/ 子目录读不了时报错：用户选错目录时应得到明确
/// 提示，而不是一份空清单。单个 jar 损坏只记警告，不影响其它版本。
/// 结果按数据版本升序。
pub fn scan_minecraft(dir: &Path) -> AppResult<McScan> {
    let versions_dir = dir.join("versions");
    let rd = std::fs::read_dir(&versions_dir).map_err(|e| {
        AppError::Parse(format!(
            "读取 {} 失败：{e}（请选择 .minecraft 目录，其下应有 versions 子目录）",
            versions_dir.display()
        ))
    })?;
    let mut scan = McScan {
        versions: Vec::new(),
        warnings: Vec::new(),
    };
    for entry in rd {
        let entry = entry.map_err(|e| {
            AppError::Parse(format!("读取 {} 失败：{e}", versions_dir.display()))
        })?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().into_owned();
        let jar = path.join(format!("{id}.jar"));
        // 加载器/整合包目录（如 fabric-loader-…）没有同名 jar，属正常情况
        if !jar.is_file() {
            continue;
        }
        match read_jar_data_version(&jar) {
            Ok(data_version) => scan.versions.push(McVersionHit {
                id,
                data_version,
                jar_path: jar,
            }),
            Err(e) => scan.warnings.push(format!("{id}：{e}")),
        }
    }
    scan.versions.sort_by_key(|v| v.data_version);
    Ok(scan)
}

/// 从客户端 jar 的 version.json 读数据版本。
fn read_jar_data_version(jar: &Path) -> AppResult<i32> {
    let jf = crate::jar_file::JarFile::open(&jar.to_string_lossy())?;
    let raw = jf
        .raw("version.json")
        .map_err(|e| AppError::Parse(format!("jar 里没有 version.json：{e}")))?;
    let v: serde_json::Value = serde_json::from_slice(&raw)
        .map_err(|e| AppError::Parse(format!("version.json 解析失败：{e}")))?;
    let n = v
        .get("world_version")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| AppError::Parse("version.json 里没有 world_version".into()))?;
    i32::try_from(n).map_err(|_| AppError::Parse(format!("world_version {n} 超出范围")))
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
    fn scan_requires_a_versions_dir() {
        let dir = test_dir("scan-no-versions");
        let e = scan_minecraft(&dir).unwrap_err().to_string();
        assert!(e.contains("versions"), "错误应点明缺 versions 目录：{e}");
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
