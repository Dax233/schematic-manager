//! Tauri 命令层：前端可调用的全部入口。
//!
//! 约定：命令只做参数校验、调用解析层、把结果转成 DTO，
//! 不在这里承载业务规则，便于解析层独立测试。

use crate::backup;
use crate::error::{AppError, AppResult};
use crate::dto::{
    AnalysisResult, BackupEntryDto, BlockCount, BlockHitDto, BlockVersionDto,
    ConvertItemDto, ConvertOutcomeDto, ConvertTargetDto, DuplicateGroupDto, DuplicateScanDto,
    EntityCount, ImportOutcomeDto, PackEntryDto, PackListDto, PackLoadFailureDto,
    ReplaceOutcomeDto, ReplaceRuleDto, SceneDto, SaveOutcome, ScanResult, SchematicSummary,
    UnreadableFileDto, VersionOptionDto,
};
use crate::litematic::{self, Summary};
use std::path::{Path, PathBuf};
use tauri::{Manager, path::BaseDirectory};

/// 程序所在目录。备份目录以它为根，与旧版行为一致。
fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// 由内部摘要转换为对前端的契约
fn to_dto(s: Summary) -> SchematicSummary {
    let p = PathBuf::from(&s.path);
    SchematicSummary {
        file_name: p
            .file_name()
            .map(|v| v.to_string_lossy().into_owned())
            .unwrap_or_default(),
        dir_name: p
            .parent()
            .map(|v| v.to_string_lossy().into_owned())
            .unwrap_or_default(),
        path: s.path.clone(),
        file_size: s.file_size,
        error: s.error.clone(),
        format_version: s.format_version,
        data_version: s.data_version,
        mc_version_name: s.mc_version_name.clone(),
        name: s.name.clone(),
        author: s.author.clone(),
        description: s.description.clone(),
        time_created: s.time_created,
        time_modified: s.time_modified,
        enclosing: s.enclosing,
        total_blocks: s.total_blocks,
        total_volume: s.total_volume,
        region_count: s.region_count,
        entity_count: s.entity_count,
        tile_entity_count: s.tile_entity_count,
        pending_ticks: s.pending_ticks,
        has_preview: s.has_preview,
    }
}

/// 只读扫描单个投影文件，不解码方块数据。
#[tauri::command]
pub fn peek_schematic(path: String) -> SchematicSummary {
    to_dto(litematic::Schematic::peek(&path))
}

/// 递归扫描目录下的全部 .litematic 文件。
///
/// depth 限制递归层数，避免误选到盘符根目录时遍历整块磁盘。
/// 不可读的子目录会被跳过并计数，随结果返回给界面提示，
/// 而不是静默给出一个看起来完整、实则缺项的结果。
#[tauri::command]
pub fn scan_litematics(dir: String, max_depth: Option<u32>) -> ScanResult {
    let depth = max_depth.unwrap_or(6).min(16);
    let mut found: Vec<PathBuf> = Vec::new();
    let mut skipped = SkipReport::default();
    collect_litematics(Path::new(&dir), depth, &mut found, &mut skipped);
    found.sort();

    ScanResult {
        items: found
            .iter()
            .map(|p| {
                let path = p.to_string_lossy().into_owned();
                to_dto(litematic::Schematic::peek(&path))
            })
            .collect(),
        skipped_dirs: skipped.count,
        skipped_paths: skipped.paths,
    }
}

/// 跳过的目录统计。路径只保留前若干条，避免异常目录结构下无限增长。
#[derive(Default)]
struct SkipReport {
    count: u32,
    paths: Vec<String>,
}

const MAX_SKIP_PATHS: usize = 20;

impl SkipReport {
    fn record(&mut self, dir: &Path) {
        self.count = self.count.saturating_add(1);
        if self.paths.len() < MAX_SKIP_PATHS {
            self.paths.push(dir.to_string_lossy().into_owned());
        }
    }
}

fn collect_litematics(dir: &Path, depth: u32, out: &mut Vec<PathBuf>, skipped: &mut SkipReport) {
    if depth == 0 {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => {
            skipped.record(dir);
            return;
        }
    };
    for e in entries {
        let entry = match e {
            Ok(v) => v,
            Err(_) => {
                skipped.record(dir);
                continue;
            }
        };
        let p = entry.path();
        let ft = match entry.file_type() {
            Ok(v) => v,
            Err(_) => {
                skipped.record(&p);
                continue;
            }
        };
        if ft.is_dir() {
            collect_litematics(&p, depth - 1, out, skipped);
        } else if ft.is_file()
            && p.extension()
                .map(|x| x.eq_ignore_ascii_case("litematic"))
                .unwrap_or(false)
        {
            out.push(p);
        }
    }
}

/// 完整分析：解码全部方块并统计。
#[tauri::command]
pub fn analyze_schematic(path: String) -> Result<AnalysisResult, String> {
    let sc = litematic::Schematic::load(&path).map_err(|e| e.to_string())?;
    let a = sc.analyze();

    let mut blocks: Vec<BlockCount> = a
        .blocks_by_state
        .iter()
        .map(|(state, count)| {
            // 按名称聚合的计数用于展示，状态串用于区分具体朝向等
            let name = state.split('[').next().unwrap_or(state).to_string();
            BlockCount { name, state: state.clone(), count: *count }
        })
        .collect();
    blocks.sort_by(|x, y| y.count.cmp(&x.count).then_with(|| x.state.cmp(&y.state)));

    let mut entities: Vec<EntityCount> = a
        .entities_by_id
        .iter()
        .map(|(id, count)| EntityCount { id: id.clone(), count: *count })
        .collect();
    entities.sort_by(|x, y| y.count.cmp(&x.count).then_with(|| x.id.cmp(&y.id)));

    let mut tiles: Vec<EntityCount> = a
        .tile_entities_by_id
        .iter()
        .map(|(id, count)| EntityCount { id: id.clone(), count: *count })
        .collect();
    tiles.sort_by(|x, y| y.count.cmp(&x.count).then_with(|| x.id.cmp(&y.id)));

    let mut types = std::collections::BTreeSet::new();
    for b in &blocks {
        types.insert(b.name.clone());
    }

    Ok(AnalysisResult {
        distinct_block_types: types.len(),
        distinct_states: blocks.len(),
        blocks,
        entities,
        tile_entities: tiles,
        non_air_total: a.non_air_total,
        air_total: a.air_total,
    })
}

