//! 将 1.20.5–1.21.1 的物品组件转换为 1.20.4 的物品 NBT。
//!
//! 只在明确支持的版本边界运行；无法表达的组件报错，避免输出看似成功却丢失物品属性的文件。
use crate::error::{AppError, AppResult};
use crate::nbt::Value;

/// 目标版本无法表示的物品被替换后的可追溯位置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompatibilityReplacement {
    pub original_id: String,
    pub replacement_id: String,
    pub count: i32,
    pub location: String,
    pub path: String,
    pub slot: Option<u8>,
}

#[derive(Debug)]
pub struct PreparedData {
    pub root: Value,
    pub replacements: Vec<CompatibilityReplacement>,
}

#[derive(Default)]
struct Migration {
    location: String,
    replacements: Vec<CompatibilityReplacement>,
}

fn known_id(id: &str, names: &str) -> bool {
    id.strip_prefix("minecraft:").is_some_and(|id| names.lines().any(|name| name == id))
}

fn location(name: &str, region: &Value, entry: &Value, entity: bool) -> String {
    let id = entry.get("id").and_then(Value::as_str).unwrap_or("unknown");
    let origin = region.get("Position").and_then(Value::as_xyz);
    let position = if entity {
        entry.get("Pos").and_then(Value::as_list).filter(|v| v.len() == 3)
            .and_then(|v| Some([v[0].as_f64()?, v[1].as_f64()?, v[2].as_f64()?]))
    } else {
        entry.as_xyz().zip(region.get("Size").and_then(Value::as_xyz)).map(|(pos, size)| {
            std::array::from_fn(|i| pos[i] as f64 + (size[i] as i64 + 1).min(0) as f64)
        })
    };
    match (origin, position) {
        (Some(origin), Some(pos)) => format!("{name} / {id} / ({}, {}, {})",
            origin[0] as f64 + pos[0], origin[1] as f64 + pos[1], origin[2] as f64 + pos[2]),
        _ => format!("{name} / {id}"),
    }
}

fn placeholder(value: &Value, id: &str, count: i8, path: &str, migration: &mut Migration) -> Value {
    let slot = value.get("Slot").and_then(Value::as_i64).map(|v| v as u8);
    let replacement = "minecraft:red_stained_glass_pane";
    let name = match id {
        "minecraft:music_disc_creator_music_box" => "音乐唱片：Creator（八音盒）",
        "minecraft:music_disc_creator" => "音乐唱片：Creator",
        "minecraft:music_disc_precipice" => "音乐唱片：Precipice",
        _ => id,
    };
    let mut result = Value::Compound(vec![
        ("id".into(), Value::String(replacement.into())),
        ("Count".into(), Value::Byte(count)),
        ("tag".into(), Value::Compound(vec![
            ("display".into(), Value::Compound(vec![
                ("Name".into(), Value::String(serde_json::json!({"text": name}).to_string())),
            ])),
            ("Enchantments".into(), Value::List(vec![Value::Compound(vec![
                ("id".into(), Value::String("minecraft:mending".into())),
                ("lvl".into(), Value::Short(1)),
            ])], 10)),
            ("schematic_manager".into(), Value::Compound(vec![
                ("original_item".into(), value.clone()),
            ])),
        ])),
    ]);
    if let Some(slot) = slot { result.set("Slot", Value::Byte(slot as i8)); }
    migration.replacements.push(CompatibilityReplacement {
        original_id: id.into(), replacement_id: replacement.into(), count: count as i32,
        location: migration.location.clone(), path: path.into(), slot,
    });
    result
}

fn invalid(path: &str, reason: &str) -> AppError {
    AppError::UnsupportedVersion(format!("{path}：{reason}"))
}

fn compound<'a>(value: &'a Value, path: &str) -> AppResult<&'a [(String, Value)]> {
    match value {
        Value::Compound(fields) => Ok(fields),
        _ => Err(invalid(path, "应为复合标签")),
    }
}

fn list<'a>(value: &'a Value, path: &str) -> AppResult<&'a [Value]> {
    value.as_list().ok_or_else(|| invalid(path, "应为列表"))
}

