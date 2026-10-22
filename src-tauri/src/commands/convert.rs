//! 投影版本转换：列出目标版本、分析缺失方块并把结果写到新文件。

use super::dto::{ConvertItemDto, ConvertTargetDto, ConvertOutcomeDto};
use super::shared::open_db;
use std::path::Path;


/// 列出可作为转换目标的版本
#[tauri::command]
pub fn convert_targets(app: tauri::AppHandle, schematic_path: String) -> Result<Vec<ConvertTargetDto>, String> {
    let loaded = open_db(&app).map_err(|e| e.to_string())?;
    let db = &loaded.db;
    let sc = crate::schematic::litematic::Schematic::load(&schematic_path).map_err(|e| e.to_string())?;
    let dv = sc.summary().data_version;
    Ok(crate::schematic::convert::targets(db, dv)
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
    let loaded = open_db(&app).map_err(|e| e.to_string())?;
    let db = &loaded.db;
    let sc = crate::schematic::litematic::Schematic::load(&schematic_path).map_err(|e| e.to_string())?;

    // 源方块的中文名要按**投影自己的版本**查：这些方块在目标版本里已经
    // 没有了，只有原版本的语言表里才有它们的名字。
    //
    // pick_version 取的是「最接近投影版本的基础版本」，库里只要有一个基础
    // 版本就不会落空；真要落到 unwrap_or 上（库里没有任何基础版本），
    // 那时用户也选不出目标版本，这段回退实际上走不到——留着只是为了不 panic。
    // target_index 来自前端、可能越界，但 lang_index 取不到版本时返回空表。
    let source_idx = db.pick_version(sc.summary().data_version).unwrap_or(target_index);

    Ok(convert_items_with_zh(
        crate::schematic::convert::analyze(&sc, db, target_index),
        &crate::block::search::lang_index(db, source_idx),
        &crate::block::search::lang_index(db, target_index),
    ))
}

/// 给转换条目补上中英文名。
///
/// 单独成函数是为了能直接测：真正的命令要 AppHandle 才能取方块库，
/// 而这里只依赖两张语言表，用构造出来的表就能覆盖各级回退。
fn convert_items_with_zh(
    items: Vec<crate::schematic::convert::ConvertItem>,
    source_langs: &std::collections::HashMap<String, (String, String)>,
    target_langs: &std::collections::HashMap<String, (String, String)>,
) -> Vec<ConvertItemDto> {
    items
        .into_iter()
        .map(|i| ConvertItemDto {
            // 源方块先查原版本的表，再退到目标版本的表：它在原版本里一定存在，
            // 但投影用的版本号不一定能在库里对上，那时只能靠目标版本的表
            name_zh: name_from_tables(&i.name, &[source_langs, target_langs], false),
            name_en: name_from_tables(&i.name, &[source_langs, target_langs], true),
            // 替代品是目标版本里的方块，只查目标版本的表
            suggested_zh: name_from_tables(&i.suggested, &[target_langs], false),
            suggested_en: name_from_tables(&i.suggested, &[target_langs], true),
            name: i.name,
            count: i.count,
            suggested: i.suggested,
            chosen: i.chosen,
        })
        .collect()
}

/// 按给定顺序在各语言表里查名字，全都没查到时退回不带命名空间的方块名。
///
/// 中英两套都要给：界面按当前语言取一套，缺哪套就退回另一套并标注
/// 「语言表中没有这一项」，与方块表、实体表的行为一致——两边不一致时，
/// 同一个方块在「方块」页签标灰、在版本转换窗口却不标，用户无从判断。
///
/// 顺序表而不是单个表：源方块在两个版本的语言表里可能只有一边有条目，
/// 因此**必须把表找完**再决定要不要退回空串。
///
/// 只有整张表里都没有这个方块时才退回不带命名空间的方块名：那种情况下
/// 界面无从判断它是缺名字还是坏了，给一个能认的 ID 比留白有用。
fn name_from_tables(
    name: &str,
    tables: &[&std::collections::HashMap<String, (String, String)>],
    want_en: bool,
) -> String {
    if name.is_empty() {
        return String::new();
    }
    // 认得出这个方块、但缺想要的那种语言的名字
    let mut known_without_wanted = false;
    for t in tables {
        if let Some((zh, en)) = t.get(name) {
            let (first, second) = if want_en { (en, zh) } else { (zh, en) };
            if !first.is_empty() {
                return first.clone();
            }
            if !second.is_empty() {
                known_without_wanted = true;
            }
        }
    }
    if known_without_wanted {
        return String::new();
    }
    name.split_once(':').map(|(_, n)| n.to_string()).unwrap_or_else(|| name.to_string())
}

/// 执行转换，结果另存为新文件，原文件不动。
#[tauri::command]
pub fn apply_conversion(
    app: tauri::AppHandle,
    schematic_path: String,
    target_index: usize,
    items: Vec<ConvertItemDto>,
) -> Result<ConvertOutcomeDto, String> {
    let loaded = open_db(&app).map_err(|e| e.to_string())?;
    let db = &loaded.db;
    let mut sc = crate::schematic::litematic::Schematic::load(&schematic_path).map_err(|e| e.to_string())?;

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
    // 版本名可能含路径分隔符或 `..`（基础库的版本名取自 jar 里的
    // version.json，用户也能自己往 base/blocks 放包），必须清理成一段
    // 安全文件名再拼接，否则写出路径会离开投影所在目录。
    let out_path = dir.join(format!(
        "{stem}_{}.litematic",
        crate::schematic::convert::safe_file_component(&target.name)
    ));

    let items: Vec<crate::schematic::convert::ConvertItem> = items
        .into_iter()
        .map(|i| crate::schematic::convert::ConvertItem {
            name: i.name,
            count: i.count,
            suggested: i.suggested,
            chosen: i.chosen,
        })
        .collect();

    let out = crate::schematic::convert::convert(&mut sc, db, source_idx, target_index, &items);
    // 只有真的替换过方块才需要重编码 BlockStates 与调色板；
    // 若仅改版本号（方块都没变），save_to 仍会序列化整个 root，
    // 版本号照样写入，无需无谓地重写全部区域数据。
    sc.blocks_modified = out.replaced_blocks > 0;
    sc.save_to(&out_path.to_string_lossy()).map_err(|e| e.to_string())?;

    let name = out_path
        .file_name()
        .map(|v| v.to_string_lossy().into_owned())
        .unwrap_or_default();
    crate::app::run_log::info(&format!(
        "转换 {name}：替换 {} 种方块，{} 种未指定",
        out.replaced_kinds, out.skipped_kinds
    ));

    Ok(ConvertOutcomeDto {
        replaced_kinds: out.replaced_kinds,
        replaced_blocks: out.replaced_blocks,
        skipped_kinds: out.skipped_kinds,
        output_path: out_path.to_string_lossy().into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 转换条目必须带上中文名，各级回退都要对。
    ///
    /// 界面上凡是出现方块 ID 的地方都要同时显示中文名，因此这条盯的是
    /// 「名字从哪来」：源方块查原版本的表（它在目标版本里已经不存在），
    /// 替代品只查目标版本的表，都查不到时退回不带命名空间的方块名，
    /// 而不是留一个空串让界面空白。
    #[test]
    fn convert_items_carry_chinese_names_with_fallbacks() {
        let mut source = std::collections::HashMap::new();
        source.insert(
            "minecraft:oak_stairs".to_string(),
            ("橡木楼梯".to_string(), "Oak Stairs".to_string()),
        );
        // 只有英文名：缺中文名时不能留白
        source.insert(
            "minecraft:old_block".to_string(),
            (String::new(), "Old Block".to_string()),
        );

        let mut target = std::collections::HashMap::new();
        target.insert(
            "minecraft:pale_oak_stairs".to_string(),
            ("苍白橡木楼梯".to_string(), "Pale Oak Stairs".to_string()),
        );

        let items = vec![
            crate::schematic::convert::ConvertItem {
                name: "minecraft:oak_stairs".into(),
                count: 12,
                suggested: "minecraft:pale_oak_stairs".into(),
                chosen: String::new(),
            },
            // 没有推荐：中文名也必须是空，不能凭空编一个
            crate::schematic::convert::ConvertItem {
                name: "minecraft:old_block".into(),
                count: 3,
                suggested: String::new(),
                chosen: String::new(),
            },
            // 两张表都没有：退回方块名本身
            crate::schematic::convert::ConvertItem {
                name: "mymod:unknown_thing".into(),
                count: 1,
                suggested: String::new(),
                chosen: String::new(),
            },
            // 推荐品只存在于原版本的表里：替代品不该借用源表的名字
            crate::schematic::convert::ConvertItem {
                name: "minecraft:old_block".into(),
                count: 1,
                suggested: "minecraft:oak_stairs".into(),
                chosen: String::new(),
            },
            // 源方块在源表里没有、只在目标表里有：必须退到第二张表去找
            crate::schematic::convert::ConvertItem {
                name: "minecraft:pale_oak_stairs".into(),
                count: 2,
                suggested: String::new(),
                chosen: String::new(),
            },
            // 空名字：两个字段都应是空串，不能拼出去命名空间的分支
            crate::schematic::convert::ConvertItem {
                name: String::new(),
                count: 0,
                suggested: String::new(),
                chosen: String::new(),
            },
        ];

        let out = convert_items_with_zh(items, &source, &target);

        assert_eq!(out[0].name_zh, "橡木楼梯");
        assert_eq!(out[0].name_en, "Oak Stairs", "英文名也要一并给出，供英语界面用");
        assert_eq!(out[0].suggested_zh, "苍白橡木楼梯");
        assert_eq!(out[0].suggested_en, "Pale Oak Stairs");
        assert_eq!(
            out[1].name_zh, "",
            "缺中文名时留空，由界面退回英文名并标注缺项，与方块表一致"
        );
        assert_eq!(out[1].name_en, "Old Block", "这一套名字本身是有的");
        assert_eq!(out[1].suggested_zh, "", "没有推荐时中文名必须为空");
        assert_eq!(out[1].suggested_en, "", "没有推荐时英文名必须为空");
        assert_eq!(out[2].name_zh, "unknown_thing", "查不到应退回方块名而不是空串");
        assert_eq!(out[2].name_en, "unknown_thing");
        assert_eq!(
            out[3].suggested_zh, "oak_stairs",
            "替代品只能在目标版本的表里查，不能借用源版本的名字"
        );
        assert_eq!(out[3].suggested_en, "oak_stairs");
        assert_eq!(
            out[4].name_zh, "苍白橡木楼梯",
            "源表里没有时应退到目标表去找"
        );
        assert_eq!(out[4].name_en, "Pale Oak Stairs");
        assert_eq!(out[5].name_zh, "", "空方块名不该拼出一个名字");
        assert_eq!(out[5].name_en, "");
        assert_eq!(out[5].suggested_zh, "");
        assert_eq!(out[5].suggested_en, "");
        assert_eq!(out[0].name, "minecraft:oak_stairs", "原字段不应被改动");
        assert_eq!(out[0].count, 12);
    }
}