/// 内置方块库压缩包的位置。
///
/// 依次尝试：可执行文件同级的 res/（旧版布局）、随包分发的资源目录、
/// 以及开发时的源码树。三者都不存在时给出明确错误，而不是让载入在
/// 更深处报出难以理解的解析失败。
fn db_archive(app: Option<&tauri::AppHandle>) -> AppResult<PathBuf> {
    let mut tried: Vec<PathBuf> = Vec::new();
    if let Some(a) = app {
        if let Ok(p) = a.path().resolve("resources/blocks.dat.z", BaseDirectory::Resource) {
            if p.is_file() {
                return Ok(p);
            }
            tried.push(p);
        }
    }
    let beside = exe_dir().join("res").join("blocks.dat.z");
    if beside.is_file() {
        return Ok(beside);
    }
    tried.push(beside);
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("resources")
        .join("blocks.dat.z");
    if dev.is_file() {
        return Ok(dev);
    }
    tried.push(dev);
    Err(AppError::Parse(format!(
        "找不到方块库 blocks.dat.z，已尝试：{}",
        tried
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join("、")
    )))
}

/// 已载入的方块库，附带资源包加载过程中的失败说明。
///
/// 失败原因不放在 `BlockDb` 里：它描述的是「载入过程」而不是方块库内容，
/// 混进数据模型会让这个结构承担两种职责。
pub struct LoadedDb {
    pub db: std::sync::Arc<crate::blockdb::BlockDb>,
    /// 资源包文件名 -> 加载失败原因
    pub failures: Vec<(String, String)>,
}

/// 进程内缓存的方块库。
///
/// 方块库解压后约 5.6 MB、含 4 万余条方块定义，而方块检索是随输入逐键
/// 触发的；每次重新读取与解析会让输入明显卡顿。方块库是随包分发的
/// 静态资源，载入一次即可长期复用。
///
/// 用 `RwLock<Option<..>>` 而不是 `OnceLock`：用户导入、启用或删除资源包后，
/// 方块库的内容必须随之变化。`OnceLock` 一旦写入就再也改不了，会让
/// 「刚导入的包要重启程序才生效」——而用户没有任何理由知道这一点。
static BLOCK_DB: std::sync::OnceLock<std::sync::RwLock<Option<std::sync::Arc<LoadedDb>>>> =
    std::sync::OnceLock::new();

fn db_cache() -> &'static std::sync::RwLock<Option<std::sync::Arc<LoadedDb>>> {
    BLOCK_DB.get_or_init(|| std::sync::RwLock::new(None))
}

/// 让方块库缓存失效。
///
/// 任何会改变「哪些资源包被启用」的操作之后都必须调用，否则改动要等
/// 下次启动才生效。
fn invalidate_db() {
    // 锁中毒说明此前有线程在持锁时 panic；缓存只是可重建的派生数据，
    // 取回内部值继续用即可，不必让整个程序跟着崩
    let mut g = db_cache().write().unwrap_or_else(|e| e.into_inner());
    *g = None;
}

/// 把清单里已启用的资源包并入基础库。
///
/// 从 `open_db` 里抽出来是为了可测试：这段逻辑决定「用户导入的包到底
/// 有没有生效」，而它此前完全没有测试覆盖。抽出来后可以用真实文件与
/// 内存构造的包直接验证合并结果。
///
/// 单个包载入失败不能中断整库：用户可能手工删过文件、或包本身损坏，
/// 此时其余包与内置库仍应可用。失败原因逐项返回供界面显示，
/// **不静默跳过**——界面上显示为启用却不生效是最难排查的状态。
///
/// `resolve` 给出包文件的完整路径，便于测试注入临时目录。
fn merge_enabled_packs<F>(
    base: &mut crate::blockdb::BlockDb,
    entries: &[crate::pack_registry::PackEntry],
    resolve: F,
) -> Vec<(String, String)>
where
    F: Fn(&str) -> PathBuf,
{
    let mut failures: Vec<(String, String)> = Vec::new();
    for item in crate::pack_registry::enabled_for_load(entries) {
        let path = resolve(&item.file);
        // 是否为叠加包必须由清单里的类型决定：pack 文件本身不存这个信息
        match crate::blockdb::BlockDb::load(&path.to_string_lossy(), item.is_overlay) {
            Ok(pack) => {
                if let Err(e) = base.merge_from(&pack, item.is_overlay) {
                    failures.push((item.file.clone(), format!("合并失败：{e}")));
                }
            }
            Err(e) => failures.push((item.file.clone(), format!("载入失败：{e}"))),
        }
    }
    failures
}

/// 取方块库：内置基础库 + 清单里已启用的资源包。
///
/// 必须传入 AppHandle：打包后方块库位于 Tauri 的资源目录，
/// 只查 exe_dir()/res/ 或编译期的 CARGO_MANIFEST_DIR 在用户机器上都不存在，
/// 忽略资源目录会让方块检索与替换在发布版整块失效。
fn open_db(app: Option<&tauri::AppHandle>) -> AppResult<std::sync::Arc<LoadedDb>> {
    {
        let g = db_cache().read().unwrap_or_else(|e| e.into_inner());
        if let Some(db) = g.as_ref() {
            return Ok(std::sync::Arc::clone(db));
        }
    }

    // 内置方块库是基础版本（legacy-cpp 的 kind=0），不是叠加包
    let mut db = crate::blockdb::BlockDb::load(&db_archive(app)?.to_string_lossy(), false)?;

    let (entries, _) = read_manifest()?;
    let failures = merge_enabled_packs(&mut db, &entries, |f| packs_dir().join(f));

    let loaded = std::sync::Arc::new(LoadedDb { db: std::sync::Arc::new(db), failures });
    let mut g = db_cache().write().unwrap_or_else(|e| e.into_inner());
    // 并发下可能被别的线程抢先设置，此时沿用已设置的即可
    if g.is_none() {
        *g = Some(std::sync::Arc::clone(&loaded));
    }
    Ok(match g.as_ref() {
        Some(existing) => std::sync::Arc::clone(existing),
        None => loaded,
    })
}