fn insert(value: &mut Value, key: &str, item: Value, path: &str) -> AppResult<()> {
    if let Some(existing) = value.get(key) {
        if existing != &item {
            return Err(invalid(path, &format!("旧物品字段 {key} 与组件冲突")));
        }
    } else {
        value.set(key, item);
    }
    Ok(())
}

fn display(tag: &mut Value, key: &str, value: Value, path: &str) -> AppResult<()> {
    let mut fields = tag.get("display").cloned().unwrap_or(Value::Compound(vec![]));
    compound(&fields, path)?;
    insert(&mut fields, key, value, path)?;
    tag.set("display", fields);
    Ok(())
}

fn enchantments(value: &Value, path: &str) -> AppResult<Value> {
    let fields = compound(value, path)?;
    if fields.iter().any(|(key, _)| key != "levels" && key != "show_in_tooltip") {
        return Err(invalid(path, "附魔组件包含无法迁移的字段"));
    }
    // 隐藏提示涉及旧版 HideFlags 的位合并，不能在这里忽略。
    if let Some(flag) = value.get("show_in_tooltip") {
        if flag.as_i64() != Some(1) {
            return Err(invalid(path, "暂不支持隐藏附魔提示或非布尔提示标记"));
        }
    }
    let levels = value.get("levels").ok_or_else(|| invalid(path, "缺少 levels"))?;
    let mut result = Vec::new();
    for (id, level) in compound(levels, path)? {
        if !known_id(id, include_str!("../data/enchantments-1.20.4.txt")) {
            return Err(invalid(path, &format!("目标版本不存在附魔 {id}")));
        }
        let level = level.as_i64().and_then(|v| i16::try_from(v).ok())
            .ok_or_else(|| invalid(path, "附魔等级超出旧版范围"))?;
        result.push(Value::Compound(vec![
            ("id".into(), Value::String(id.clone())),
            ("lvl".into(), Value::Short(level)),
        ]));
    }
    Ok(Value::List(result, 10))
}

fn container(value: &Value, path: &str, migration: &mut Migration) -> AppResult<Value> {
    let mut items = Vec::new();
    let mut slots = std::collections::BTreeSet::new();
    for (i, entry) in list(value, path)?.iter().enumerate() {
        let path = format!("{path}[{i}]");
        let fields = compound(entry, &path)?;
        if fields.iter().any(|(key, _)| key != "slot" && key != "item") {
            return Err(invalid(&path, "容器条目含有无法迁移的字段"));
        }
        let slot = entry.get("slot").and_then(Value::as_i64)
            .and_then(|v| u8::try_from(v).ok())
            .ok_or_else(|| invalid(&path, "容器槽位应在 0–255 范围内"))?;
        if !slots.insert(slot) {
            return Err(invalid(&path, "容器槽位重复"));
        }
        let mut item = entry.get("item").cloned()
            .ok_or_else(|| invalid(&path, "缺少 item"))?;
        insert(&mut item, "Slot", Value::Byte(slot as i8), &path)?;
        downgrade_item(&mut item, &format!("{path}/item"), migration)?;
        items.push(item);
    }
    Ok(Value::List(items, 10))
}

