//! 方块检索：跨版本按名称查找方块。
//!
//! 供界面上的方块选择使用。支持三种匹配目标：完整 ID（minecraft:stone）、
//! 去掉命名空间的短名（stone）、以及中文名（石头）。
//!
//! 同一方块在多个版本中都存在时只返回一次，并附带可选的版本列表，
//! 否则结果里会出现几十条同名条目，无法挑选。

use crate::blockdb::BlockDb;
use std::collections::HashMap;

/// 一条检索结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockHit {
    /// 完整方块 ID
    pub name: String,
    /// 中文名，可能为空
    pub zh: String,
    /// 英文名，可能为空
    pub en: String,
    /// 出现该方块的版本（下标 + 名称 + dataVersion），按 dataVersion 升序
    pub versions: Vec<BlockVersion>,
}

/// 方块所在的某个版本
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockVersion {
    pub index: usize,
    pub data_version: i32,
}

/// 版本选项，供界面上的版本下拉使用
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionOption {
    pub index: usize,
    pub name: String,
    pub data_version: i32,
    pub is_overlay: bool,
    pub block_count: usize,
}

/// 列出全部可用版本，按 dataVersion 升序（叠加包排在最后）
pub fn version_options(db: &BlockDb) -> Vec<VersionOption> {
    let mut out: Vec<VersionOption> = db
        .versions
        .iter()
        .enumerate()
        .map(|(i, v)| VersionOption {
            index: i,
            name: v.name.clone(),
            data_version: v.data_version,
            is_overlay: v.is_overlay,
            block_count: v.blocks.len(),
        })
        .collect();
    out.sort_by(|a, b| {
        a.is_overlay
            .cmp(&b.is_overlay)
            .then_with(|| a.data_version.cmp(&b.data_version))
    });
    out
}

/// 为某个版本建立 方块名 -> (中文名, 英文名) 的索引。
///
/// VersionDef::display_name 是线性查找，逐方块调用会退化成平方复杂度，
/// 因此这里先建索引。`pub(crate)`：版本转换也要按同一套索引取中文名，
/// 两处各建一份就会出现同一方块在检索与转换里显示不同名字。
pub(crate) fn lang_index(db: &BlockDb, version_idx: usize) -> HashMap<String, (String, String)> {
    let mut map = HashMap::new();
    if let Some(v) = db.versions.get(version_idx) {
        for e in &v.langs {
            map.insert(e.block_name.clone(), (e.zh.clone(), e.en.clone()));
        }
    }
    map
}

/// 把库里所有版本的语言表合并成一张，作为最后一级兜底。
///
/// 为什么需要它：单个版本的语言表未必带中文——中文来自随资源索引分发的
/// 语言文件，用户机器上不一定有，构建那个版本时取不到就整张表没有中文。
/// 而方块检索是遍历全部版本的，同一个方块在别处可能就有名字；版本转换
/// 原先只查源版本与目标版本两张表，于是在「只有一个版本带中文」的库上
/// 整列英文，看起来像转换丢了名字。
///
/// 合并顺序按版本下标升序、**先入为主**：同一方块在多个版本里都有中文时
/// 保留先遇到的那个，结果与构建顺序无关，可重复。英文名同一方块取第一个
/// 非空的，中文缺失的版本不会把已有的中文覆盖成空。
pub(crate) fn merged_lang_index(db: &BlockDb) -> HashMap<String, (String, String)> {
    let mut map: HashMap<String, (String, String)> = HashMap::new();
    for v in &db.versions {
        for e in &v.langs {
            let slot = map.entry(e.block_name.clone()).or_default();
            if slot.0.is_empty() && !e.zh.is_empty() {
                slot.0 = e.zh.clone();
            }
            if slot.1.is_empty() && !e.en.is_empty() {
                slot.1 = e.en.clone();
            }
        }
    }
    map
}