/// 跨版本检索方块
#[tauri::command]
pub fn search_blocks(
    app: tauri::AppHandle,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<BlockHitDto>, String> {
    let loaded = open_db(Some(&app)).map_err(|e| e.to_string())?;
    let db = &loaded.db;
    Ok(crate::block_search::search(db, &query, limit.unwrap_or(200))
        .into_iter()
        .map(|h| BlockHitDto {
            name: h.name,
            zh: h.zh,
            en: h.en,
            versions: h
                .versions
                .into_iter()
                .map(|v| BlockVersionDto { index: v.index, data_version: v.data_version })
                .collect(),
        })
        .collect())
}

/// 列出方块库中的全部版本
#[tauri::command]
pub fn list_block_versions(app: tauri::AppHandle) -> Result<Vec<VersionOptionDto>, String> {
    let loaded = open_db(Some(&app)).map_err(|e| e.to_string())?;
    let db = &loaded.db;
    Ok(crate::block_search::version_options(db)
        .into_iter()
        .map(|v| VersionOptionDto {
            index: v.index,
            name: v.name,
            data_version: v.data_version,
            is_overlay: v.is_overlay,
            block_count: v.block_count,
        })
        .collect())
}

/// 预计会被替换的方块数量，不写入文件
#[tauri::command]
pub fn preview_replace(
    app: tauri::AppHandle,
    schematic_path: String,
    rule: ReplaceRuleDto,
) -> Result<ReplaceOutcomeDto, String> {
    let loaded = open_db(Some(&app)).map_err(|e| e.to_string())?;
    let db = &loaded.db;
    let sc = crate::litematic::Schematic::load(&schematic_path).map_err(|e| e.to_string())?;
    let summary = sc.summary();
    let vi = db
        .pick_version(summary.data_version)
        .ok_or_else(|| "方块库中没有可用版本".to_string())?;
    let info = crate::block_model::DbBlockInfo { db, version: &db.versions[vi] };
    let rule: crate::replace::ReplaceRule = rule.into();
    Ok(ReplaceOutcomeDto {
        replaced: crate::replace::count_matches(&sc, &rule, &info),
        palette_changed: 0,
    })
}

/// 执行方块替换并写回文件。写前先备份。
#[tauri::command]
pub fn apply_replace(
    app: tauri::AppHandle,
    schematic_path: String,
    rule: ReplaceRuleDto,
) -> Result<ReplaceOutcomeDto, String> {
    let loaded = open_db(Some(&app)).map_err(|e| e.to_string())?;
    let db = &loaded.db;
    let mut sc = crate::litematic::Schematic::load(&schematic_path).map_err(|e| e.to_string())?;
    let summary = sc.summary();
    let vi = db
        .pick_version(summary.data_version)
        .ok_or_else(|| "方块库中没有可用版本".to_string())?;
    let info = crate::block_model::DbBlockInfo { db, version: &db.versions[vi] };
    let rule: crate::replace::ReplaceRule = rule.into();

    let out = crate::replace::replace_blocks(&mut sc, &rule, &info, 0);
    if out.replaced == 0 {
        // 没有任何改动就不写文件，避免无谓地产生一次备份
        return Ok(out.into());
    }

    // 改动前先备份：替换是批量改写，出错时这是唯一的回退途径。
    // 备份根目录必须与 list_backups / save_metadata 一致（都是 exe_dir），
    // 否则替换产生的备份在「备份管理」里永远看不到，也无法还原。
    backup::create(&exe_dir(), Path::new(&schematic_path), "replace")
        .map_err(|e| e.to_string())?;
    sc.save_to(&schematic_path).map_err(|e| e.to_string())?;

    let name = Path::new(&schematic_path)
        .file_name()
        .map(|v| v.to_string_lossy().into_owned())
        .unwrap_or_default();
    let _ = backup::append_log(
        &backup::backup_root(&exe_dir()),
        &format!("替换 {name}：{} -> {}，共 {} 个", rule.from_name, rule.to_name, out.replaced),
    );
    Ok(out.into())
}

/// 列出可作为转换目标的版本
#[tauri::command]
pub fn convert_targets(app: tauri::AppHandle, schematic_path: String) -> Result<Vec<ConvertTargetDto>, String> {
    let loaded = open_db(Some(&app)).map_err(|e| e.to_string())?;
    let db = &loaded.db;
    let sc = crate::litematic::Schematic::load(&schematic_path).map_err(|e| e.to_string())?;
    let dv = sc.summary().data_version;
    Ok(crate::convert::targets(db, dv)
        .into_iter()
        .map(|t| ConvertTargetDto { index: t.index, name: t.name, data_version: t.data_version })
        .collect())
}

/// 分析：投影中用到但目标版本没有的方块
#[tauri::command]
pub fn analyze_conversion(
    app: tauri::AppHandle,
    schematic_path: String,
    target_index: usize,
) -> Result<Vec<ConvertItemDto>, String> {
    let loaded = open_db(Some(&app)).map_err(|e| e.to_string())?;
    let db = &loaded.db;
    let sc = crate::litematic::Schematic::load(&schematic_path).map_err(|e| e.to_string())?;
    Ok(crate::convert::analyze(&sc, db, target_index)
        .into_iter()
        .map(|i| ConvertItemDto {
            name: i.name,
            count: i.count,
            suggested: i.suggested,
            chosen: i.chosen,
        })
        .collect())
}

/// 执行转换，结果另存为新文件，原文件不动。
#[tauri::command]
pub fn apply_conversion(
    app: tauri::AppHandle,
    schematic_path: String,
    target_index: usize,
    items: Vec<ConvertItemDto>,
) -> Result<ConvertOutcomeDto, String> {
    let loaded = open_db(Some(&app)).map_err(|e| e.to_string())?;
    let db = &loaded.db;
    let mut sc = crate::litematic::Schematic::load(&schematic_path).map_err(|e| e.to_string())?;

    let source_idx = db
        .pick_version(sc.summary().data_version)
        .ok_or_else(|| "方块库中找不到与投影匹配的版本".to_string())?;
    let target = db
        .versions
        .get(target_index)
        .ok_or_else(|| "目标版本不存在".to_string())?;

    // 另存为新文件：转换会大量改写方块，直接覆盖原文件风险太大
    let src = Path::new(&schematic_path);
    let dir = src.parent().unwrap_or_else(|| Path::new("."));
    let stem = src
        .file_stem()
        .map(|v| v.to_string_lossy().into_owned())
        .unwrap_or_else(|| "projection".into());
    let out_path = dir.join(format!("{stem}_{}.litematic", target.name));

    let items: Vec<crate::convert::ConvertItem> = items
        .into_iter()
        .map(|i| crate::convert::ConvertItem {
            name: i.name,
            count: i.count,
            suggested: i.suggested,
            chosen: i.chosen,
        })
        .collect();

    let out = crate::convert::convert(&mut sc, db, source_idx, target_index, &items);
    // 只有真的替换过方块才需要重编码 BlockStates 与调色板；
    // 若仅改版本号（方块都没变），save_to 仍会序列化整个 root，
    // 版本号照样写入，无需无谓地重写全部区域数据。
    sc.blocks_modified = out.replaced_blocks > 0;
    sc.save_to(&out_path.to_string_lossy()).map_err(|e| e.to_string())?;

    let name = out_path
        .file_name()
        .map(|v| v.to_string_lossy().into_owned())
        .unwrap_or_default();
    let _ = backup::append_log(
        &backup::backup_root(&exe_dir()),
        &format!("转换 {name}：替换 {} 种方块，{} 种未指定", out.replaced_kinds, out.skipped_kinds),
    );

    Ok(ConvertOutcomeDto {
        replaced_kinds: out.replaced_kinds,
        replaced_blocks: out.replaced_blocks,
        skipped_kinds: out.skipped_kinds,
        output_path: out_path.to_string_lossy().into_owned(),
    })
}

/// 在一个目录下筛查内容重复的投影文件。
///
/// 只探测该目录本身，不递归子目录：重复筛查的目的通常是整理一个
/// 导出目录，递归会把不同用途目录下的同名文件也卷进来。
#[tauri::command]
pub fn scan_duplicates(dir: String) -> Result<DuplicateScanDto, String> {
    let mut candidates = Vec::new();
    let entries = std::fs::read_dir(&dir).map_err(|e| format!("无法读取目录 {dir}：{e}"))?;
    for e in entries.flatten() {
        let path = e.path();
        if !path.is_file() {
            continue;
        }
        let is_litematic = path
            .extension()
            .map(|x| x.eq_ignore_ascii_case("litematic"))
            .unwrap_or(false);
        if !is_litematic {
            continue;
        }
        let size = e.metadata().map(|m| m.len()).unwrap_or(0);
        candidates.push(crate::duplicate::Candidate { path, size });
    }

    let r = crate::duplicate::find(&candidates);
    Ok(DuplicateScanDto {
        groups: r
            .groups
            .into_iter()
            .map(|g| DuplicateGroupDto {
                size: g.size,
                paths: g.paths.iter().map(|p| p.to_string_lossy().into_owned()).collect(),
            })
            .collect(),
        unreadable: r
            .unreadable
            .into_iter()
            .map(|(p, reason)| UnreadableFileDto {
                path: p.to_string_lossy().into_owned(),
                reason,
            })
            .collect(),
    })
}

/// 列出备份。stem 非空时只返回该文件的备份。
#[tauri::command]
pub fn list_backups(stem: Option<String>) -> Vec<BackupEntryDto> {
    backup::list(&backup::backup_root(&exe_dir()))
        .into_iter()
        .filter(|e| stem.as_deref().map_or(true, |s| s == e.stem))
        .map(|e| BackupEntryDto {
            path: e.path.to_string_lossy().into_owned(),
            stem: e.stem,
            op: e.op,
            created_ns: e.created_ns.to_string(),
            size: e.size,
        })
        .collect()
}

/// 把备份还原到目标文件。
///
/// 还原是覆盖写，因此先校验备份与目标同源：跨文件还原很容易把 A 的备份
/// 盖到 B 上，而覆盖后 B 的原内容只剩它自己的备份可寻。
#[tauri::command]
pub fn restore_backup(backup_path: String, target_path: String) -> Result<(), String> {
    let b = PathBuf::from(&backup_path);
    let t = PathBuf::from(&target_path);
    if !backup::matches_target(&b, &t) {
        return Err(format!(
            "备份与目标文件不同源，已中止还原：{}",
            t.file_name().map(|v| v.to_string_lossy().into_owned()).unwrap_or_default()
        ));
    }
    // 还原是覆盖写：先把当前内容备份一份，否则一旦还原错了，
    // 目标文件的现有内容就只剩更早的备份可寻。
    if t.is_file() {
        backup::create(&exe_dir(), &t, "restore-before").map_err(|e| e.to_string())?;
    }
    backup::restore(&b, &t).map_err(|e| e.to_string())?;
    let name = t.file_name().map(|v| v.to_string_lossy().into_owned()).unwrap_or_default();
    let _ = backup::append_log(&backup::backup_root(&exe_dir()), &format!("还原 {name} <- {backup_path}"));
    Ok(())
}

/// 删除一条备份。
#[tauri::command]
pub fn delete_backup(backup_path: String) -> Result<(), String> {
    let p = PathBuf::from(&backup_path);
    // 参数来自前端，必须限定在备份根目录内：删除是不可逆操作，
    // 不能允许任意路径被删掉。
    let root = backup::backup_root(&exe_dir());
    if !backup::is_inside(&root, &p) {
        return Err(format!("拒绝删除备份目录之外的路径：{}", p.display()));
    }
    backup::delete(&p).map_err(|e| e.to_string())?;
    let name = p.file_name().map(|v| v.to_string_lossy().into_owned()).unwrap_or_default();
    let _ = backup::append_log(&backup::backup_root(&exe_dir()), &format!("删除备份 {name}"));
    Ok(())
}

/// 读取操作日志，由新到旧。
#[tauri::command]
pub fn read_operation_log(max_lines: Option<usize>) -> Vec<String> {
    backup::read_log(&backup::backup_root(&exe_dir()), max_lines.unwrap_or(200).min(5000))
}

/// 组装前端 Three.js 渲染所需的场景。
///
/// 方块库作为应用资源随包分发，路径由这里解析，
/// 前端不必知道它在哪个目录，也不必关心各平台的资源布局差异。
#[tauri::command]
pub fn build_scene(app: tauri::AppHandle, schematic_path: String) -> Result<SceneDto, String> {
    let loaded = open_db(Some(&app)).map_err(|e| e.to_string())?;
    let db = &loaded.db;
    crate::scene::build_with_db(&schematic_path, db)
        .map(SceneDto::from)
        .map_err(|e| e.to_string())
}

/// 保存元数据（投影名称 / 作者 / 描述）。
///
/// 写入前先在 backups/ 下留一份原件：这几项都是用户手动输入的内容，
/// 出错时必须有可回退的原件。备份失败则中止保存，不做「没备份就改」。
#[tauri::command]
pub fn save_metadata(
    path: String,
    name: Option<String>,
    author: Option<String>,
    description: Option<String>,
) -> Result<SaveOutcome, String> {
    let p = PathBuf::from(&path);
    if !p.is_file() {
        return Err(format!("文件不存在：{path}"));
    }

    // 先载入并判断是否真的有变化，再决定要不要备份。
    // 顺序反过来的话，重复保存相同内容也会产出无用备份。
    let mut sc = litematic::Schematic::load(&path).map_err(|e| e.to_string())?;

    let mut changed = false;
    if let Some(v) = name {
        changed |= sc.set_meta_name(&v);
    }
    if let Some(v) = author {
        changed |= sc.set_meta_author(&v);
    }
    if let Some(v) = description {
        changed |= sc.set_meta_description(&v);
    }

    if !changed {
        // 可能是值本身没有变化，也可能是该投影缺少这几项字段。
        // 两种都不应备份、也不应写盘；后者需要让用户知道。
        let md = sc.root.get("Metadata");
        let has_fields = md.is_some()
            && ["Name", "Author", "Description"]
                .iter()
                .any(|k| md.and_then(|m| m.get(k)).is_some());
        if !has_fields {
            return Err("该投影缺少可编辑的元数据字段".into());
        }
        return Err("内容没有变化，未做保存".into());
    }

    let bak = backup::create(&exe_dir(), &p, "metadata")
        .map_err(|e| format!("备份失败，已中止保存：{e}"))?;

    sc.save_to(&path).map_err(|e| format!("写入失败：{e}"))?;

    Ok(SaveOutcome {
        path,
        backup_path: bak.backup_path.to_string_lossy().into_owned(),
    })
}

/// 查询 dataVersion 对应的游戏版本名。
#[tauri::command]
pub fn mc_version_name(data_version: i32) -> String {
    crate::mcver::version_name_from_data_version(data_version)
}

#[cfg(test)]
mod tests {
    use super::*;

    // 注意：这里的路径必须与平台无关。
    // std::path::Path 按各平台的分隔符切分，在 Unix 上反斜杠只是普通字符，
    // 因此 Windows 风格的 "Z:\\x.litematic" 在 Linux 上会被整体当作文件名。
    // 测试只关心「文件不存在时的行为」，用纯文件名即可。
    #[test]
    fn scan_missing_dir_returns_empty() {
        let r = scan_litematics("__sm_not_exist_dir__".into(), Some(2));
        assert!(r.items.is_empty());
        assert!(r.skipped_dirs > 0, "不存在的目录应计入跳过数");
    }

    #[test]
    fn peek_missing_file_reports_error() {
        let s = peek_schematic("__sm_not_exist__.litematic".into());
        assert!(!s.error.is_empty(), "文件不存在时应给出错误原因");
        assert_eq!(s.file_name, "__sm_not_exist__.litematic");
    }

    #[test]
    fn scan_finds_litematic_files_only() {
        let dir = std::env::temp_dir().join("sm_scan_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.litematic"), b"x").unwrap();
        std::fs::write(dir.join("b.txt"), b"x").unwrap();
        std::fs::write(dir.join("sub/c.LITEMATIC"), b"x").unwrap();

        let r = scan_litematics(dir.to_string_lossy().into_owned(), Some(4));
        let names: Vec<&str> = r.items.iter().map(|s| s.file_name.as_str()).collect();
        assert_eq!(names.len(), 2, "应只收录 .litematic，实际 {names:?}");
        assert!(names.contains(&"a.litematic"));
        assert!(names.contains(&"c.LITEMATIC"), "扩展名匹配应忽略大小写");
        assert_eq!(r.skipped_dirs, 0, "正常目录不应产生跳过");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mc_version_command_proxies_table() {
        assert!(!mc_version_name(3465).is_empty());
        assert_eq!(mc_version_name(-1), "");
    }

    use crate::blockdb::{BlockDb, BlockDef, LangEntry, Model, ModelElement, ModelFace, TextureInfo, Variant, VersionDef};
    use crate::pack_registry::{PackEntry, PackKind};
    use std::collections::HashMap;

    /// 造一个可序列化、可读回的最小方块库
    fn sample_db(pack: &str, dv: i32, block: &str, tex: &str, pixel: u8) -> BlockDb {
        let strings = vec![pack.to_string(), tex.to_string()];
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
            name: pack.into(),
            data_version: dv,
            lang_idx: 0,
            texture_by_path: HashMap::from([(tex.to_string(), 0u32)]),
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
            format_version: 3,
            strings,
            langs: vec![vec![LangEntry {
                block_name: block.into(),
                en: "EN".into(),
                zh: "中文名".into(),
            }]],
            textures: vec![TextureInfo { w: 1, h: 1, offset: 0, len: 4, has_alpha: false }],
            models: vec![model],
            versions: vec![version],
            pixels: vec![pixel, pixel, pixel, 255],
        }
    }

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join(format!(
                "cmd_{}_{}",
                name,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 把一个库写成 zlib 压缩的 pack 文件
    fn write_pack(dir: &std::path::Path, file: &str, db: &BlockDb) {
        let raw = crate::blockdb_write::serialize(db).expect("序列化失败");
        use std::io::Write;
        let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(9));
        e.write_all(&raw).unwrap();
        let packed = e.finish().unwrap();
        std::fs::write(dir.join(file), packed).unwrap();
    }

    fn entry(file: &str, kind: PackKind, enabled: bool) -> PackEntry {
        PackEntry {
            file: file.into(),
            name: file.into(),
            source: "/j/x.jar".into(),
            kind,
            data_version: 3465,
            enabled,
        }
    }

    /// 这是本轮的核心验证：导入的包必须真正参与方块解析。
    /// 此前 open_db 只加载内置库，导入功能对渲染与检索毫无影响。
    #[test]
    fn enabled_pack_is_merged_and_its_block_becomes_findable() {
        let dir = tmp("merge_enabled");
        let base = sample_db("base", 3465, "minecraft:stone", "block/stone", 10);
        let pack = sample_db("mymod", 3465, "mymod:thing", "block/thing", 20);
        write_pack(&dir, "mymod.pack", &pack);

        let mut merged = base;
        let entries = vec![entry("mymod.pack", PackKind::Mod, true)];
        let failures = merge_enabled_packs(&mut merged, &entries, |f| dir.join(f));
        assert!(failures.is_empty(), "不应有失败：{failures:?}");

        let ov = merged.versions.last().unwrap();
        assert!(
            crate::block_model::find_block(&merged, ov, "mymod:thing").is_some(),
            "启用的包必须真正参与解析，否则导入毫无作用"
        );
        assert_eq!(merged.versions.len(), 2);
    }

    #[test]
    fn disabled_pack_is_not_merged() {
        let dir = tmp("merge_disabled");
        let base = sample_db("base", 3465, "minecraft:stone", "block/stone", 10);
        let pack = sample_db("mymod", 3465, "mymod:thing", "block/thing", 20);
        write_pack(&dir, "mymod.pack", &pack);

        let mut merged = base;
        let entries = vec![entry("mymod.pack", PackKind::Mod, false)];
        let failures = merge_enabled_packs(&mut merged, &entries, |f| dir.join(f));
        assert!(failures.is_empty());
        assert_eq!(merged.versions.len(), 1, "停用的包不应被并入");
    }

    #[test]
    fn missing_pack_file_is_reported_not_ignored() {
        // 文件被手工删掉：必须上报，界面才能提示；静默跳过会让用户
        // 看着「已启用」却不知道为什么没生效
        let dir = tmp("merge_missing");
        let base = sample_db("base", 3465, "minecraft:stone", "block/stone", 10);
        let mut merged = base;
        let entries = vec![entry("gone.pack", PackKind::Mod, true)];
        let failures = merge_enabled_packs(&mut merged, &entries, |f| dir.join(f));

        assert_eq!(failures.len(), 1, "缺失的包必须被记为失败");
        assert_eq!(failures[0].0, "gone.pack");
        assert!(failures[0].1.contains("载入失败"), "原因：{}", failures[0].1);
        assert_eq!(merged.versions.len(), 1, "基础库仍应可用");
    }

    #[test]
    fn corrupt_pack_does_not_break_other_packs() {
        // 一个坏包 + 一个好包：坏包上报，好包仍被并入
        let dir = tmp("merge_corrupt");
        let base = sample_db("base", 3465, "minecraft:stone", "block/stone", 10);
        let good = sample_db("good", 3465, "good:block", "block/good", 30);
        write_pack(&dir, "good.pack", &good);
        std::fs::write(dir.join("bad.pack"), b"not a pack file at all").unwrap();

        let mut merged = base;
        let entries = vec![
            entry("bad.pack", PackKind::Mod, true),
            entry("good.pack", PackKind::Mod, true),
        ];
        let failures = merge_enabled_packs(&mut merged, &entries, |f| dir.join(f));

        assert_eq!(failures.len(), 1, "只有坏包应失败：{failures:?}");
        assert_eq!(failures[0].0, "bad.pack");
        assert_eq!(merged.versions.len(), 2, "好包仍应被并入");
        let ov = merged.versions.last().unwrap();
        assert!(crate::block_model::find_block(&merged, ov, "good:block").is_some());
    }

    #[test]
    fn version_pack_is_added_as_base_version_and_participates_in_picking() {
        // kind=1 的版本包不是叠加层，应成为可被选中的基础版本
        let dir = tmp("merge_version");
        let base = sample_db("base", 3465, "minecraft:stone", "block/stone", 10);
        let pack = sample_db("1.21", 3953, "minecraft:deepslate", "block/deepslate", 40);
        write_pack(&dir, "v121.pack", &pack);

        let mut merged = base;
        let entries = vec![entry("v121.pack", PackKind::Version, true)];
        merge_enabled_packs(&mut merged, &entries, |f| dir.join(f));

        assert_eq!(merged.versions.len(), 2);
        assert!(!merged.versions[1].is_overlay, "版本包不应标记为叠加包");
        assert_eq!(merged.pick_version(3953), Some(1), "新基础版本应可被选中");
        assert!(merged.overlays_of(1).is_empty(), "版本包不承载叠加包");
    }

    #[test]
    fn pack_texture_is_reachable_after_merge() {
        // 模组方块引用的贴图只存在于模组包里，合并后必须能取到其像素
        let dir = tmp("merge_tex");
        let base = sample_db("base", 3465, "minecraft:stone", "block/stone", 10);
        let pack = sample_db("mymod", 3465, "mymod:thing", "block/thing", 77);
        write_pack(&dir, "mymod.pack", &pack);

        let mut merged = base;
        merge_enabled_packs(
            &mut merged,
            &[entry("mymod.pack", PackKind::Mod, true)],
            |f| dir.join(f),
        );

        let sidx = merged
            .strings
            .iter()
            .position(|s| s == "block/thing")
            .expect("字符串表里应有模组贴图路径") as u32;
        let idx = merged.texture_pool_index(0, sidx);
        assert_ne!(idx, crate::blockdb::NO_TEXTURE, "应通过叠加包取到贴图");
        assert_eq!(merged.texture_pixels(idx as usize).unwrap(), &[77, 77, 77, 255]);
    }

    #[test]
    fn load_order_follows_manifest_order() {
        // 两个包定义同名方块：后者（清单里靠后）生效
        let dir = tmp("merge_order");
        let base = sample_db("base", 3465, "minecraft:stone", "block/stone", 10);
        let a = sample_db("A", 3465, "dup:block", "block/a", 1);
        let b = sample_db("B", 3465, "dup:block", "block/b", 2);
        write_pack(&dir, "a.pack", &a);
        write_pack(&dir, "b.pack", &b);

        let mut merged = base;
        let entries = vec![
            entry("a.pack", PackKind::Mod, true),
            entry("b.pack", PackKind::Mod, true),
        ];
        merge_enabled_packs(&mut merged, &entries, |f| dir.join(f));

        // 版本按清单顺序追加，因此最后一个是被后载入的 b
        let last = merged.versions.last().unwrap();
        assert_eq!(last.name, "B", "清单靠后的包应最后载入");
    }

    #[test]
    fn enabled_for_load_maps_kind_to_overlay_flag() {
        use crate::pack_registry::enabled_for_load;
        let entries = vec![
            entry("a.pack", PackKind::Version, true),
            entry("b.pack", PackKind::Mod, true),
            entry("c.pack", PackKind::Mod, false),
        ];
        let items = enabled_for_load(&entries);
        assert_eq!(items.len(), 2, "停用的不应出现");
        assert!(!items[0].is_overlay, "版本包不是叠加包");
        assert!(items[1].is_overlay, "模组包是叠加包");
    }
}

/// 删除某个资源包：同时移除清单记录与磁盘文件。
///
/// 顺序是先删文件再改清单：若反过来，删文件失败会留下一条指向不存在
/// 文件的记录（界面显示缺失）；而先删文件后写清单失败时，残留的是
/// 一条 `exists=false` 的记录，重新导入即可修复。
#[tauri::command]
pub fn delete_pack(file: String) -> Result<(), String> {
    // 只接受文件名，不接受路径：清单里的 file 由程序生成，
    // 但如果被手工改成 `../../x`，拼接后就会删到 packs 目录之外
    if file.contains('/') || file.contains('\\') || file.contains("..") {
        return Err(format!("资源包文件名不合法：{file}"));
    }
    let mut entries = read_manifest_for_write().map_err(|e| e.to_string())?;
    let target = packs_dir().join(&file);
    if target.exists() {
        std::fs::remove_file(&target).map_err(|e| {
            format!("无法删除资源包文件 {}：{e}", target.display())
        })?;
    }
    if !crate::pack_registry::remove(&mut entries, &file) {
        return Err(format!("清单里没有资源包 {file}"));
    }
    write_manifest(&entries).map_err(|e| e.to_string())?;
    invalidate_db();
    Ok(())
}

// ---------------------------------------------------------------- 资源包

/// 资源包目录：`<程序所在目录>/packs`
///
/// 与备份目录同样以程序目录为根，与旧版行为一致——用户既有的 packs
/// 目录无需迁移即可继续使用。
fn packs_dir() -> PathBuf {
    exe_dir().join("packs")
}

fn manifest_path() -> PathBuf {
    packs_dir().join("packs.tsv")
}

/// 读取清单。
///
/// 清单不存在时视为「尚未导入任何包」，返回空清单而不是报错：
/// 全新安装就是这种状态。
fn read_manifest() -> AppResult<(Vec<crate::pack_registry::PackEntry>, Vec<String>)> {
    let path = manifest_path();
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok((Vec::new(), Vec::new()))
        }
        Err(e) => {
            return Err(AppError::Parse(format!(
                "无法读取资源包清单 {}：{e}",
                path.display()
            )))
        }
    };
    Ok(crate::pack_registry::parse_with_errors(&text))
}

