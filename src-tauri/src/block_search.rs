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

/// 逐版本遍历方块，把命中项按方块名合并成一条并汇总版本。
///
/// `rank_of` 收到（已转小写的完整方块 ID、短名、中文名、英文名），返回命中
/// 优先级，`None` 表示不命中。检索与「列出全部」只差这一个判据，合并、去重、
/// 版本汇总的规则完全一样；分两处写会让同一方块在两条路径上拿到不同的版本列表。
fn collect<F>(db: &BlockDb, rank_of: F) -> Vec<(u8, BlockHit)>
where
    F: Fn(&str, &str, &str, &str) -> Option<u8>,
{
    // name -> (rank, hit)
    let mut found: HashMap<String, (u8, BlockHit)> = HashMap::new();

    for (vi, v) in db.versions.iter().enumerate() {
        let langs = lang_index(db, vi);
        for b in &v.blocks {
            let short = b.name.split_once(':').map(|(_, n)| n).unwrap_or(&b.name);
            let (zh, en) = langs.get(&b.name).cloned().unwrap_or_default();
            let Some(r) = rank_of(&b.name.to_lowercase(), &short.to_lowercase(), &zh, &en) else {
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
    out
}

/// 跨版本检索方块。
///
/// query 为空时返回空结果——用户还没输入关键字时应当用 [`list_all`] 取全量，
/// 而不是把整库方块一次性倒给界面。
pub fn search(db: &BlockDb, query: &str, limit: usize) -> Vec<BlockHit> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }

    let mut out = collect(db, |name, short, zh, en| rank(name, short, zh, en, &q));
    // 先按命中优先级，再按名称，保证结果稳定
    out.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.name.cmp(&b.1.name)));
    out.into_iter().take(limit.max(1)).map(|(_, h)| h).collect()
}

/// 列出方块库中的全部方块，按方块 ID 升序。
///
/// 供方块选择列表在用户还没输入关键字时使用：挑一个方块需要先看到全貌，
/// 只给空列表等于要求用户先知道名字。排序按 ID 而不是相关度——没有查询就没有
/// 相关度，按 ID 排是稳定且可预期的顺序。
///
/// 不截断：调用方要的就是完整清单，截一半会让排在后面的方块永远挑不到。
pub fn list_all(db: &BlockDb) -> Vec<BlockHit> {
    let mut out = collect(db, |_, _, _, _| Some(0));
    out.sort_by(|a, b| a.1.name.cmp(&b.1.name));
    out.into_iter().map(|(_, h)| h).collect()
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

    #[test]
    fn list_all_lists_every_block_across_versions() {
        let db = sample();
        let hits = list_all(&db);
        let names: Vec<&str> = hits.iter().map(|h| h.name.as_str()).collect();
        // 两个版本合起来共 4 个不同的方块名；没有查询也要全部列出，
        // 否则用户不输入关键字时就挑不到任何方块
        assert_eq!(
            names,
            vec![
                "minecraft:copper_block",
                "minecraft:oak_log",
                "minecraft:stone",
                "minecraft:stone_bricks"
            ],
            "应按方块 ID 升序列出全部方块，实际 {names:?}"
        );
    }

    #[test]
    fn list_all_merges_versions_of_the_same_block() {
        let db = sample();
        let stone = list_all(&db)
            .into_iter()
            .find(|h| h.name == "minecraft:stone")
            .expect("应有石头");
        assert_eq!(stone.versions.len(), 2, "两个版本都有的方块只应出现一次并列出两个版本");
        // 与检索给出同一份版本列表：两处各自合并会让同一方块在两条路径上不一致
        let by_search = search(&db, "minecraft:stone", 20)
            .into_iter()
            .find(|h| h.name == "minecraft:stone")
            .expect("检索也应命中石头");
        assert_eq!(stone.versions, by_search.versions);
    }

    #[test]
    fn list_all_keeps_blocks_without_display_name() {
        let db = db_with(vec![version(
            "1.20.1",
            3465,
            &[("minecraft:stone", "石头", "Stone"), ("mod:mystery", "", "")],
            false,
        )]);
        let hits = list_all(&db);
        let mystery = hits
            .iter()
            .find(|h| h.name == "mod:mystery")
            .expect("没有显示名称的方块也要列出，否则它在选择列表里永远不出现");
        assert!(mystery.zh.is_empty());
        assert!(mystery.en.is_empty());
    }

    #[test]
    fn list_all_of_empty_library_is_empty() {
        assert!(list_all(&db_with(Vec::new())).is_empty());
    }
}