/// 命中的优先级，越小越靠前
fn rank(name: &str, short: &str, zh: &str, en: &str, q: &str) -> Option<u8> {
    if name == q || short == q {
        return Some(0);
    }
    if zh == q || en.eq_ignore_ascii_case(q) {
        return Some(1);
    }
    if name.starts_with(q) || zh.starts_with(q) || en.to_lowercase().starts_with(q) {
        return Some(2);
    }
    if name.contains(q) || zh.contains(q) || en.to_lowercase().contains(q) {
        return Some(3);
    }
    None
}

/// 跨版本检索方块。
///
/// query 为空时返回空结果——调用方应改用版本选项先行筛选，
/// 否则会把整库方块一次性倒给界面。
pub fn search(db: &BlockDb, query: &str, limit: usize) -> Vec<BlockHit> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }

    // name -> (rank, hit)
    let mut found: HashMap<String, (u8, BlockHit)> = HashMap::new();

    for (vi, v) in db.versions.iter().enumerate() {
        let langs = lang_index(db, vi);
        for b in &v.blocks {
            let short = b.name.split_once(':').map(|(_, n)| n).unwrap_or(&b.name);
            let (zh, en) = langs.get(&b.name).cloned().unwrap_or_default();
            let Some(r) = rank(&b.name.to_lowercase(), &short.to_lowercase(), &zh, &en, &q) else {
                continue;
            };
            match found.get_mut(&b.name) {
                Some((best, hit)) => {
                    if r < *best {
                        *best = r;
                    }
                    hit.versions.push(BlockVersion {
                        index: vi,
                        data_version: v.data_version,
                    });
                }
                None => {
                    found.insert(
                        b.name.clone(),
                        (
                            r,
                            BlockHit {
                                name: b.name.clone(),
                                zh,
                                en,
                                versions: vec![BlockVersion {
                                    index: vi,
                                    data_version: v.data_version,
                                }],
                            },
                        ),
                    );
                }
            }
        }
    }

    let mut out: Vec<(u8, BlockHit)> = found.into_values().collect();
    for (_, hit) in out.iter_mut() {
        hit.versions.sort_by_key(|v| v.data_version);
        hit.versions.dedup();
    }
    // 先按命中优先级，再按名称，保证结果稳定
    out.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.name.cmp(&b.1.name)));
    out.into_iter().take(limit.max(1)).map(|(_, h)| h).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blockdb::{BlockDef, LangEntry, Variant, VersionDef};

    fn version(name: &str, dv: i32, blocks: &[(&str, &str, &str)], overlay: bool) -> VersionDef {
        VersionDef {
            name: name.into(),
            data_version: dv,
            lang_idx: 0,
            texture_by_path: HashMap::new(),
            blocks: blocks
                .iter()
                .map(|(n, _, _)| BlockDef {
                    name: (*n).into(),
                    variants: vec![Variant {
                        when: String::new(),
                        model_idx: 0,
                        rot_x_div90: 0,
                        rot_y_div90: 0,
                        uvlock: false,
                    }],
                    multipart: Vec::new(),
                })
                .collect(),
            langs: blocks
                .iter()
                .map(|(n, zh, en)| LangEntry {
                    block_name: (*n).into(),
                    zh: (*zh).into(),
                    en: (*en).into(),
                })
                .collect(),
            is_overlay: overlay,
        }
    }

    fn db_with(versions: Vec<VersionDef>) -> BlockDb {
        BlockDb {
            strings: Vec::new(),
            langs: Vec::new(),
            textures: Vec::new(),
            models: Vec::new(),
            versions,
            pixels: Vec::new(),
        }
    }

    fn sample() -> BlockDb {
        db_with(vec![
            version(
                "1.20.1",
                3465,
                &[
                    ("minecraft:stone", "石头", "Stone"),
                    ("minecraft:stone_bricks", "石砖", "Stone Bricks"),
                    ("minecraft:oak_log", "橡木原木", "Oak Log"),
                ],
                false,
            ),
            version(
                "1.21",
                3953,
                &[("minecraft:stone", "石头", "Stone"), ("minecraft:copper_block", "铜块", "Block of Copper")],
                false,
            ),
        ])
    }

    #[test]
    fn search_by_chinese_name() {
        let db = sample();
        // 完全匹配中文名：只应命中石头本身。
        // 注意「石砖」并不包含「石头」，二者没有子串关系。
        let exact = search(&db, "石头", 20);
        assert_eq!(exact.len(), 1, "只应命中石头，实际 {:?}", exact.iter().map(|h| &h.name).collect::<Vec<_>>());
        assert_eq!(exact[0].name, "minecraft:stone");
        assert_eq!(exact[0].zh, "石头");
    }

    #[test]
    fn search_by_chinese_prefix_matches_several() {
        let db = sample();
        // 前缀「石」应同时命中石头与石砖
        let hits = search(&db, "石", 20);
        let names: Vec<&str> = hits.iter().map(|h| h.name.as_str()).collect();
        assert!(names.contains(&"minecraft:stone"), "实际 {names:?}");
        assert!(names.contains(&"minecraft:stone_bricks"), "实际 {names:?}");
        // 完全匹配的应排在前面
        assert_eq!(names[0], "minecraft:stone");
    }

    #[test]
    fn search_by_short_and_full_id() {
        let db = sample();
        let by_short = search(&db, "oak_log", 20);
        assert_eq!(by_short.len(), 1);
        assert_eq!(by_short[0].name, "minecraft:oak_log");

        let by_full = search(&db, "minecraft:oak_log", 20);
        assert_eq!(by_full[0].name, "minecraft:oak_log");
    }

    #[test]
    fn search_is_case_insensitive() {
        let db = sample();
        let a = search(&db, "STONE", 20);
        let b = search(&db, "stone", 20);
        assert!(!a.is_empty());
        assert_eq!(a.len(), b.len());
    }

    #[test]
    fn same_block_across_versions_is_merged() {
        let db = sample();
        let hits = search(&db, "minecraft:stone", 20);
        let stone = hits.iter().find(|h| h.name == "minecraft:stone").expect("应有石头");
        assert_eq!(stone.versions.len(), 2, "存在于两个版本时应合并为一条，并列出两个版本");
        assert_eq!(stone.versions[0].data_version, 3465, "版本应按 dataVersion 升序");
        assert_eq!(stone.versions[1].data_version, 3953);
    }

    #[test]
    fn results_are_ranked_exact_first() {
        let db = sample();
        let hits = search(&db, "stone", 20);
        // 完全匹配短名的 minecraft:stone 应排在 minecraft:stone_bricks 之前
        let i_stone = hits.iter().position(|h| h.name == "minecraft:stone").unwrap();
        let i_bricks = hits.iter().position(|h| h.name == "minecraft:stone_bricks").unwrap();
        assert!(i_stone < i_bricks, "完全匹配应在前，实际 {i_stone} vs {i_bricks}");
    }

    #[test]
    fn empty_query_returns_nothing() {
        let db = sample();
        assert!(search(&db, "", 20).is_empty());
        assert!(search(&db, "   ", 20).is_empty(), "全空白也视为空查询");
    }

    #[test]
    fn limit_is_respected() {
        let db = sample();
        assert_eq!(search(&db, "stone", 1).len(), 1);
        // limit 为 0 时至少返回一条，避免界面空白
        assert_eq!(search(&db, "stone", 0).len(), 1);
    }

    #[test]
    fn version_options_sorted_with_overlays_last() {
        let db = db_with(vec![
            version("base-1.21", 3953, &[], false),
            version("base-1.20.1", 3465, &[], false),
            version("modpack", -1, &[], true),
        ]);
        let opts = version_options(&db);
        assert_eq!(opts[0].data_version, 3465);
        assert_eq!(opts[1].data_version, 3953);
        assert!(opts[2].is_overlay, "叠加包应排在最后");
    }

    #[test]
    fn no_match_returns_empty() {
        let db = sample();
        assert!(search(&db, "不存在的方块名", 20).is_empty());
    }
}