/// 写出清单。
///
/// 先写临时文件再改名：直接覆盖时若中途失败（磁盘满、进程被杀），
/// 清单会处于半截状态，用户已导入的包会整批丢失记录。
fn write_manifest(entries: &[crate::pack_registry::PackEntry]) -> AppResult<()> {
    let dir = packs_dir();
    std::fs::create_dir_all(&dir)?;
    let path = manifest_path();
    let tmp = dir.join("packs.tsv.tmp");
    std::fs::write(&tmp, crate::pack_registry::serialize(entries))?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// 列出已导入的资源包
#[tauri::command]
pub fn list_packs() -> Result<PackListDto, String> {
    let (entries, skipped) = read_manifest().map_err(|e| e.to_string())?;
    let dir = packs_dir();
    let packs = entries
        .into_iter()
        .map(|e| {
            let p = dir.join(&e.file);
            let (size, exists) = match std::fs::metadata(&p) {
                Ok(m) => (m.len(), true),
                Err(_) => (0, false),
            };
            PackEntryDto {
                kind_label: e.kind.label().to_string(),
                mc_version: crate::mcver::version_name_from_data_version(e.data_version),
                kind: e.kind.as_i32(),
                file: e.file,
                name: e.name,
                source: e.source,
                data_version: e.data_version,
                enabled: e.enabled,
                size,
                exists,
            }
        })
        .collect();
    // 取一次方块库：载入失败的原因由它记录。若因内置库缺失而整体失败，
    // 那不属「资源包载入失败」，不应混进这个列表——list_packs 本身
    // 仍应能列出清单，否则用户连自己导入过什么都看不到。
    let load_failures = match open_db(None) {
        Ok(loaded) => loaded
            .failures
            .iter()
            .map(|(file, reason)| PackLoadFailureDto {
                file: file.clone(),
                reason: reason.clone(),
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    Ok(PackListDto { packs, load_failures, skipped })
}

/// 从 jar 导入资源包。
///
/// `kind`：1 = MC 客户端版本包（数据版本取自 jar 内 version.json），
/// 2 = 模组包（叠加到 `data_version` 指定的版本）。
///
/// 命名规则沿用旧版：`name` 为空时取 jar 文件名主干。
#[tauri::command]
pub fn import_jar(
    jar_path: String,
    name: Option<String>,
    kind: i32,
    data_version: i32,
) -> Result<ImportOutcomeDto, String> {
    let kind = crate::pack_registry::PackKind::from_i32(kind)
        .ok_or_else(|| format!("不支持的类型 {kind}（只接受 1=MC 版本包、2=模组包）"))?;
    if kind == crate::pack_registry::PackKind::Builtin {
        return Err("内置基础库随程序分发，不能通过导入生成".into());
    }
    let src = Path::new(&jar_path);
    if !src.is_file() {
        return Err(format!("找不到 jar 文件：{jar_path}"));
    }

    let pack_name = match name.map(|v| v.trim().to_string()).filter(|v| !v.is_empty()) {
        Some(v) => v,
        None => src
            .file_stem()
            .map(|v| v.to_string_lossy().into_owned())
            .ok_or_else(|| format!("无法从路径推出资源包名：{jar_path}"))?,
    };

    let mut entries = read_manifest_for_write().map_err(|e| e.to_string())?;

    let file = crate::pack_registry::pack_file_name(&pack_name, &entries)
        .map_err(|e| e.to_string())?;
    let out_path = packs_dir().join(&file);
    let is_overlay = kind == crate::pack_registry::PackKind::Mod;

    let stats = crate::jar_file::build_pack_to(
        &jar_path,
        &out_path.to_string_lossy(),
        &pack_name,
        data_version,
        is_overlay,
    )
    .map_err(|e| e.to_string())?;

    // 模组包的 data_version 由调用方指定；版本包以 jar 内的为准，
    // 这里回读一次拿实际生效的值，避免清单记录与实际不符。
    let actual_dv = read_pack_data_version(&out_path, is_overlay).unwrap_or(data_version);

    let size = std::fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0);
    crate::pack_registry::upsert(
        &mut entries,
        crate::pack_registry::PackEntry {
            file: file.clone(),
            name: pack_name.clone(),
            source: jar_path.clone(),
            kind,
            data_version: actual_dv,
            enabled: true,
        },
    );
    write_manifest(&entries).map_err(|e| e.to_string())?;
    // 新包已登记为启用，方块库必须重建才会包含它
    invalidate_db();

    Ok(ImportOutcomeDto {
        file,
        name: pack_name,
        kind: kind.as_i32(),
        kind_label: kind.label().to_string(),
        data_version: actual_dv,
        mc_version: crate::mcver::version_name_from_data_version(actual_dv),
        blocks: stats.blocks,
        models: stats.models,
        textures: stats.textures,
        failed_blockstates: stats.failed_blockstates,
        failed_blocks: stats.failed_blocks,
        namespaces: stats.namespaces,
        size,
    })
}

/// 读回资源包实际使用的数据版本
fn read_pack_data_version(path: &Path, is_overlay: bool) -> Option<i32> {
    crate::blockdb::BlockDb::load(&path.to_string_lossy(), is_overlay)
        .ok()
        .and_then(|db| db.versions.first().map(|v| v.data_version))
}

/// 读取清单，并要求其中没有解析失败的行。
///
/// 用于**所有会整份重写清单的操作**。清单里存在坏行时，重写会把它们
/// 连同其内容一起抹掉，而用户损失的记录无从追回；因此宁可拒绝操作并
/// 指出清单位置，让用户先修好。三个写清单的命令必须用同一个判断，
/// 只在其中一个上加保护会让另外两个成为数据丢失的入口。
fn read_manifest_for_write() -> AppResult<Vec<crate::pack_registry::PackEntry>> {
    let (entries, skipped) = read_manifest()?;
    if !skipped.is_empty() {
        return Err(AppError::Parse(format!(
            "资源包清单有 {} 行无法解析，请先修复 {} 后再操作：{}",
            skipped.len(),
            manifest_path().display(),
            skipped.join("；")
        )));
    }
    Ok(entries)
}

/// 启用或停用某个资源包
#[tauri::command]
pub fn set_pack_enabled(file: String, enabled: bool) -> Result<(), String> {
    let mut entries = read_manifest_for_write().map_err(|e| e.to_string())?;
    if !crate::pack_registry::set_enabled(&mut entries, &file, enabled) {
        return Err(format!("清单里没有资源包 {file}"));
    }
    write_manifest(&entries).map_err(|e| e.to_string())?;
    // 启用集合变了，方块库必须重建：否则要重启程序才生效
    invalidate_db();
    Ok(())
}

