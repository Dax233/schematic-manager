//! Tauri 命令层：前端可调用的全部入口。
//!
//! 约定：命令只做参数校验、调用解析层、把结果转成 DTO，
//! 不在这里承载业务规则，便于解析层独立测试。

use crate::backup;
use crate::blockdb::BlockDb;
use crate::error::{AppError, AppResult};
use crate::dto::{
    AnalysisResult, BackupEntryDto, BlockCount, BlockHitDto, BlockIconDto, BlockVersionDto,
    ConvertItemDto, ConvertOutcomeDto, ConvertTargetDto, DuplicateGroupDto, DuplicateScanDto,
    EntityCount, FileOpOutcome, ImportOutcomeDto, ModsImportDto, PackEntryDto, PackListDto, PackLoadFailureDto,
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

/// 在语言表里查一个键，返回 (中文, 英文)；查不到返回两个空串。
///
/// 语言表按版本保存（`VersionDef::lang_idx` 指向 `db.langs` 的一张表），
/// 因此必须先选对版本，否则查到的名字与投影里的方块对不上。
///
/// **中英文必须分别补齐**，不能一命中就返回：打包时只有一部分版本抓到了
/// 官方中文语言文件（实测 1.13.2~1.21.11 里约一半的表只有英文，
/// 用户那个 1.21.5 对应的表恰恰一条中文都没有）。早期写法在本版本的
/// 条目只有英文时直接返回，于是中文恒为空——这是实现者自己的缺陷，
/// 不是数据缺失。
///
/// 缺的一侧从整个语言表池里找同名的键补上：方块名跨版本相当稳定，
/// 与贴图解析的处理一致（见 `BlockDb::texture_pool_index` 的全局回退）。
pub(crate) fn lookup_lang(db: &BlockDb, version_idx: Option<usize>, key: &str) -> (String, String) {
    let mut zh = String::new();
    let mut en = String::new();

    // 本版本的表优先：它的命名最贴合该版本
    if let Some(vi) = version_idx {
        if let Some(version) = db.versions.get(vi) {
            if let Some(table) = db.langs.get(version.lang_idx as usize) {
                if let Some(e) = table.iter().find(|e| e.block_name == key) {
                    zh = e.zh.clone();
                    en = e.en.clone();
                }
            }
        }
    }
    if !zh.is_empty() && !en.is_empty() {
        return (zh, en);
    }

    // 缺哪一侧就补哪一侧
    for table in &db.langs {
        for e in table.iter().filter(|e| e.block_name == key) {
            if zh.is_empty() && !e.zh.is_empty() {
                zh = e.zh.clone();
            }
            if en.is_empty() && !e.en.is_empty() {
                en = e.en.clone();
            }
            if !zh.is_empty() && !en.is_empty() {
                return (zh, en);
            }
        }
    }
    (zh, en)
}

/// 查方块的中英文名。
///
/// 方块语言表的键有两种写法：老版本直接用方块名（`minecraft:stone`），
/// 新版本用 `block.minecraft.stone`。两种都试，避免因版本差异整列空着。
pub(crate) fn block_display_names(db: &BlockDb, version_idx: Option<usize>, name: &str) -> (String, String) {
    let (zh, en) = lookup_lang(db, version_idx, name);
    if !zh.is_empty() || !en.is_empty() {
        return (zh, en);
    }
    if let Some(short) = name.strip_prefix("minecraft:") {
        let dotted = format!("block.minecraft.{short}");
        let (zh, en) = lookup_lang(db, version_idx, &dotted);
        if !zh.is_empty() || !en.is_empty() {
            return (zh, en);
        }
    }
    (String::new(), String::new())
}

/// 查实体的中英文名。实体 id 形如 `minecraft:zombie`，键是 `entity.minecraft.zombie`。
pub(crate) fn entity_display_names(db: &BlockDb, version_idx: Option<usize>, id: &str) -> (String, String) {
    // 投影里的 id 可能不带命名空间，补上再查
    let full = if id.contains(':') { id.to_string() } else { format!("minecraft:{id}") };
    let dotted = format!("entity.{}", full.replace(':', "."));
    let (zh, en) = lookup_lang(db, version_idx, &dotted);
    if !zh.is_empty() || !en.is_empty() {
        return (zh, en);
    }
    // 有些语言表直接以 id 为键
    lookup_lang(db, version_idx, &full)
}

/// 查方块实体（箱子、告示牌等）的中英文名。
///
/// 它们与实体共用 `entity.*` 命名空间，但方块实体在语言表里也有
/// `block.*` 的别名，因此两种都试。
pub(crate) fn tile_display_names(db: &BlockDb, version_idx: Option<usize>, id: &str) -> (String, String) {
    let full = if id.contains(':') { id.to_string() } else { format!("minecraft:{id}") };
    for key in [
        format!("block.{}", full.replace(':', ".")),
        format!("entity.{}", full.replace(':', ".")),
        full.clone(),
    ] {
        let (zh, en) = lookup_lang(db, version_idx, &key);
        if !zh.is_empty() || !en.is_empty() {
            return (zh, en);
        }
    }
    (String::new(), String::new())
}

/// 完整分析：解码全部方块并统计。
#[tauri::command]
pub fn analyze_schematic(app: tauri::AppHandle, path: String) -> Result<AnalysisResult, String> {
    let loaded = open_db(Some(&app)).map_err(|e| e.to_string())?;
    let db = &loaded.db;
    let sc = litematic::Schematic::load(&path).map_err(|e| e.to_string())?;
    let a = sc.analyze();
    // 按投影自己的 dataVersion 选版本，语言表才与投影内容对得上
    let vi = db.pick_version(sc.summary().data_version);

    let mut blocks: Vec<BlockCount> = a
        .blocks_by_state
        .iter()
        .map(|(state, count)| {
            // 按名称聚合的计数用于展示，状态串用于区分具体朝向等
            let name = state.split('[').next().unwrap_or(state).to_string();
            let (zh_name, en_name) = block_display_names(db, vi, &name);
            BlockCount { name, state: state.clone(), count: *count, zh_name, en_name }
        })
        .collect();
    blocks.sort_by(|x, y| y.count.cmp(&x.count).then_with(|| x.state.cmp(&y.state)));

    // 实体 id 形如 `minecraft:zombie`，语言表里的键是 `entity.minecraft.zombie`
    let mut entities: Vec<EntityCount> = a
        .entities_by_id
        .iter()
        .map(|(id, count)| {
            let (zh_name, en_name) = entity_display_names(db, vi, id);
            EntityCount { id: id.clone(), count: *count, zh_name, en_name }
        })
        .collect();
    entities.sort_by(|x, y| y.count.cmp(&x.count).then_with(|| x.id.cmp(&y.id)));

    let mut tiles: Vec<EntityCount> = a
        .tile_entities_by_id
        .iter()
        .map(|(id, count)| {
            let (zh_name, en_name) = tile_display_names(db, vi, id);
            EntityCount { id: id.clone(), count: *count, zh_name, en_name }
        })
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

/// 一次最多渲染多少张方块图。
///
/// 前端一页 50 行、按 128 一批请求，正常远低于这个数；
/// 设上限是因为这是跨进程入口，参数由前端给出，不该接受无上限的工作量。
const MAX_ICON_BATCH: usize = 256;

/// 批量渲染方块图标。
///
/// 一次一批而不是一行一次：方块栏一页 50 行，逐行调用就是 50 次跨进程往返，
/// 界面会明显顿一下。
///
/// 单个方块画不出来（模型缺失、贴图缺失）只让那一项返回错误，
/// 不整批失败——某一行没有图标不该让整张表都空着。
#[tauri::command]
pub fn render_block_icons(
    app: tauri::AppHandle,
    data_version: i32,
    states: Vec<String>,
    size: Option<u32>,
) -> Result<Vec<BlockIconDto>, String> {
    if states.len() > MAX_ICON_BATCH {
        return Err(format!(
            "一次最多渲染 {MAX_ICON_BATCH} 张方块图，本次请求了 {} 张",
            states.len()
        ));
    }
    if states.is_empty() {
        return Ok(Vec::new());
    }
    let loaded = open_db(Some(&app)).map_err(|e| e.to_string())?;
    let db = &loaded.db;
    // 用投影自己的 dataVersion 选版本，语言表与贴图才与投影内容对得上
    let vi = db
        .pick_version(data_version)
        .ok_or_else(|| format!("方块库中没有数据版本 {data_version} 对应的版本"))?;
    let size = crate::block_icon::check_icon_size(size.unwrap_or(crate::block_icon::ICON_SIZE))
        .map_err(|e| e.to_string())?;

    Ok(states
        .into_iter()
        .map(|state| match crate::block_icon::render_data_url(db, vi, &state, size) {
            Ok(data_url) => BlockIconDto { state, data_url, error: String::new() },
            Err(e) => BlockIconDto { state, data_url: String::new(), error: e.to_string() },
        })
        .collect())
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
    // 版本名可能含路径分隔符或 `..`（导入资源包时未指定名称的话，
    // 包名会直接成为版本名），必须清理成一段安全文件名再拼接，
    // 否则写出路径会离开投影所在目录。
    let out_path = dir.join(format!(
        "{stem}_{}.litematic",
        crate::convert::safe_file_component(&target.name)
    ));

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
    // NBT 的字符串长度字段是 u16，写入时超限只能报错。在碰文件之前先校验：
    // 一是让用户拿到「哪一项太长」的可读提示，而不是等写盘阶段才失败
    // （那时的错误来自编码深层，容易被误当成文件损坏）；二是这类校验
    // 不该依赖磁盘状态。
    //
    // 这里按字符数粗筛，精确的字节数由 nbt 层按 modified UTF-8 判定。
    // 中文每字 3 字节；用 20000 字符做上限，留出足够余量又远低于 65535 字节。
    const MAX_META_CHARS: usize = 20000;
    for (label, v) in [
        ("名称", &name),
        ("作者", &author),
        ("描述", &description),
    ] {
        if let Some(s) = v {
            let n = s.chars().count();
            if n > MAX_META_CHARS {
                return Err(format!("{label}过长：{n} 个字符，上限 {MAX_META_CHARS} 个字符"));
            }
        }
    }

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

    /// 每个用例用独立的临时目录，避免并行执行时互相覆盖
    fn temp_state_dir(tag: &str) -> PathBuf {
        let mut d = std::env::temp_dir();
        d.push(format!(
            "sm-ui-state-{tag}-{}-{}",
            std::process::id(),
            // 用纳秒时间戳区分同一进程内的多次调用
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|v| v.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&d).expect("建临时目录失败");
        d
    }

    #[test]
    fn ui_state_round_trips_through_the_file() {
        let dir = temp_state_dir("rt");
        let path = dir.join(UI_STATE_FILE);
        let json = r#"{"version":1,"sidebarWidth":820,"dirs":["C:/D1"]}"#;

        write_ui_state_file(&path, json).expect("写入失败");
        let got = read_ui_state_file(&path).expect("读取失败");
        assert_eq!(got.as_deref(), Some(json), "写进去的应能原样读回来");

        // 覆盖写：第二次必须替换掉第一次，而不是追加或残留
        let json2 = r#"{"version":1,"sidebarWidth":666,"dirs":[]}"#;
        write_ui_state_file(&path, json2).expect("覆盖写入失败");
        assert_eq!(
            read_ui_state_file(&path).expect("读取失败").as_deref(),
            Some(json2),
            "第二次保存应替换旧内容"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn ui_state_missing_file_is_not_an_error() {
        let dir = temp_state_dir("missing");
        let path = dir.join(UI_STATE_FILE);
        // 首次启动：文件还不存在。这是正常情况，必须返回 None 而不是错误，
        // 否则每次首启都会弹出一个「读取界面状态失败」的告警。
        let got = read_ui_state_file(&path).expect("文件不存在不应视为错误");
        assert!(got.is_none(), "未保存过时应返回 None");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn ui_state_write_leaves_no_temp_file_behind() {
        let dir = temp_state_dir("tmp");
        let path = dir.join(UI_STATE_FILE);
        write_ui_state_file(&path, "{}").expect("写入失败");
        // 临时文件必须已被改名带走：残留会在下次写入时产生
        // 「目标已存在」之类的意外，也会让用户目录里留下垃圾
        assert!(!path.with_extension("json.tmp").exists(), "不应残留临时文件");
        let names: Vec<String> = std::fs::read_dir(&dir)
            .expect("列目录失败")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec![UI_STATE_FILE.to_string()], "目录里应只剩状态文件本身");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn ui_state_failed_write_keeps_previous_content() {
        let dir = temp_state_dir("fail");
        let path = dir.join(UI_STATE_FILE);
        let good = r#"{"version":1,"sidebarWidth":700}"#;
        write_ui_state_file(&path, good).expect("首次写入失败");

        // 用一个必然失败的目标模拟「写盘中途失败」：
        // 把临时文件的位置占成一个目录，写入就会失败。
        // 关键是**原有内容必须完好**——否则用户所有界面设置会被清零。
        std::fs::create_dir_all(path.with_extension("json.tmp")).expect("占位失败");
        let r = write_ui_state_file(&path, r#"{"version":1,"sidebarWidth":1}"#);
        assert!(r.is_err(), "临时文件写不进去时应报错，而不是静默成功");
        assert_eq!(
            read_ui_state_file(&path).expect("读取失败").as_deref(),
            Some(good),
            "写入失败后原有内容必须保持不变"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    // 注意：这里的路径必须与平台无关。
    // std::path::Path 按各平台的分隔符切分，在 Unix 上反斜杠只是普通字符，
    // 因此 Windows 风格的 "Z:\\x.litematic" 在 Linux 上会被整体当作文件名。
    // 测试只关心「文件不存在时的行为」，用纯文件名即可。
    /// 版本号形如 1.20.1 / 26.2 的文件名要被识别出来，
    /// 否则客户端 jar 会被当成模组导入，污染资源包清单。
    #[test]
    fn mc_version_names_are_detected() {
        for good in ["1.20.1", "1.20", "26.2", "26.1.2", "1.13.2"] {
            assert!(is_mc_version_name(good), "{good} 是版本号，应被识别");
        }
        for bad in [
            "fabric-api-0.92.2",   // 模组名里也有数字与点，但含字母
            "sodium",              // 纯字母
            "iris-1.7",            // 字母+数字
            "1.20.1-beta",         // 段里含非数字
            "",                    // 空
            "1",                   // 只有一段
            "a.b.c.d",             // 四段
        ] {
            assert!(!is_mc_version_name(bad), "{bad} 不是版本号，不应被识别");
        }
    }

    /// 不存在的目录要报错，而不是返回「导入 0 个」让人以为成功了。
    #[test]
    fn import_mods_folder_rejects_missing_dir() {
        let r = import_mods_folder("__sm_no_such_mods__".into(), 3465);
        assert!(r.is_err(), "目录不存在时应报错");
        let msg = r.unwrap_err();
        assert!(msg.contains("不是目录"), "错误信息应说明原因，实际：{msg}");
    }

    /// 目录里没有 jar 时要明确报错，而不是静默返回空结果。
    #[test]
    fn import_mods_folder_reports_when_no_jars() {
        let d = std::env::temp_dir().join(format!(
            "sm_mods_empty_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|v| v.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&d).unwrap();
        // 放一个非 jar 文件，确认它不会被误收
        std::fs::write(d.join("readme.txt"), b"x").unwrap();
        let r = import_mods_folder(d.to_string_lossy().into_owned(), 3465);
        assert!(r.is_err(), "目录里没有 jar 时应报错");
        assert!(r.unwrap_err().contains("没有可导入"));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn scan_missing_dir_returns_empty() {
        let r = scan_litematics("__sm_not_exist_dir__".into(), Some(2));
        assert!(r.items.is_empty());
        assert!(r.skipped_dirs > 0, "不存在的目录应计入跳过数");
    }

    /// 造一个本次测试专用的可写目录。
    ///
    /// 不用 `std::env::temp_dir()`：本仓库的 Windows 开发环境下系统临时目录
    /// 不可写（见 real_project.rs 与 backup.rs 的说明），那样这些用例在默认
    /// 环境下会直接 PermissionDenied 失败。目录名带进程号与纳秒，避免并发
    /// 跑测试时互相踩。
    fn mods_test_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("testtmp")
            .join(format!("sm_mods_{tag}_{}_{nanos}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("创建测试目录失败");
        d
    }

    /// mods 目录的过滤规则：非 jar 与客户端包都不该进待导入列表。
    ///
    /// 这些规则错了不会报错，只会静默少导入几个模组，因此必须直接测。
    #[test]
    fn collect_mod_jars_filters_out_non_mods() {
        let dir = mods_test_dir("scan");
        std::fs::create_dir_all(dir.join("子目录")).unwrap();
        std::fs::write(dir.join("readme.txt"), b"x").unwrap();
        std::fs::write(dir.join("1.20.1.jar"), b"x").unwrap();
        std::fs::write(dir.join("foo-client.jar"), b"x").unwrap();
        // 大小写不同也必须按客户端包跳过：Windows 上两者是同一个东西
        std::fs::write(dir.join("Bar-Client-1.0.jar"), b"x").unwrap();
        std::fs::write(dir.join("真模组.jar"), b"x").unwrap();
        std::fs::write(dir.join("another_mod.JAR"), b"x").unwrap();

        let scan = collect_mod_jars(&dir).expect("扫描失败");
        let kept: Vec<String> = scan
            .jars
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            kept,
            vec!["another_mod.JAR", "真模组.jar"],
            "只应留下真正的模组 jar（扩展名大小写不敏感）"
        );

        // 跳过的必须逐条回报，否则用户只能靠「数目对不上」发现
        let skipped: Vec<String> = scan
            .skipped
            .iter()
            .map(|(p, _)| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        for name in ["readme.txt", "1.20.1.jar", "foo-client.jar", "Bar-Client-1.0.jar"] {
            assert!(skipped.contains(&name.to_string()), "{name} 应被记为跳过");
        }
        assert!(
            !skipped.contains(&"子目录".to_string()),
            "目录不应计入跳过项（它不是文件）"
        );
        assert!(scan.unreadable.is_empty(), "正常目录不应有读不出的项");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 扫描结果必须稳定排序，否则同一次导入每次的原子上传顺序都不同。
    #[test]
    fn collect_mod_jars_is_sorted() {
        let dir = mods_test_dir("sort");
        for n in ["z.jar", "a.jar", "m.jar"] {
            std::fs::write(dir.join(n), b"x").unwrap();
        }
        let scan = collect_mod_jars(&dir).unwrap();
        let names: Vec<String> = scan
            .jars
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["a.jar", "m.jar", "z.jar"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 不存在的目录必须报错，而不是当成「没有模组」。
    #[test]
    fn collect_mod_jars_rejects_missing_dir() {
        let e = collect_mod_jars(Path::new("__sm_not_exist_mods__"))
            .unwrap_err()
            .to_string();
        assert!(e.contains("无法读取目录"), "应说明目录读不了：{e}");
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

    /// 超长元数据必须在写入之前被拒绝。
    ///
    /// NBT 的字符串长度字段是 u16：超限时若只截断长度字段，写出的文件
    /// 记录长度与实际字节数不符，后续字段全部错位，而写入路径的终点是
    /// 覆盖用户的 .litematic。因此这里要求「明确报错」而不是「写出坏文件」。
    #[test]
    fn oversized_metadata_is_rejected_before_writing() {
        let too_long = "啊".repeat(20_001);
        let err = save_metadata(
            "__sm_not_exist__.litematic".into(),
            None,
            None,
            Some(too_long),
        )
        .expect_err("超长描述应被拒绝");
        assert!(
            err.contains("过长"),
            "错误信息应指出超长，实际：{err}"
        );
    }

    /// 三个字段都要校验，不能只检查描述。
    #[test]
    fn oversized_name_and_author_are_rejected_too() {
        let long = "x".repeat(20_001);
        for (label, name, author) in [
            ("名称", Some(long.clone()), None),
            ("作者", None, Some(long.clone())),
        ] {
            let err = save_metadata(
                "__sm_not_exist__.litematic".into(),
                name,
                author,
                None,
            )
            .expect_err(&format!("超长{label}应被拒绝"));
            assert!(
                err.contains("过长") && err.contains(label),
                "错误信息应指明是哪一项超长，实际：{err}"
            );
        }
    }

    /// 恰好等于上限时不应因长度被拒。
    ///
    /// 路径不存在，因此会以「文件不存在」失败——关键是不能再报「过长」，
    /// 否则说明上限被判成了 >=。
    #[test]
    fn metadata_at_exact_limit_is_not_rejected_for_length() {
        let ok = "x".repeat(20_000);
        let err = save_metadata(
            "__sm_not_exist__.litematic".into(),
            None,
            None,
            Some(ok),
        )
        .expect_err("文件不存在应报错");
        assert!(
            !err.contains("过长"),
            "恰好达上限不应被判超长，实际：{err}"
        );
        assert!(err.contains("不存在"), "应报文件不存在，实际：{err}");
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
            source_dir: "/j".into(),
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

    // ---- delete_pack 的文件名校验 ----
    //
    // 这是安全校验：`file` 来自清单，而清单是可被手工编辑的文本文件。
    // 校验不通过时必须直接拒绝，因此下面这些用例**不会触碰磁盘**——
    // 合法文件名才会走到读写清单那一步，而那些需要真实 packs 目录，
    // 不适合在单元测试里构造。
    //
    // 用例必须在 Windows 与 Linux 上得到**相同**结论：CI 的单元测试跑在
    // Linux 上，本地开发在 Windows 上。最初用 `Path::components()` 实现时，
    // `C:x.pack` 在 Windows 被判为盘符前缀而拒绝、在 Linux 只是普通字符而
    // 放过，于是本地通过、CI 失败。因此实现改为纯字符串判断。
    // 下面把带盘符的名字列进「必须拒绝」的清单，若有人改回组件解析，
    // Linux 上的 CI 会立刻失败。

    /// 各种「不是单一文件名」的输入都必须被拒绝。
    #[test]
    fn delete_pack_rejects_path_like_names() {
        let bad = [
            // 上级目录
            "..",
            "../x.pack",
            "..\\x.pack",
            "a/../../x.pack",
            // 子目录
            "sub/x.pack",
            "sub\\x.pack",
            // 绝对路径（Windows 盘符与 Unix 根）
            "C:\\Windows\\x.pack",
            "C:/Windows/x.pack",
            "/etc/x.pack",
            "\\\\server\\share\\x.pack",
            // 盘符相对路径：不含 / \ ..，朴素的字符串检查会放过，
            // 但 Path::join 会把基础目录整个替换掉
            "C:x.pack",
            "D:evil.pack",
            // 当前目录前缀
            "./x.pack",
            // 空串与纯空白
            "",
            " ",
        ];
        for name in bad {
            let r = delete_pack(name.to_string());
            assert!(
                r.is_err(),
                "{name:?} 应被拒绝，实际返回 {r:?}"
            );
            let msg = r.unwrap_err();
            assert!(
                msg.contains("不合法"),
                "{name:?} 的错误信息应说明文件名不合法，实际：{msg}"
            );
        }
    }

    /// 正常文件名不应被校验拦住。
    ///
    /// 这些输入会继续走到读写清单，因此会以「清单里没有该资源包」之类的
    /// 原因失败——关键是错误**不是**「文件名不合法」。
    #[test]
    fn delete_pack_accepts_plain_file_names() {
        for name in ["a.pack", "1.20.1.pack", "my-pack_v2.pack", "中文名.pack"] {
            let r = delete_pack(name.to_string());
            if let Err(e) = r {
                assert!(
                    !e.contains("不合法"),
                    "{name:?} 是合法文件名，不应被判为不合法：{e}"
                );
            }
        }
    }
}

/// 删除某个资源包：同时移除清单记录与磁盘文件。
///
/// 顺序是先删文件再改清单：若反过来，删文件失败会留下一条指向不存在
/// 文件的记录（界面显示缺失）；而先删文件后写清单失败时，残留的是
/// 一条 `exists=false` 的记录，重新导入即可修复。
#[tauri::command]
pub fn delete_pack(file: String) -> Result<(), String> {
    // 只接受单一文件名，不接受任何路径形式。
    //
    // 这里刻意用**显式且与平台无关**的判断，而不是 `Path::components()`：
    // 后者在 Windows 上把 `C:` 解析成盘符前缀、在 Unix 上只是一个普通字符，
    // 于是同一个输入在两个系统上得到不同结论。而清单是跨平台共用的文本
    // 文件，同一份内容可能在两个系统上被读取，校验结果必须一致。
    //
    // 危险点（Windows 实测）：
    //   Path::new(r"C:\app\packs").join("C:x.pack") == "C:x.pack"
    // `join` 遇到带盘符前缀的路径会**直接替换掉基础目录**，删除目标就跑出
    // 了 packs 目录。而 `C:x.pack` 不含 `/`、`\`、`..`，朴素的字符串检查
    // 会放过它。
    //
    // 因此拒绝：空串、纯空白、含路径分隔符、含冒号、以及 `.` 与 `..`。
    // 程序自己生成的文件名源自 jar 名加 `.pack` 后缀，不含这些字符，
    // 所以不会误伤正常用法。
    let bad = file.is_empty()
        || file.trim().is_empty()
        || file == "."
        || file == ".."
        || file.contains('/')
        || file.contains('\\')
        || file.contains(':');
    if bad {
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
                source_dir: e.source_dir,
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
    Ok(PackListDto {
        packs_dir: dir.to_string_lossy().to_string(),
        packs,
        load_failures,
        skipped,
    })
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
            // 来源目录从 jar 路径推出来。从 mods 文件夹批量导入时，
            // 界面靠这个字段把同一批包归成一组。
            source_dir: Path::new(&jar_path)
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
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

/// 从一个 mods 文件夹批量导入其中所有模组。
///
/// 用户要求「模组包要选择到 mods 文件夹，导入所有模组方块贴图」——
/// 逐个选 jar 在装了上百个模组时不可行，因此提供整目录导入。
///
/// 行为约定：
/// - 只处理目录**本身**下的 `.jar`，不递归。子目录里是模组自己的资源，
///   不是可导入的模组包；递归会撞上大量非模组文件。
/// - **单个失败不中断整批**：模组 jar 五花八门，其中一个坏掉不该让
///   其余几十个都导不进来。失败项连同原因一并回报，由界面列出。
/// - 跳过 MC 客户端 jar（名字形如 `1.20.1.jar` 或含 `client`）：
///   它们属于版本包而非模组，混进来会把整份清单搞乱。
#[tauri::command]
pub fn import_mods_folder(
    dir: String,
    data_version: i32,
) -> Result<ModsImportDto, String> {
    let root = Path::new(&dir);
    if !root.is_dir() {
        return Err(format!("{dir} 不是目录"));
    }
    let scan = collect_mod_jars(root).map_err(|e| e.to_string())?;
    if scan.jars.is_empty() {
        return Err(format!("{dir} 里没有可导入的模组 jar"));
    }

    let mut imported: Vec<String> = Vec::new();
    let mut failed: Vec<(String, String)> = scan
        .unreadable
        .iter()
        .map(|(p, why)| (p.display().to_string(), why.clone()))
        .collect();
    for jar in &scan.jars {
        let label = jar
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        match import_jar(
            jar.to_string_lossy().into_owned(),
            None,
            crate::pack_registry::PackKind::Mod.as_i32(),
            data_version,
        ) {
            Ok(_) => imported.push(label),
            Err(e) => failed.push((label, e)),
        }
    }

    Ok(ModsImportDto {
        dir,
        imported,
        failed: failed
            .into_iter()
            .map(|(file, reason)| UnreadableFileDto { path: file, reason })
            .collect(),
        // 有意跳过的条目也要回报：名字里恰好含 client 的模组会被判成客户端包，
        // 静默丢掉的话用户只能靠「数目对不上」才发现。
        skipped: scan
            .skipped
            .into_iter()
            .map(|(path, reason)| UnreadableFileDto {
                path: path.to_string_lossy().into_owned(),
                reason,
            })
            .collect(),
    })
}

/// 一次 mods 目录扫描的结果
#[derive(Debug, Default)]
struct ModScan {
    /// 待导入的模组 jar，按路径排序，结果稳定
    jars: Vec<PathBuf>,
    /// 有意跳过的条目及原因
    skipped: Vec<(PathBuf, String)>,
    /// 目录项读不出名字等失败，必须上报而不是 `.flatten()` 掉
    unreadable: Vec<(PathBuf, String)>,
}

/// 扫描 mods 目录，挑出待导入的模组 jar。
///
/// 单独成函数是为了能脱离 AppHandle 直接测：过滤规则（非 jar、客户端 jar、
/// 版本号命名的客户端 jar）都在这里，它们错了不会报错、只会静默少导入。
fn collect_mod_jars(dir: &Path) -> AppResult<ModScan> {
    let mut out = ModScan::default();
    let entries = std::fs::read_dir(dir)
        .map_err(|e| AppError::Parse(format!("无法读取目录 {}：{e}", dir.display())))?;
    for e in entries {
        // 目录项自身读失败时不能丢掉：那意味着有一个文件我们不知道是什么，
        // 与「它不重复」是两回事，因此记进 unreadable 上报。
        let e = match e {
            Ok(e) => e,
            Err(err) => {
                out.unreadable
                    .push((dir.to_path_buf(), format!("读取目录项失败：{err}")));
                continue;
            }
        };
        let p = e.path();
        if !p.is_file() {
            continue;
        }
        if !p
            .extension()
            .map(|x| x.eq_ignore_ascii_case("jar"))
            .unwrap_or(false)
        {
            out.skipped.push((p, "不是 .jar 文件".into()));
            continue;
        }
        let stem = p
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        // 客户端 jar 属于版本包而不是模组。大小写不敏感：Windows 上
        // `Foo-Client.jar` 与 `foo-client.jar` 是同一个东西。
        if stem.to_ascii_lowercase().contains("client") {
            out.skipped.push((p, "名字含 client，按客户端包跳过".into()));
            continue;
        }
        if is_mc_version_name(&stem) {
            out.skipped.push((p, "名字是游戏版本号，按客户端包跳过".into()));
            continue;
        }
        out.jars.push(p);
    }
    out.jars.sort();
    out.skipped.sort();
    out.unreadable.sort();
    Ok(out)
}

/// 判断一个文件名主干是否形如 Minecraft 版本号（`1.20.1`、`26.2`）。
///
/// 客户端 jar 常以版本号命名，混进模组批量导入会污染清单。
/// 判据：由点分隔的 2~3 段全数字。不用正则，避免为此引入依赖。
fn is_mc_version_name(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() < 2 || parts.len() > 3 {
        return false;
    }
    parts
        .iter()
        .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
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

// ---------------------------------------------------------------- 目录栏文件操作
//
// 这四个命令都作用于用户在目录栏里勾选的文件。共同点：
// 路径来自前端，属于不可信输入，因此统一先经 file_ops::check_files
// 校验存在性与类型，再执行具体动作。
//
// 与 delete_pack 的差别：后者只接受单一文件名并限制在 packs 目录内；
// 这里的文件由用户在任意目录中选择，因此不能限制目录，
// 但删除一律走回收站，把「误删」的代价限制在可还原范围内。

/// 删除选中的文件（移入系统回收站，不永久删除）。
#[tauri::command]
pub fn delete_files(paths: Vec<String>) -> Result<FileOpOutcome, String> {
    let files = crate::file_ops::check_files(&paths).map_err(|e| e.to_string())?;
    crate::file_ops::delete_to_trash(&files).map_err(|e| e.to_string())?;
    let names: Vec<String> = files.iter().map(|f| f.name.clone()).collect();
    let _ = backup::append_log(
        &backup::backup_root(&exe_dir()),
        &format!("删除到回收站 {} 个文件：{}", names.len(), names.join("、")),
    );
    Ok(FileOpOutcome {
        total_bytes: files.iter().map(|f| f.size).sum(),
        names,
        written: Vec::new(),
        dest_dir: String::new(),
    })
}

/// 把选中的文件复制到系统剪贴板，供用户在资源管理器里粘贴。
#[tauri::command]
pub fn copy_files_to_clipboard(paths: Vec<String>) -> Result<FileOpOutcome, String> {
    let files = crate::file_ops::check_files(&paths).map_err(|e| e.to_string())?;
    crate::file_ops::copy_paths_to_clipboard(&files).map_err(|e| e.to_string())?;
    Ok(FileOpOutcome {
        total_bytes: files.iter().map(|f| f.size).sum(),
        names: files.iter().map(|f| f.name.clone()).collect(),
        written: Vec::new(),
        dest_dir: String::new(),
    })
}

/// 把选中的文件复制到指定目录。同名不覆盖，改为追加序号。
#[tauri::command]
pub fn copy_files_to_dir(paths: Vec<String>, dest_dir: String) -> Result<FileOpOutcome, String> {
    let files = crate::file_ops::check_files(&paths).map_err(|e| e.to_string())?;
    let dest = PathBuf::from(&dest_dir);
    let written = crate::file_ops::copy_into(&files, &dest).map_err(|e| e.to_string())?;
    let names: Vec<String> = files.iter().map(|f| f.name.clone()).collect();
    let _ = backup::append_log(
        &backup::backup_root(&exe_dir()),
        &format!(
            "复制 {} 个文件到 {}：{}",
            names.len(),
            dest.display(),
            names.join("、")
        ),
    );
    Ok(FileOpOutcome {
        total_bytes: files.iter().map(|f| f.size).sum(),
        names,
        written: written
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
        dest_dir: dest.to_string_lossy().into_owned(),
    })
}

/// 在系统文件管理器中定位到一个文件（打开其所在目录并选中该文件）。
///
/// 只处理**一个**文件：资源管理器的「选中」语义只对一个目标有意义，
/// 传多个时行为不确定，因此这里明确取第一个由前端决定。
#[tauri::command]
pub fn reveal_in_file_manager(path: String) -> Result<(), String> {
    let p = Path::new(&path);
    crate::file_ops::reveal_in_file_manager(p).map_err(|e| e.to_string())
}

/// 在文件管理器里打开一个目录。
///
/// 目录不存在时先创建：用户还没导入过资源包时 `packs` 目录并不存在，
/// 而「打开资源包目录」按钮正是那时候最需要能用的——他要往里放东西。
#[tauri::command]
pub fn open_directory(path: String) -> Result<(), String> {
    let p = Path::new(&path);
    std::fs::create_dir_all(p).map_err(|e| format!("创建目录 {} 失败：{e}", p.display()))?;
    crate::file_ops::open_directory(p).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------- 界面状态记忆

/// 界面状态文件名。内容是一段前端自己解释的 JSON。
const UI_STATE_FILE: &str = "ui-state.json";

/// 读取界面状态文件；文件不存在返回 `None`（首次启动的正常情况，不是错误）。
///
/// 与定位路径分开：路径依赖运行时的 AppHandle，而读写逻辑本身
/// （尤其是「不存在」与「读失败」的区分）是可以独立验证的。
fn read_ui_state_file(path: &Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("读取界面状态 {} 失败：{e}", path.display())),
    }
}

/// 原子写入界面状态文件。
///
/// 先写同目录的临时文件再改名：直接覆盖时若中途失败（磁盘满、进程被杀）
/// 会留下半截 JSON，下次启动解析失败就等于把用户所有界面设置清零。
/// 改成同目录内的改名在主流文件系统上是原子的，因此读到的要么是旧内容、
/// 要么是完整的新内容。
fn write_ui_state_file(path: &Path, json: &str) -> Result<(), String> {
    // 临时文件必须与目标同目录：跨目录改名不保证原子，甚至可能失败
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json.as_bytes())
        .map_err(|e| format!("写入 {} 失败：{e}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .map_err(|e| format!("替换 {} 失败：{e}", path.display()))
}

/// 界面状态的存放路径。
///
/// 放在系统的应用配置目录（`%APPDATA%\<bundle-id>` 等）而不是程序目录：
/// 程序可能装在 Program Files 这类只读位置，而「记住界面」失败不该影响使用。
/// 用 Tauri 核心的 PathResolver 取，不需要额外插件。
fn ui_state_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("无法定位配置目录：{e}"))?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("无法创建配置目录 {}：{e}", dir.display()))?;
    Ok(dir.join(UI_STATE_FILE))
}

/// 读取上次保存的界面状态；从未保存过时返回 None（不是错误）。
#[tauri::command]
pub fn load_ui_state(app: tauri::AppHandle) -> Result<Option<String>, String> {
    read_ui_state_file(&ui_state_path(&app)?)
}

/// 保存界面状态。
#[tauri::command]
pub fn save_ui_state(app: tauri::AppHandle, json: String) -> Result<(), String> {
    write_ui_state_file(&ui_state_path(&app)?, &json)
}

