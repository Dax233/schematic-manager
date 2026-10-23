use super::*;
use crate::block::db::VersionDef;
use crate::block::db::{BlockDef, Variant};
use crate::schematic::litematic::{BlockState, Region};
use std::path::Path;
use std::collections::HashMap;

fn version(name: &str, dv: i32, blocks: &[&str]) -> VersionDef {
    VersionDef {
        name: name.into(),
        data_version: dv,
        lang_idx: 0,
        texture_by_path: HashMap::new(),
        blocks: blocks
            .iter()
            .map(|n| BlockDef {
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
        langs: Vec::new(),
        is_overlay: false,
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

fn schematic(palette: Vec<BlockState>, indices: Vec<i32>, dv: i32) -> Schematic {
    Schematic {
        root: crate::format::nbt::Value::Compound(vec![(
            "MinecraftDataVersion".into(),
            crate::format::nbt::Value::Int(dv),
        )]),
        regions: vec![Region {
            name: "r".into(),
            pos: [0, 0, 0],
            size: [indices.len() as i32, 1, 1],
            volume: indices.len(),
            bits: 4,
            palette,
            indices,
            ..Default::default()
        }],
        path: String::new(),
        meta_modified: false,
        blocks_modified: false,
    }
}

/// 版本名不得把转换结果写出投影所在目录。
///
/// 这不是假想的输入：`base/blocks` 下的单版本包可以由用户自己放进去，
/// 版本名随之而来。这里把当时实测能逃出目录的几个取值固定成用例。
#[test]
fn safe_file_component_blocks_path_traversal() {
    for bad in [
        r"a\..\..\evil",
        "a/../../evil",
        r"..\..\evil",
        "C:/Windows/Temp/evil",
        r"a\..\..\..\..\evil",
    ] {
        let cleaned = safe_file_component(bad);
        // 清理后必须只剩一段文件名：不能含任何路径分隔符。
        // 注意不必禁止子串 ".."——`a_.._.._evil` 里的 ".." 已被下划线
        // 夹住，整体只是一个普通文件名，不再是上级目录引用。
        assert!(
            !cleaned.contains('/') && !cleaned.contains('\\'),
            "版本名 {bad:?} 清理后仍含路径分隔符：{cleaned:?}"
        );
        // 真正要守的性质：拼出的文件仍落在原目录，父目录不变
        let joined = Path::new("/data/proj").join(format!("demo_{cleaned}.litematic"));
        assert_eq!(
            joined.parent(),
            Some(Path::new("/data/proj")),
            "版本名 {bad:?} 清理后仍会写出目录：{}",
            joined.display()
        );
    }
}

#[test]
fn safe_file_component_keeps_normal_version_names() {
    // 正常版本名必须原样保留，否则转换结果的文件名会变得难以辨认
    assert_eq!(safe_file_component("1.20.1"), "1.20.1");
    assert_eq!(safe_file_component("26.3"), "26.3");
    assert_eq!(safe_file_component("我的整合包"), "我的整合包");
}

#[test]
fn safe_file_component_handles_windows_quirks() {
    // 结尾的点与空格在 Windows 上被静默忽略，会导致实际文件名与预期不符
    assert_eq!(safe_file_component("1.20.1."), "1.20.1");
    assert_eq!(safe_file_component("  1.20.1  "), "1.20.1");
    assert_eq!(safe_file_component("1.20.1 "), "1.20.1");
    // 保留字符与控制字符换成下划线
    assert_eq!(safe_file_component("a<b>c"), "a_b_c");
    assert_eq!(safe_file_component("a|b"), "a_b");
    assert_eq!(safe_file_component("a\u{7}b"), "a_b");
    // 全是分隔符时会被换成下划线，得到 ____ 这类可用文件名（不是空串）；
    // 只有清理后**确实为空**（空输入或全是空格/结尾点）才用兜底名
    assert_eq!(safe_file_component(r"\\\/"), "____");
    assert_eq!(safe_file_component("..."), "未知版本");
    assert_eq!(safe_file_component("   "), "未知版本");
    assert_eq!(safe_file_component(""), "未知版本");
}

/// 两个版本名清理后若相同，会写到同一个文件、互相覆盖。
/// 这是清理带来的固有取舍：宁可让用户看到「覆盖」，也不能写出目录。
#[test]
fn safe_file_component_collapses_slash_variants() {
    assert_eq!(
        safe_file_component("a/b"),
        safe_file_component(r"a\b"),
        "路径分隔符的两种写法应归一到同一个文件名"
    );
}

#[test]
fn analyze_lists_only_blocks_missing_from_target() {        let old = version("old", 100, &["minecraft:oak_stairs", "minecraft:stone"]);
    let new = version("new", 200, &["minecraft:stone"]);
    let db = db_with(vec![old, new]);
    let sc = schematic(
        vec![
            BlockState::new("minecraft:oak_stairs"),
            BlockState::new("minecraft:stone"),
        ],
        vec![0, 1, 0],
        100,
    );
    let items = analyze(&sc, &db, 1);
    assert_eq!(items.len(), 1, "只有目标版本缺失的方块才需处理");
    assert_eq!(items[0].name, "minecraft:oak_stairs");
    assert_eq!(items[0].count, 2, "应统计出现次数");
}

#[test]
fn analyze_counts_per_palette_entry_not_per_block() {
    // 同一方块在调色板里只出现一次，但实例数是 3
    let old = version("old", 100, &["minecraft:gone"]);
    let new = version("new", 200, &["minecraft:stone"]);
    let db = db_with(vec![old, new]);
    let sc = schematic(vec![BlockState::new("minecraft:gone")], vec![0, 0, 0], 100);
    let items = analyze(&sc, &db, 1);
    assert_eq!(items[0].count, 3, "应按调色板项折算实例数");
}

#[test]
fn suggest_prefers_same_suffix() {
    let target = ["minecraft:oak_slab", "minecraft:spruce_stairs", "minecraft:deepslate_stone"];
    // 同后缀 _stairs -> 应选 spruce_stairs
    assert_eq!(suggest_block("mod:bamboo_stairs", &target), "minecraft:spruce_stairs");
    // 同后缀 _slab -> 应选 oak_slab
    assert_eq!(suggest_block("mod:birch_slab", &target), "minecraft:oak_slab");
}

#[test]
fn suggest_falls_back_to_same_prefix() {
    let target = ["minecraft:deepslate_bricks", "minecraft:stone"];
    // 没有同后缀候选时用同前缀
    assert_eq!(
        suggest_block("mod:deepslate_tiles", &target),
        "minecraft:deepslate_bricks"
    );
}

#[test]
fn suggest_prefers_vanilla_and_is_stable() {
    let target = ["modpack:aaa_stairs", "minecraft:zzz_stairs"];
    // 原版优先，即便字典序靠后
    assert_eq!(suggest_block("mod:x_stairs", &target), "minecraft:zzz_stairs");
    // 同一输入两次结果一致
    assert_eq!(
        suggest_block("mod:x_stairs", &target),
        suggest_block("mod:x_stairs", &target)
    );
}

#[test]
fn suggest_returns_empty_when_nothing_matches() {
    assert_eq!(suggest_block("minecraft:stone", &["mod:completely_different"]), "");
}

#[test]
fn suggestions_preserve_placement_forms() {
    let target = ["minecraft:birch_sign", "minecraft:birch_wall_sign",
        "minecraft:birch_hanging_sign", "minecraft:birch_wall_hanging_sign",
        "minecraft:skeleton_skull", "minecraft:skeleton_wall_skull",
        "minecraft:player_head", "minecraft:player_wall_head"];
    for (from, to) in [
        ("pale_oak_sign", "birch_sign"), ("pale_oak_wall_sign", "birch_wall_sign"),
        ("pale_oak_hanging_sign", "birch_hanging_sign"),
        ("pale_oak_wall_hanging_sign", "birch_wall_hanging_sign"),
        ("wither_skeleton_wall_skull", "skeleton_wall_skull"),
        ("zombie_wall_head", "player_wall_head"),
    ] {
        assert_eq!(suggest_block(&format!("minecraft:{from}"), &target), format!("minecraft:{to}"));
    }
    assert_eq!(suggest_block("minecraft:pale_oak_wall_sign", &["minecraft:oak_sign"]), "");
    assert_eq!(suggest_block("minecraft:bubble_column", &["minecraft:bubble_coral"]), "");
    assert_eq!(suggest_block("minecraft:soul_wall_torch", &["minecraft:torch", "minecraft:wall_torch"]), "minecraft:wall_torch");
    assert_eq!(suggest_block("minecraft:soul_torch", &["minecraft:torch", "minecraft:wall_torch"]), "minecraft:torch");
}

#[test]
fn analyze_uses_registry_even_when_cached_models_are_missing() {
    let db = db_with(vec![version("1.20.4", 3700, &[])]);
    let sc = schematic(vec!["water", "lava", "bubble_column", "wither_skeleton_wall_skull",
        "pale_oak_wall_sign", "pale_oak_sign"].into_iter()
        .map(|name| BlockState::new(&format!("minecraft:{name}"))).collect(), vec![0, 1, 2, 3, 4], 4189);
    let items = analyze(&sc, &db, 0);
    assert_eq!(items.len(), 1, "合法特殊方块与未使用调色板项不应列为缺失");
    assert_eq!(items[0].name, "minecraft:pale_oak_wall_sign");
    assert!(items[0].suggested.ends_with("_wall_sign"));
}

#[test]
fn conversion_preserves_target_wall_sign_properties_without_render_conditions() {
    let db = db_with(vec![version("source", 100, &["mod:custom_wall_sign"]),
        version("1.20.4", 3700, &["minecraft:birch_wall_sign"])]);
    let mut state = BlockState::new("mod:custom_wall_sign");
    state.set_prop("facing", "east");
    state.set_prop("waterlogged", "true");
    state.set_prop("rotation", "8");
    let mut sc = schematic(vec![state], vec![0], 100);
    let items = vec![ConvertItem { name: "mod:custom_wall_sign".into(), count: 1,
        suggested: "minecraft:birch_wall_sign".into(), chosen: String::new() }];
    convert(&mut sc, &db, 0, 1, &items).unwrap();
    let result = &sc.regions[0].palette[sc.regions[0].indices[0] as usize];
    assert_eq!(result.name, "minecraft:birch_wall_sign");
    assert_eq!(result.prop("facing"), Some("east"));
    assert_eq!(result.prop("waterlogged"), Some("true"));
    assert_eq!(result.prop("rotation"), None);
}

#[test]
fn conversion_checks_target_version_property_domain() {
    let mut source = version("source", 100, &["mod:old", "mod:new"]);
    source.blocks[1].variants[0].when = "facing=east,removed=yes".into();
    let mut target = version("target", 200, &["mod:new"]);
    target.blocks[0].variants[0].when = "facing=west,added=true".into();
    let db = db_with(vec![source, target]);
    let mut state = BlockState::new("mod:old");
    state.set_prop("facing", "east");
    state.set_prop("removed", "yes");
    let mut sc = schematic(vec![state], vec![0], 100);
    let items = vec![ConvertItem { name: "mod:old".into(), count: 1,
        suggested: "mod:new".into(), chosen: String::new() }];
    convert(&mut sc, &db, 0, 1, &items).unwrap();
    let result = &sc.regions[0].palette[sc.regions[0].indices[0] as usize];
    assert_eq!(result.prop("facing"), Some("west"));
    assert_eq!(result.prop("added"), Some("true"));
    assert_eq!(result.prop("removed"), None);
}

#[test]
fn invalid_target_or_replacement_leaves_input_unchanged() {
    let db = db_with(vec![version("source", 100, &["mod:old"]),
        version("1.20.4", 3700, &[]), version("snapshot", 3699, &[])]);
    let mut sc = schematic(vec![BlockState::new("mod:old")], vec![0], 100);
    let root = sc.root.clone();
    let palette = sc.regions[0].palette.clone();
    let items = vec![ConvertItem { name: "mod:old".into(), count: 1,
        suggested: "minecraft:not_a_block".into(), chosen: String::new() }];
    assert!(convert(&mut sc, &db, 0, 1, &items).unwrap_err().to_string().contains("替代方块"));
    assert!(convert(&mut sc, &db, 0, 2, &[]).unwrap_err().to_string().contains("完整方块"));
    assert_eq!(sc.root, root);
    assert_eq!(sc.regions[0].palette, palette);
}

#[test]
fn analyze_sorted_by_count_desc() {
    let old = version("old", 100, &["minecraft:a", "minecraft:b"]);
    let new = version("new", 200, &[]);
    let db = db_with(vec![old, new]);
    let sc = schematic(
        vec![BlockState::new("minecraft:a"), BlockState::new("minecraft:b")],
        vec![0, 0, 0, 1],
        100,
    );
    let items = analyze(&sc, &db, 1);
    assert_eq!(items[0].name, "minecraft:a", "数量多的排前面");
    assert_eq!(items[0].count, 3);
    assert_eq!(items[1].count, 1);
}

#[test]
fn convert_replaces_and_updates_data_version() {
    let old = version("old", 100, &["minecraft:gone", "minecraft:stone"]);
    let new = version("new", 200, &["minecraft:kept"]);
    let db = db_with(vec![old, new]);
    let mut sc = schematic(
        vec![BlockState::new("minecraft:gone"), BlockState::new("minecraft:stone")],
        vec![0, 1, 0],
        100,
    );
    let items = vec![ConvertItem {
        name: "minecraft:gone".into(),
        count: 2,
        suggested: "minecraft:kept".into(),
        chosen: String::new(),
    }];
    let out = convert(&mut sc, &db, 0, 1, &items).unwrap();

    assert_eq!(out.replaced_kinds, 1);
    assert_eq!(out.replaced_blocks, 2);
    // 方块被换掉
    for idx in &sc.regions[0].indices {
        let st = &sc.regions[0].palette[*idx as usize];
        assert_ne!(st.name, "minecraft:gone", "原方块应已被替换");
    }
    // 版本号被改写
    assert_eq!(
        sc.root.get("MinecraftDataVersion"),
        Some(&crate::format::nbt::Value::Int(200))
    );
}

#[test]
fn convert_skips_items_without_replacement() {
    let old = version("old", 100, &["minecraft:gone"]);
    let new = version("new", 200, &["minecraft:kept"]);
    let db = db_with(vec![old, new]);
    let mut sc = schematic(vec![BlockState::new("minecraft:gone")], vec![0], 100);
    let items = vec![ConvertItem {
        name: "minecraft:gone".into(),
        count: 1,
        suggested: String::new(),
        chosen: String::new(),
    }];
    let out = convert(&mut sc, &db, 0, 1, &items).unwrap();
    assert_eq!(out.replaced_kinds, 0);
    assert_eq!(out.skipped_kinds, 1, "没有替代品应计入未指定");
    // 方块保持原样，但版本号仍然改写
    let idx = sc.regions[0].indices[0] as usize;
    assert_eq!(sc.regions[0].palette[idx].name, "minecraft:gone");
    assert_eq!(
        sc.root.get("MinecraftDataVersion"),
        Some(&crate::format::nbt::Value::Int(200))
    );
}

#[test]
fn chosen_overrides_suggestion() {
    let old = version("old", 100, &["minecraft:gone"]);
    let new = version("new", 200, &["minecraft:kept", "minecraft:picked"]);
    let db = db_with(vec![old, new]);
    let mut sc = schematic(vec![BlockState::new("minecraft:gone")], vec![0], 100);
    let items = vec![ConvertItem {
        name: "minecraft:gone".into(),
        count: 1,
        suggested: "minecraft:kept".into(),
        chosen: "minecraft:picked".into(),
    }];
    convert(&mut sc, &db, 0, 1, &items).unwrap();
    // 替换会把新状态追加到调色板、并改写索引指向它，
    // 因此必须顺着索引读，不能直接看 palette[0]
    let idx = sc.regions[0].indices[0] as usize;
    assert_eq!(
        sc.regions[0].palette[idx].name, "minecraft:picked",
        "用户指定应优先于推荐"
    );
}

#[test]
fn convert_adds_data_version_when_missing() {
    let old = version("old", 100, &["minecraft:gone"]);
    let new = version("new", 200, &["minecraft:kept"]);
    let db = db_with(vec![old, new]);
    let mut sc = schematic(vec![BlockState::new("minecraft:gone")], vec![0], 100);
    // 移除版本字段，验证会被补上
    if let crate::format::nbt::Value::Compound(f) = &mut sc.root {
        f.retain(|(k, _)| k != "MinecraftDataVersion");
    }
    convert(&mut sc, &db, 0, 1, &[]).unwrap();
    assert_eq!(
        sc.root.get("MinecraftDataVersion"),
        Some(&crate::format::nbt::Value::Int(200)),
        "缺少版本字段时应补上"
    );
}

#[test]
fn convert_with_unknown_version_does_nothing() {
    let db = db_with(vec![version("old", 100, &["minecraft:gone"])]);
    let mut sc = schematic(vec![BlockState::new("minecraft:gone")], vec![0], 100);
    let items = vec![ConvertItem {
        name: "minecraft:gone".into(),
        count: 1,
        suggested: "minecraft:kept".into(),
        chosen: String::new(),
    }];
    // 目标版本不存在：不能转换，也不能改写版本号
    let err = convert(&mut sc, &db, 0, 9, &items).unwrap_err();
    assert!(err.to_string().contains("目标版本"));
    let idx = sc.regions[0].indices[0] as usize;
    assert_eq!(sc.regions[0].palette[idx].name, "minecraft:gone");
    assert_eq!(
        sc.root.get("MinecraftDataVersion"),
        Some(&crate::format::nbt::Value::Int(100)),
        "目标版本无效时不应改写版本号"
    );
}

#[test]
fn targets_exclude_overlays_and_source() {
    let mut overlay = version("pack", -1, &[]);
    overlay.is_overlay = true;
    let db = db_with(vec![
        version("b", 200, &[]),
        overlay,
        version("a", 100, &[]),
    ]);
    let t = targets(&db, 100);
    assert_eq!(t.len(), 1, "应排除叠加包与源版本");
    assert_eq!(t[0].name, "b");
    assert_eq!(t[0].data_version, 200);
}

/// 没有任何方块被替换时，不应把 blocks_modified 置位。
///
/// 该标志只控制是否重编码 BlockStates；仅改版本号时区块数据没变，
/// 重编码既慢又可能因编码差异改动原文件。
#[test]
fn no_replacement_leaves_blocks_modified_false() {
    let old = version("old", 100, &["minecraft:gone"]);
    let new = version("new", 200, &["minecraft:stone"]);
    let db = db_with(vec![old, new]);
    let mut sc = schematic(vec![BlockState::new("minecraft:gone")], vec![0], 100);
    // 没有可替换项
    let out = convert(&mut sc, &db, 0, 1, &[]).unwrap();
    assert_eq!(out.replaced_blocks, 0);
    assert!(
        !sc.blocks_modified,
        "未替换任何方块时不应置 blocks_modified"
    );
    // 但版本号仍然改写
    assert_eq!(
        sc.root.get("MinecraftDataVersion"),
        Some(&crate::format::nbt::Value::Int(200))
    );
}

#[test]
fn rejected_item_migration_keeps_blocks_root_and_flags_unchanged() {
    use crate::format::nbt::Value;
    let db = db_with(vec![
        version("1.21", 3953, &["minecraft:gone"]),
        version("1.20.4", 3700, &["minecraft:stone"]),
    ]);
    let mut sc = schematic(vec![BlockState::new("minecraft:gone")], vec![0], 3953);
    let stack = |unknown| Value::Compound(vec![
        ("id".into(), Value::String("minecraft:stone".into())),
        ("count".into(), Value::Int(1)),
        ("components".into(), Value::Compound(if unknown {
            vec![("mod:unknown".into(), Value::Int(1))]
        } else { vec![] })),
    ]);
    sc.root.set("Regions", Value::Compound(vec![("r".into(), Value::Compound(vec![
        ("TileEntities".into(), Value::List(vec![Value::Compound(vec![
            ("Items".into(), Value::List(vec![stack(false), stack(true)], 10)),
        ])], 10)),
    ]))]));
    let before_root = sc.root.clone();
    let before_palette = sc.regions[0].palette.clone();
    let before_indices = sc.regions[0].indices.clone();
    let items = analyze(&sc, &db, 1);
    assert!(convert(&mut sc, &db, 0, 1, &items).is_err());
    assert_eq!(sc.root, before_root);
    assert_eq!(sc.regions[0].palette, before_palette);
    assert_eq!(sc.regions[0].indices, before_indices);
    assert!(!sc.blocks_modified && !sc.meta_modified);
}

#[test]
fn migration_with_palette_alias_and_block_replacement_roundtrips() {
    use crate::format::nbt::Value;
    let db = db_with(vec![
        version("1.21", 3953, &["minecraft:gone"]),
        version("1.20.4", 3700, &["minecraft:stone"]),
    ]);
    let mut sc = schematic(vec![BlockState::new("minecraft:gone")], vec![0], 3953);
    let xyz = |x, y, z| Value::Compound(vec![
        ("x".into(), Value::Int(x)), ("y".into(), Value::Int(y)), ("z".into(), Value::Int(z)),
    ]);
    sc.root.set("Version", Value::Int(7));
    sc.root.set("Regions", Value::Compound(vec![("r".into(), Value::Compound(vec![
        ("Position".into(), xyz(0, 0, 0)), ("Size".into(), xyz(1, 1, 1)),
        ("Palette".into(), Value::List(vec![Value::Compound(vec![
            ("Name".into(), Value::String("minecraft:gone".into())),
        ])], 10)),
        ("BlockStates".into(), Value::LongArray(vec![0])),
    ]))]));
    let mut items = analyze(&sc, &db, 1);
    items[0].chosen = "minecraft:stone".into();
    let out = convert(&mut sc, &db, 0, 1, &items).unwrap();
    sc.blocks_modified = out.replaced_blocks > 0;
    let path = std::env::temp_dir().join(format!("sm_conversion_{}_{}.litematic",
        std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    let filename = path.to_string_lossy();
    sc.save_to(&filename).unwrap();
    let back = Schematic::load(&filename).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(back.summary().data_version, 3700);
    assert_eq!(back.summary().format_version, 6);
    assert_eq!(back.regions[0].palette[back.regions[0].indices[0] as usize].name, "minecraft:stone");
    let region = back.root.get("Regions").unwrap().get("r").unwrap();
    assert!(region.get("Palette").is_none());
    assert!(region.get("BlockStatePalette").is_some());
}