fn components(value: &Value, tag: &mut Value, path: &str, migration: &mut Migration) -> AppResult<()> {
    // custom_data 是旧 tag 的基础，必须先处理，不能依赖输入键的排列顺序。
    if let Some(custom) = value.get("minecraft:custom_data") {
        for (key, item) in compound(custom, path)? {
            insert(tag, key, item.clone(), path)?;
        }
    }
    for (name, value) in compound(value, path)? {
        let path = format!("{path}/{name}");
        match name.as_str() {
            "minecraft:custom_data" => {}
            "minecraft:custom_name" => {
                if value.as_str().is_none() {
                    return Err(invalid(&path, "自定义名称应为 JSON 字符串"));
                }
                display(tag, "Name", value.clone(), &path)?;
            }
            "minecraft:lore" => {
                if list(value, &path)?.iter().any(|v| v.as_str().is_none()) {
                    return Err(invalid(&path, "物品描述应为字符串列表"));
                }
                display(tag, "Lore", value.clone(), &path)?;
            }
            "minecraft:container" => {
                let items = container(value, &path, migration)?;
                let mut block = tag.get("BlockEntityTag").cloned().unwrap_or(Value::Compound(vec![]));
                compound(&block, &path)?;
                insert(&mut block, "Items", items, &path)?;
                tag.set("BlockEntityTag", block);
            }
            "minecraft:block_entity_data" => {
                let mut block = value.clone();
                downgrade_contents(&mut block, &path, migration)?;
                let mut existing = tag.get("BlockEntityTag").cloned().unwrap_or(Value::Compound(vec![]));
                compound(&existing, &path)?;
                for (key, item) in compound(&block, &path)? {
                    insert(&mut existing, key, item.clone(), &path)?;
                }
                tag.set("BlockEntityTag", existing);
            }
            "minecraft:damage" | "minecraft:repair_cost" | "minecraft:custom_model_data" => {
                let number = value.as_i64().and_then(|v| i32::try_from(v).ok())
                    .ok_or_else(|| invalid(&path, "应为 32 位整数"))?;
                let key = match name.as_str() {
                    "minecraft:damage" => "Damage",
                    "minecraft:repair_cost" => "RepairCost",
                    _ => "CustomModelData",
                };
                insert(tag, key, Value::Int(number), &path)?;
            }
            "minecraft:enchantments" | "minecraft:stored_enchantments" => {
                let key = if name == "minecraft:enchantments" { "Enchantments" } else { "StoredEnchantments" };
                insert(tag, key, enchantments(value, &path)?, &path)?;
            }
            _ => return Err(invalid(&path, "该物品组件尚不支持转换为 1.20.4；已中止转换，未保存输出")),
        }
    }
    Ok(())
}

fn downgrade_item(value: &mut Value, path: &str, migration: &mut Migration) -> AppResult<()> {
    let fields = compound(value, path)?;
    // 装备列表允许空物品；旧版物品保持原样。
    if fields.is_empty() {
        return Ok(());
    }
    let id = value.get("id").and_then(Value::as_str)
        .ok_or_else(|| invalid(path, "物品缺少 id"))?;
    if !known_id(id, include_str!("../data/items-1.20.4.txt")) {
        let count = value.get("count").or_else(|| value.get("Count")).and_then(Value::as_i64)
            .and_then(|v| i8::try_from(v).ok()).filter(|v| *v > 0)
            .ok_or_else(|| invalid(path, "物品数量应在旧版可表示的 1–127 范围内"))?;
        *value = placeholder(value, id, count, path, migration);
        return Ok(());
    }
    if value.get("count").is_none() && value.get("components").is_none() {
        return Ok(());
    }
    let count = value.get("count").and_then(Value::as_i64)
        .and_then(|v| i8::try_from(v).ok()).filter(|v| *v > 0)
        .ok_or_else(|| invalid(path, "物品数量应在旧版可表示的 1–127 范围内"))?;
    let mut tag = value.get("tag").cloned().unwrap_or(Value::Compound(vec![]));
    compound(&tag, path)?;
    if let Some(data) = value.get("components") {
        components(data, &mut tag, &format!("{path}/components"), migration)?;
    }
    let mut result = value.clone();
    if let Value::Compound(fields) = &mut result {
        fields.retain(|(key, _)| key != "count" && key != "components");
    }
    insert(&mut result, "Count", Value::Byte(count), path)?;
    if !compound(&tag, path)?.is_empty() {
        result.set("tag", tag);
    }
    *value = result;
    Ok(())
}

fn downgrade_contents(value: &mut Value, path: &str, migration: &mut Migration) -> AppResult<()> {
    let Value::Compound(fields) = value else { return Err(invalid(path, "实体应为复合标签")); };
    for (key, item) in fields {
        let path = format!("{path}/{key}");
        match key.as_str() {
            "Items" | "Inventory" | "HandItems" | "ArmorItems" => {
                let Value::List(items, _) = item else { return Err(invalid(&path, "物品应为列表")); };
                for (i, item) in items.iter_mut().enumerate() {
                    downgrade_item(item, &format!("{path}[{i}]"), migration)?;
                }
            }
            "Item" | "SelectedItem" | "RecordItem" | "Book" | "item" | "SaddleItem" | "ArmorItem" | "DecorItem" => {
                downgrade_item(item, &path, migration)?;
            }
            "Passengers" => {
                let Value::List(items, _) = item else { return Err(invalid(&path, "乘客应为列表")); };
                for (i, item) in items.iter_mut().enumerate() {
                    downgrade_contents(item, &format!("{path}[{i}]"), migration)?;
                }
            }
            // 非支持的物品位置必须显式拒绝；不能让新版物品绕过组件检查。
            _ => reject_unhandled_items(item, &path)?,
        }
    }
    Ok(())
}

