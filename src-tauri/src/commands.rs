//! Tauri 命令层：前端可调用的全部入口。
//!
//! 约定：命令只做参数校验、调用解析层、把结果转成 DTO，
//! 不在这里承载业务规则，便于解析层独立测试。

use crate::backup;
use crate::error::{AppError, AppResult};
use crate::dto::{
    AnalysisResult, BackupEntryDto, BlockCount, BlockHitDto, BlockVersionDto,
    ConvertItemDto, ConvertOutcomeDto, ConvertTargetDto, DuplicateGroupDto, DuplicateScanDto,
    EntityCount,
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

/// 进程内缓存的内置方块库。
///
/// 方块库解压后约 5.6 MB、含 4 万余条方块定义，而方块检索是随输入逐键
/// 触发的；每次重新读取与解析会让输入明显卡顿。方块库是随包分发的
/// 静态资源，载入一次即可长期复用。
static BLOCK_DB: std::sync::OnceLock<crate::blockdb::BlockDb> = std::sync::OnceLock::new();

/// 取内置方块库，首次调用时载入。
///
/// 必须传入 AppHandle：打包后方块库位于 Tauri 的资源目录，
/// 只查 exe_dir()/res/ 或编译期的 CARGO_MANIFEST_DIR 在用户机器上都不存在，
/// 忽略资源目录会让方块检索与替换在发布版整块失效。
fn open_db(app: Option<&tauri::AppHandle>) -> AppResult<&'static crate::blockdb::BlockDb> {
    if let Some(db) = BLOCK_DB.get() {
        return Ok(db);
    }
    let loaded = crate::blockdb::BlockDb::load(&db_archive(app)?.to_string_lossy())?;
    // 并发下可能被别的线程抢先设置，此时沿用已设置的即可
    let _ = BLOCK_DB.set(loaded);
    BLOCK_DB
        .get()
        .ok_or_else(|| AppError::Parse("方块库缓存初始化失败".into()))
}

/// 跨版本检索方块
#[tauri::command]
pub fn search_blocks(
    app: tauri::AppHandle,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<BlockHitDto>, String> {
    let db = open_db(Some(&app)).map_err(|e| e.to_string())?;
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
    let db = open_db(Some(&app)).map_err(|e| e.to_string())?;
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
    let db = open_db(Some(&app)).map_err(|e| e.to_string())?;
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
    let db = open_db(Some(&app)).map_err(|e| e.to_string())?;
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
    let db = open_db(Some(&app)).map_err(|e| e.to_string())?;
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
    let db = open_db(Some(&app)).map_err(|e| e.to_string())?;
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
    let db = open_db(Some(&app)).map_err(|e| e.to_string())?;
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
    let db = open_db(Some(&app)).map_err(|e| e.to_string())?;
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
}
