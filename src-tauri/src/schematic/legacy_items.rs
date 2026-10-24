//! 将 1.20.5–1.21.4 的物品组件及关联实体结构转换为 1.20.4 的 NBT。
//!
//! 只在明确支持的版本边界运行；无法表达的组件报错，避免输出看似成功却丢失物品属性的文件。
use crate::error::{AppError, AppResult};
use crate::format::nbt::Value;

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
    choices: Vec<super::data_replacements::Choice>,
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

// NBT 复合标签的键顺序不影响游戏含义；列表顺序与数值类型仍须严格相同。
fn same_data(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Compound(a), Value::Compound(b)) => {
            if a.len() != b.len() { return false; }
            let mut a: Vec<_> = a.iter().collect();
            let mut b: Vec<_> = b.iter().collect();
            a.sort_by(|a, b| a.0.cmp(&b.0));
            b.sort_by(|a, b| a.0.cmp(&b.0));
            a.iter().zip(b).all(|(a, b)| a.0 == b.0 && same_data(&a.1, &b.1))
        }
        (Value::List(a, at), Value::List(b, bt)) => {
            at == bt && a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same_data(a, b))
        }
        _ => left == right,
    }
}

fn insert(value: &mut Value, key: &str, item: Value, path: &str) -> AppResult<()> {
    if let Some(existing) = value.get(key) {
        if !same_data(existing, &item) {
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
    let mut ids = std::collections::BTreeSet::new();
    for (id, level) in compound(levels, path)? {
        let id = if id == "minecraft:sweeping_edge" { "minecraft:sweeping" } else { id.as_str() };
        if !known_id(id, include_str!("../../data/enchantments-1.20.4.txt")) {
            return Err(invalid(path, &format!("目标版本不存在附魔 {id}")));
        }
        if !ids.insert(id) { return Err(invalid(path, "附魔映射后名称重复")); }
        let level = level.as_i64().and_then(|v| i16::try_from(v).ok())
            .ok_or_else(|| invalid(path, "附魔等级超出旧版范围"))?;
        result.push(Value::Compound(vec![
            ("id".into(), Value::String(id.into())),
            ("lvl".into(), Value::Short(level)),
        ]));
    }
    Ok(Value::List(result, 10))
}

fn unbreakable(value: &Value, tag: &mut Value, path: &str) -> AppResult<()> {
    if compound(value, path)?.iter().any(|(key, _)| key != "show_in_tooltip") {
        return Err(invalid(path, "不可破坏组件含有尚不能迁移的字段"));
    }
    let show = match value.get("show_in_tooltip") {
        None | Some(Value::Byte(1)) => true,
        Some(Value::Byte(0)) => false,
        _ => return Err(invalid(path, "提示标记应为布尔值")),
    };
    let flags = tag.get("HideFlags").map(|v| v.as_i64().and_then(|v| i32::try_from(v).ok())
        .ok_or_else(|| invalid(path, "HideFlags 应为32位整数"))).transpose()?.unwrap_or(0);
    if show && flags & 4 != 0 { return Err(invalid(path, "不可破坏提示与旧 HideFlags 冲突")); }
    insert(tag, "Unbreakable", Value::Byte(1), path)?;
    if !show { tag.set("HideFlags", Value::Int(flags | 4)); }
    Ok(())
}

fn writable_book(value: &Value, tag: &mut Value, path: &str) -> AppResult<()> {
    if compound(value, path)?.iter().any(|(key, _)| key != "pages") {
        return Err(invalid(path, "书与笔含有尚不能迁移的字段"));
    }
    let mut pages = Vec::new();
    let mut filtered = Value::Compound(vec![]);
    if let Some(values) = value.get("pages") {
        for (i, page) in list(values, path)?.iter().enumerate() {
            let path = format!("{path}/pages[{i}]");
            if let Value::String(_) = page { pages.push(page.clone()); continue; }
            if compound(page, &path)?.iter().any(|(key, _)| key != "raw" && key != "filtered") {
                return Err(invalid(&path, "书页含有尚不能迁移的字段"));
            }
            let raw = page.get("raw").filter(|v| v.as_str().is_some()).ok_or_else(|| invalid(&path, "书页缺少原始文本"))?;
            pages.push(raw.clone());
            if let Some(text) = page.get("filtered") {
                if text.as_str().is_none() { return Err(invalid(&path, "过滤后书页应为文本")); }
                filtered.set(&i.to_string(), text.clone());
            }
        }
    }
    insert(tag, "pages", Value::List(pages, 8), path)?;
    if !compound(&filtered, path)?.is_empty() || tag.get("filtered_pages").is_some() {
        insert(tag, "filtered_pages", filtered, path)?;
    }
    Ok(())
}

fn custom_model_data(value: &Value, path: &str) -> AppResult<Value> {
    if let Some(number) = value.as_i64().and_then(|n| i32::try_from(n).ok()) {
        return Ok(Value::Int(number));
    }
    for (key, data) in compound(value, path)? {
        match key.as_str() {
            "floats" => { list(data, path)?; }
            "flags" | "strings" | "colors" if list(data, path)?.is_empty() => {}
            _ => return Err(invalid(path, "多通道自定义模型数据无法等价转换为旧版整数")),
        }
    }
    let values = value.get("floats").map(|v| list(v, path)).transpose()?.unwrap_or(&[]);
    let number = match values {
        [] => 0.0,
        [number] => number.as_f64().ok_or_else(|| invalid(path, "自定义模型数据应为数值"))?,
        _ => return Err(invalid(path, "多个模型数值无法等价转换为旧版整数")),
    };
    if !number.is_finite() || number.fract() != 0.0 || number < i32::MIN as f64 || number > i32::MAX as f64 {
        return Err(invalid(path, "模型数值超出旧版整数范围或精度"));
    }
    Ok(Value::Int(number as i32))
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
            "minecraft:unbreakable" => unbreakable(value, tag, &path)?,
            "minecraft:writable_book_content" => writable_book(value, tag, &path)?,
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
            "minecraft:damage" | "minecraft:repair_cost" => {
                let number = value.as_i64().and_then(|v| i32::try_from(v).ok())
                    .ok_or_else(|| invalid(&path, "应为 32 位整数"))?;
                let key = if name == "minecraft:damage" { "Damage" } else { "RepairCost" };
                insert(tag, key, Value::Int(number), &path)?;
            }
            "minecraft:custom_model_data" => {
                insert(tag, "CustomModelData", custom_model_data(value, &path)?, &path)?;
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
        .ok_or_else(|| invalid(path, "物品缺少 id"))?.to_string();
    if !known_id(&id, include_str!("../../data/items-1.20.4.txt")) {
        let count = value.get("count").or_else(|| value.get("Count")).and_then(Value::as_i64)
            .and_then(|v| i8::try_from(v).ok()).filter(|v| *v > 0)
            .ok_or_else(|| invalid(path, "物品数量应在旧版可表示的 1–127 范围内"))?;
        let replacement = migration.choices.iter().find(|choice|
            choice.kind == super::data_replacements::Kind::Item && choice.name == id)
            .map(|choice| choice.replacement.clone()).unwrap_or_else(|| super::data_replacements::MARKER.into());
        if let Some(data) = value.get("components") {
                let container = data.get("minecraft:container").and_then(Value::as_list).is_some_and(|items| !items.is_empty());
                let embedded = data.get("minecraft:block_entity_data").is_some_and(|value| !matches!(value, Value::Compound(fields) if fields.is_empty()));
                if container || embedded {
                    return Err(invalid(path, "缺失物品携带库存或方块实体数据：尚未实现对应的结构及库存迁移规则，内部选择无法生效"));
                }
            }
        if replacement == super::data_replacements::MARKER {
            *value = placeholder(value, &id, count, path, migration);
            return Ok(());
        }
        migration.replacements.push(CompatibilityReplacement {
            original_id: id, replacement_id: replacement.clone(), count: count as i32,
            location: migration.location.clone(), path: path.into(),
            slot: value.get("Slot").and_then(Value::as_i64).map(|v| v as u8),
        });
        value.set("id", Value::String(replacement));
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
    if let Some(id) = value.get("id").and_then(Value::as_str).map(str::to_string) {
        if let Some(choice) = migration.choices.iter().find(|choice|
            choice.kind == super::data_replacements::Kind::Entity && choice.name == id) {
            let (target, wood) = super::data_replacements::entity_target(&choice.replacement)
                .ok_or_else(|| invalid(path,"实体替代品无效"))?;
            insert(value, "Type", Value::String(wood.into()), path)?;
            value.set("id",Value::String(target.into()));
            migration.replacements.push(CompatibilityReplacement {
                original_id: id, replacement_id: choice.replacement.clone(), count: 1,
                location: migration.location.clone(), path: path.into(), slot: None,
            });
        }
    }
    crate::schematic::legacy_entities::downgrade(value, path)?;
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
                    let path = format!("{path}[{i}]");
                    downgrade_contents(item, &path, migration)?;
                    crate::schematic::legacy_entities::validate_type(item, &path)?;
                }
            }
            "SpawnData" => downgrade_spawn_data(item, &path, migration)?,
            "SpawnPotentials" => {
                let Value::List(entries, _) = item else { return Err(invalid(&path, "刷怪候选应为列表")); };
                for (i, entry) in entries.iter_mut().enumerate() {
                    let path = format!("{path}[{i}]");
                    let Value::Compound(fields) = entry else { return Err(invalid(&path, "刷怪候选应为复合标签")); };
                    for (key, data) in fields {
                        let path = format!("{path}/{key}");
                        match key.as_str() {
                            "data" => downgrade_spawn_data(data, &path, migration)?,
                            "equipment" => return Err(invalid(&path, "刷怪装备战利品表尚不能等价转换为 1.20.4")),
                            _ => reject_unhandled_items(data, &path)?,
                        }
                    }
                }
            }
            "components" => {}
            // 非支持的物品位置必须显式拒绝；不能让新版物品绕过组件检查。
            _ => reject_unhandled_items(item, &path)?,
        }
    }
    Ok(())
}