fn reject_unhandled_items(value: &Value, path: &str) -> AppResult<()> {
    match value {
        Value::Compound(fields) => {
            if value.get("id").and_then(Value::as_str).is_some()
                && (value.get("count").is_some() || value.get("components").is_some())
            {
                return Err(invalid(path, "此物品位置尚不支持迁移到 1.20.4"));
            }
            for (key, item) in fields {
                reject_unhandled_items(item, &format!("{path}/{key}"))?;
            }
        }
        Value::List(items, _) => {
            for (i, item) in items.iter().enumerate() {
                reject_unhandled_items(item, &format!("{path}[{i}]"))?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// 在副本上迁移数据；失败时调用方的版本号、方块及实体均保持原样。
pub fn prepare(root: &Value, source: i32, target: i32) -> AppResult<Option<PreparedData>> {
    if source < 3837 || target >= 3837 {
        return Ok(None);
    }
    if target != 3700 || source > 3955 {
        return Err(invalid("MinecraftDataVersion", "跨越物品组件格式的降级目前支持 1.20.5–1.21.1 → 1.20.4"));
    }
    let mut result = root.clone();
    let mut migration = Migration::default();
    let Value::Compound(fields) = &mut result else { return Err(invalid("/", "根节点应为复合标签")); };
    if let Some((_, regions)) = fields.iter_mut().find(|(key, _)| key == "Regions") {
        let Value::Compound(regions) = regions else { return Err(invalid("Regions", "应为复合标签")); };
        for (name, region) in regions {
            let region_info = Value::Compound(["Position", "Size"].into_iter()
                .filter_map(|key| region.get(key).cloned().map(|value| (key.into(), value))).collect());
            let Value::Compound(fields) = region else { return Err(invalid(name, "区域应为复合标签")); };
            for (key, data) in fields.iter_mut() {
                if key == "Entities" || key == "TileEntities" {
                    let Value::List(entries, _) = data else { return Err(invalid(name, "实体应为列表")); };
                    for (i, entry) in entries.iter_mut().enumerate() {
                        migration.location = location(name, &region_info, entry, key == "Entities");
                        downgrade_contents(entry, &format!("Regions/{name}/{key}[{i}]"), &mut migration)?;
                    }
                }
            }
            // 读取器接受的调色板别名不能随新版版本标签进入 v6 文件。
            if fields.iter().any(|(key, _)| key == "Palette") {
                if fields.iter().any(|(key, _)| key == "BlockStatePalette") {
                    return Err(invalid(name, "同时存在两个调色板字段，无法确定正确内容"));
                }
                for (key, _) in fields.iter_mut() {
                    if key == "Palette" { *key = "BlockStatePalette".into(); }
                }
            }
        }
    }
    result.set("Version", Value::Int(6));
    result.set("SubVersion", Value::Int(1));
    Ok(Some(PreparedData { root: result, replacements: migration.replacements }))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn downgrade_item(value: &mut Value, path: &str) -> AppResult<()> {
        super::downgrade_item(value, path, &mut Migration::default())
    }
    fn downgrade_contents(value: &mut Value, path: &str) -> AppResult<()> {
        super::downgrade_contents(value, path, &mut Migration::default())
    }

    fn c(fields: Vec<(&str, Value)>) -> Value {
        Value::Compound(fields.into_iter().map(|(k, v)| (k.into(), v)).collect())
    }

    fn text(value: &str) -> Value { Value::String(value.into()) }

    fn stack(id: &str, count: i32, components: Value) -> Value {
        c(vec![("id", text(id)), ("count", Value::Int(count)), ("components", components)])
    }

    fn root(entries: Vec<Value>) -> Value {
        c(vec![
            ("Version", Value::Int(7)),
            ("MinecraftDataVersion", Value::Int(3953)),
            ("Regions", c(vec![("r", c(vec![
                ("TileEntities", Value::List(entries, 10)),
            ]))])),
        ])
    }

    #[test]
    fn nested_container_keeps_slots_counts_and_display_data() {
        let nested = stack("minecraft:bone_meal", 41, c(vec![
            ("minecraft:lore", Value::List(vec![text("\"说明\"")], 8)),
        ]));
        let mut outer = stack("minecraft:shulker_box", 1, c(vec![
            ("minecraft:custom_name", text("\"补给\"")),
            ("minecraft:container", Value::List(vec![c(vec![
                ("slot", Value::Int(3)), ("item", nested),
            ])], 10)),
        ]));
        outer.set("Slot", Value::Byte(7));
        downgrade_item(&mut outer, "test").unwrap();
        assert_eq!(outer.get("Count"), Some(&Value::Byte(1)));
        assert_eq!(outer.get("Slot"), Some(&Value::Byte(7)));
        assert!(outer.get("components").is_none());
        let tag = outer.get("tag").unwrap();
        assert_eq!(tag.get("display").unwrap().get("Name"), Some(&text("\"补给\"")));
        let inner = &tag.get("BlockEntityTag").unwrap().get("Items").unwrap().as_list().unwrap()[0];
        assert_eq!(inner.get("Slot"), Some(&Value::Byte(3)));
        assert_eq!(inner.get("Count"), Some(&Value::Byte(41)));
        assert_eq!(inner.get("tag").unwrap().get("display").unwrap().get("Lore"),
            Some(&Value::List(vec![text("\"说明\"")], 8)));
    }

    #[test]
    fn frame_and_passenger_inventory_migrate_without_moving_entities() {
        let frame = c(vec![
            ("id", text("minecraft:item_frame")),
            ("TileY", Value::Int(-10)),
            ("Facing", Value::Byte(1)),
            ("Pos", Value::List(vec![Value::Double(13.5), Value::Double(-9.96875), Value::Double(-11.5)], 6)),
            ("Item", stack("minecraft:clock", 1, c(vec![]))),
            ("Passengers", Value::List(vec![c(vec![
                ("id", text("minecraft:hopper_minecart")),
                ("Items", Value::List(vec![stack("minecraft:bone_meal", 64, c(vec![]))], 10)),
            ])], 10)),
        ]);
        let mut converted = frame.clone();
        downgrade_contents(&mut converted, "frame").unwrap();
        for key in ["id", "TileY", "Facing", "Pos"] {
            assert_eq!(frame.get(key), converted.get(key));
        }
        assert_eq!(converted.get("Item").unwrap().get("Count"), Some(&Value::Byte(1)));
        let passenger = &converted.get("Passengers").unwrap().as_list().unwrap()[0];
        assert_eq!(passenger.get("Items").unwrap().as_list().unwrap()[0].get("Count"), Some(&Value::Byte(64)));
    }

    #[test]
    fn signs_and_custom_data_are_not_interpreted_as_item_stacks() {
        let sign = c(vec![
            ("id", text("minecraft:sign")),
            ("front_text", c(vec![("messages", Value::List(vec![text("\"文字\""); 4], 8))])),
        ]);
        let input = root(vec![sign.clone()]);
        let output = prepare(&input, 3953, 3700).unwrap().unwrap().root;
        assert_eq!(output.get("Regions").unwrap().get("r").unwrap().get("TileEntities"),
            Some(&Value::List(vec![sign], 10)));
        let arbitrary = c(vec![("id", text("app:data")), ("count", Value::Int(900))]);
        let mut item = stack("minecraft:stone", 1, c(vec![("minecraft:custom_data", arbitrary.clone())]));
        downgrade_item(&mut item, "test").unwrap();
        assert_eq!(item.get("tag"), Some(&arbitrary));
    }

    #[test]
    fn unknown_components_reject_the_whole_migration_with_location() {
        let known = c(vec![("Items", Value::List(vec![stack("minecraft:stone", 2, c(vec![]))], 10))]);
        let unknown = c(vec![("Items", Value::List(vec![stack("minecraft:stone", 1,
            c(vec![("mod:unknown", Value::Int(1))]))], 10))]);
        let input = root(vec![known, unknown]);
        let before = input.clone();
        let err = prepare(&input, 3953, 3700).unwrap_err().to_string();
        assert!(err.contains("Regions/r/TileEntities[1]/Items[0]/components/mod:unknown"));
        assert_eq!(input, before);
    }

    #[test]
    fn invalid_counts_and_component_collisions_are_rejected() {
        for count in [0, -1, 128, 256] {
            let mut item = stack("minecraft:stone", count, c(vec![]));
            let before = item.clone();
            assert!(downgrade_item(&mut item, "test").is_err());
            assert_eq!(item, before);
        }
        let mut item = stack("minecraft:stone", 1, c(vec![
            ("minecraft:custom_name", text("\"new\"")),
            ("minecraft:custom_data", c(vec![("display", c(vec![("Name", text("\"old\""))]))])),
        ]));
        assert!(downgrade_item(&mut item, "test").unwrap_err().to_string().contains("冲突"));
    }

    #[test]
    fn enchantments_keep_typed_levels_and_existing_items_stay_unchanged() {
        let mut item = stack("minecraft:diamond_pickaxe", 1, c(vec![
            ("minecraft:damage", Value::Int(12)),
            ("minecraft:enchantments", c(vec![("levels", c(vec![("minecraft:efficiency", Value::Int(5))]))])),
        ]));
        downgrade_item(&mut item, "test").unwrap();
        assert_eq!(item.get("tag").unwrap().get("Damage"), Some(&Value::Int(12)));
        assert_eq!(item.get("tag").unwrap().get("Enchantments"), Some(&Value::List(vec![
            c(vec![("id", text("minecraft:efficiency")), ("lvl", Value::Short(5))]),
        ], 10)));
        let before = item.clone();
        downgrade_item(&mut item, "test").unwrap();
        assert_eq!(before, item);
    }

    #[test]
    fn other_version_boundaries_do_not_receive_a_false_success() {
        let input = root(vec![]);
        assert!(prepare(&input, 3700, 3953).unwrap().is_none());
        assert!(prepare(&input, 3953, 3955).unwrap().is_none());
        assert!(prepare(&input, 3953, 3465).is_err());
        assert!(prepare(&input, 4189, 3700).is_err());
    }

    #[test]
    fn unavailable_enchantments_are_rejected_before_saving() {
        let mut item = stack("minecraft:diamond_sword", 1, c(vec![
            ("minecraft:enchantments", c(vec![("levels", c(vec![("minecraft:breach", Value::Int(2))]))])),
        ]));
        assert!(downgrade_item(&mut item, "test").unwrap_err().to_string().contains("breach"));
    }

    #[test]
    fn unavailable_items_preserve_original_stack_and_report_nested_slot_and_position() {
        let original = stack("minecraft:music_disc_creator", 2, c(vec![]));
        let outer = stack("minecraft:shulker_box", 1, c(vec![
            ("minecraft:container", Value::List(vec![c(vec![
                ("slot", Value::Int(0)), ("item", original.clone()),
            ])], 10)),
        ]));
        let mut input = root(vec![c(vec![
            ("id", text("minecraft:chest")), ("x", Value::Int(2)),
            ("y", Value::Int(3)), ("z", Value::Int(4)),
            ("Items", Value::List(vec![outer], 10)),
        ])]);
        let mut region = input.get("Regions").unwrap().get("r").unwrap().clone();
        region.set("Position", c(vec![("x", Value::Int(10)), ("y", Value::Int(27)), ("z", Value::Int(26))]));
        region.set("Size", c(vec![("x", Value::Int(36)), ("y", Value::Int(-28)), ("z", Value::Int(-27))]));
        input.set("Regions", c(vec![("r", region)]));
        let before = input.clone();
        let prepared = prepare(&input, 3953, 3700).unwrap().unwrap();
        assert_eq!(input, before);
        let report = &prepared.replacements[0];
        assert_eq!(prepared.replacements.len(), 1);
        assert_eq!(report.slot, Some(0));
        assert_eq!(report.location, "r / minecraft:chest / (12, 3, 4)");
        assert!(report.path.ends_with("/Items[0]/components/minecraft:container[0]/item"));
        let outer = &prepared.root.get("Regions").unwrap().get("r").unwrap().get("TileEntities").unwrap().as_list().unwrap()[0]
            .get("Items").unwrap().as_list().unwrap()[0];
        let item = &outer.get("tag").unwrap().get("BlockEntityTag").unwrap().get("Items").unwrap().as_list().unwrap()[0];
        assert_eq!(item.get("id"), Some(&text("minecraft:red_stained_glass_pane")));
        assert_eq!(item.get("Count"), Some(&Value::Byte(2)));
        assert_eq!(item.get("Slot"), Some(&Value::Byte(0)));
        let tag = item.get("tag").unwrap();
        let name: serde_json::Value = serde_json::from_str(tag.get("display").unwrap().get("Name").unwrap().as_str().unwrap()).unwrap();
        assert_eq!(name["text"], "音乐唱片：Creator");
        assert_eq!(tag.get("Enchantments").unwrap().as_list().unwrap()[0].get("id"), Some(&text("minecraft:mending")));
        let mut saved_original = original;
        saved_original.set("Slot", Value::Byte(0));
        assert_eq!(tag.get("schematic_manager").unwrap().get("original_item"), Some(&saved_original));
    }

    #[test]
    fn multiple_disc_replacements_report_entity_coordinates_and_each_slot() {
        let ids = ["minecraft:music_disc_creator_music_box", "minecraft:music_disc_creator", "minecraft:music_disc_precipice"];
        let names = ["音乐唱片：Creator（八音盒）", "音乐唱片：Creator", "音乐唱片：Precipice"];
        let items = ids.iter().enumerate().map(|(i, id)| {
            let mut item = stack(id, 1, c(vec![]));
            item.set("Slot", Value::Byte(i as i8));
            item
        }).collect();
        let input = c(vec![("Regions", c(vec![("r", c(vec![
            ("Position", c(vec![("x", Value::Int(10)), ("y", Value::Int(20)), ("z", Value::Int(30))])),
            ("Entities", Value::List(vec![c(vec![
                ("id", text("minecraft:chest_minecart")),
                ("Pos", Value::List(vec![Value::Double(1.5), Value::Double(-2.0), Value::Double(3.5)], 6)),
                ("Items", Value::List(items, 10)),
            ])], 10)),
        ]))]))]);
        let prepared = prepare(&input, 3953, 3700).unwrap().unwrap();
        assert_eq!(prepared.replacements.len(), 3);
        let entity = &prepared.root.get("Regions").unwrap().get("r").unwrap().get("Entities").unwrap().as_list().unwrap()[0];
        for (i, item) in entity.get("Items").unwrap().as_list().unwrap().iter().enumerate() {
            let report = &prepared.replacements[i];
            assert_eq!(report.original_id, ids[i]);
            assert_eq!(report.location, "r / minecraft:chest_minecart / (11.5, 18, 33.5)");
            assert_eq!(report.slot, Some(i as u8));
            let name: serde_json::Value = serde_json::from_str(item.get("tag").unwrap().get("display").unwrap().get("Name").unwrap().as_str().unwrap()).unwrap();
            assert_eq!(name["text"], names[i]);
        }
    }

    #[test]
    fn unsupported_equipment_and_trades_cannot_bypass_component_checks() {
        let mut wolf = c(vec![("body_armor_item", stack("minecraft:leather_chestplate", 1, c(vec![])))]);
        assert!(downgrade_contents(&mut wolf, "wolf").unwrap_err().to_string().contains("body_armor_item"));
        let mut villager = c(vec![("Offers", c(vec![("Recipes", Value::List(vec![c(vec![
            ("sell", stack("minecraft:stone", 1, c(vec![("mod:unknown", Value::Int(1))]))),
        ])], 10))]))]);
        assert!(downgrade_contents(&mut villager, "villager").unwrap_err().to_string().contains("Offers/Recipes[0]/sell"));
    }

    #[test]
    fn block_entity_component_migrates_embedded_inventory() {
        let mut item = stack("minecraft:chest", 1, c(vec![
            ("minecraft:block_entity_data", c(vec![
                ("id", text("minecraft:chest")),
                ("Items", Value::List(vec![stack("minecraft:stone", 32, c(vec![]))], 10)),
            ])),
        ]));
        downgrade_item(&mut item, "chest").unwrap();
        let block = item.get("tag").unwrap().get("BlockEntityTag").unwrap();
        assert_eq!(block.get("id"), Some(&text("minecraft:chest")));
        assert_eq!(block.get("Items").unwrap().as_list().unwrap()[0].get("Count"), Some(&Value::Byte(32)));
    }
}