fn downgrade_spawn_data(value: &mut Value, path: &str, migration: &mut Migration) -> AppResult<()> {
    let Value::Compound(fields) = value else { return Err(invalid(path, "刷怪数据应为复合标签")); };
    for (key, value) in fields {
        let path = format!("{path}/{key}");
        match key.as_str() {
            "entity" => {
                downgrade_contents(value, &path, migration)?;
                crate::schematic::legacy_entities::validate_type(value, &path)?;
            }
            "equipment" => return Err(invalid(&path, "刷怪装备战利品表尚不能等价转换为 1.20.4")),
            _ => reject_unhandled_items(value, &path)?,
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
    prepare_with_choices(root, source, target, None)
}

/// 显式清单必须覆盖分析出的全部缺失项，避免转换时悄悄采用另一种替代品。
pub fn prepare_with_choices(root: &Value, source: i32, target: i32, choices: Option<&[super::data_replacements::Choice]>) -> AppResult<Option<PreparedData>> {
    if source < 3837 || target >= 3837 {
        return Ok(None);
    }
    if target != 3700 || source > 4189 {
        return Err(invalid("MinecraftDataVersion", "跨越物品组件格式的降级目前支持 1.20.5–1.21.4 → 1.20.4"));
    }
    let missing = super::data_replacements::analyze(root, source, target)?;
    let defaults: Vec<_> = missing.iter().map(|row| super::data_replacements::Choice {
        kind: row.kind, name: row.name.clone(), replacement: row.suggested.clone(),
    }).collect();
    let choices = choices.unwrap_or(&defaults);
    super::data_replacements::validate(&missing, choices)?;
    let mut result = root.clone();
    let mut migration = Migration {choices: choices.to_vec(), ..Default::default()};
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
                        let path = format!("Regions/{name}/{key}[{i}]");
                        let mut migrate = || -> AppResult<()> {
                            downgrade_contents(entry, &path, &mut migration)?;
                            if key == "Entities" { crate::schematic::legacy_entities::validate_type(entry, &path)?; }
                            Ok(())
                        };
                        migrate().map_err(|error| match error {
                            AppError::UnsupportedVersion(reason) => invalid(&migration.location, &reason),
                            other => other,
                        })?;
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
#[path = "../tests/schematic/legacy_items.rs"]
mod tests;
