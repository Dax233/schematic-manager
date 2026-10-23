//! 将实体与方块实体的结构字段还原为 1.20.4 格式。
//!
//! 物品递归由 legacy_items 负责；无法等价表达的行为必须在写出前拒绝。
use crate::error::{AppError, AppResult};
use crate::nbt::Value;

fn invalid(path: &str, reason: &str) -> AppError {
    AppError::UnsupportedVersion(format!("{path}：{reason}"))
}

fn remove(value: &mut Value, key: &str) {
    if let Value::Compound(fields) = value { fields.retain(|(name, _)| name != key); }
}

fn assign(value: &mut Value, key: &str, data: Value, path: &str) -> AppResult<()> {
    if let Some(old) = value.get(key) {
        if old != &data { return Err(invalid(path, &format!("新旧字段 {key} 冲突"))); }
    } else { value.set(key, data); }
    Ok(())
}

fn rename_short(value: &mut Value, from: &str, to: &str, path: &str) -> AppResult<()> {
    if let Some(data) = value.get(from) {
        let number = data.as_i64().and_then(|n| i16::try_from(n).ok())
            .ok_or_else(|| invalid(&format!("{path}/{from}"), "熔炉计时超出旧版 16 位整数范围"))?;
        assign(value, to, Value::Short(number), path)?;
        remove(value, from);
    }
    Ok(())
}

fn remove_default(value: &mut Value, key: &str, expected: f64, path: &str) -> AppResult<()> {
    if let Some(data) = value.get(key) {
        if data.as_f64() != Some(expected) {
            return Err(invalid(&format!("{path}/{key}"), "该非默认行为无法等价转换为 1.20.4"));
        }
        remove(value, key);
    }
    Ok(())
}

fn attributes(value: &Value, path: &str) -> AppResult<Value> {
    let entries = value.as_list().ok_or_else(|| invalid(path, "属性应为列表"))?;
    let mut result = Vec::new();
    let mut names = std::collections::BTreeSet::new();
    for (i, entry) in entries.iter().enumerate() {
        let path = format!("{path}[{i}]");
        let Value::Compound(fields) = entry else { return Err(invalid(&path, "属性应为复合标签")); };
        if fields.iter().any(|(key, _)| !matches!(key.as_str(), "id" | "base" | "modifiers")) {
            return Err(invalid(&path, "属性含有尚不能迁移的字段"));
        }
        if let Some(modifiers) = entry.get("modifiers") {
            if !modifiers.as_list().is_some_and(|v| v.is_empty()) {
                return Err(invalid(&path, "实体属性修饰符尚不能等价转换为 1.20.4"));
            }
        }
        let id = entry.get("id").and_then(Value::as_str).ok_or_else(|| invalid(&path, "属性缺少 id"))?;
        let name = id.strip_prefix("minecraft:").unwrap_or(id);
        let old_name = include_str!("../data/attributes-1.20.4.txt").lines().find(|known| {
            // 新版 jump_strength 适用于更多实体，不能仅凭名称还原成马专用属性。
            *known == name || (*known != "horse.jump_strength" && !name.contains('.')
                && known.split_once('.').is_some_and(|(_, suffix)| suffix == name))
        }).ok_or_else(|| invalid(&path, &format!("目标版本不存在属性 {id}")))?;
        if !names.insert(old_name) { return Err(invalid(&path, "属性重复")); }
        let base = entry.get("base").and_then(Value::as_f64).filter(|v| v.is_finite())
            .ok_or_else(|| invalid(&path, "属性缺少有限的基础数值"))?;
        result.push(Value::Compound(vec![
            ("Name".into(), Value::String(format!("minecraft:{old_name}"))),
            ("Base".into(), Value::Double(base)),
        ]));
    }
    Ok(Value::List(result, 10))
}

/// 还原字段名称、数字类型及船类型；不迁移物品或任意自定义数据。
pub fn downgrade(value: &mut Value, path: &str) -> AppResult<()> {
    if let Some(data) = value.get("attributes") {
        let converted = attributes(data, &format!("{path}/attributes"))?;
        if !converted.as_list().unwrap().is_empty() || value.get("Attributes").is_some() {
            assign(value, "Attributes", converted, path)?;
        }
        remove(value, "attributes");
    }
    if value.get("acceleration_power").is_some() {
        return Err(invalid(&format!("{path}/acceleration_power"), "新版火球加速度尚不能等价转换为 1.20.4"));
    }
    if value.get("lock").is_some() {
        return Err(invalid(&format!("{path}/lock"), "物品谓词容器锁不能等价转换为旧版名称锁"));
    }
    let id = value.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
    let unsupported: &[&str] = match id.as_str() {
        "minecraft:banner" => &["patterns"],
        "minecraft:skull" => &["profile", "custom_name"],
        "minecraft:beehive" => &["bees", "flower_pos"],
        "minecraft:bee" => &["flower_pos", "hive_pos"],
        "minecraft:end_gateway" => &["exit_portal"],
        "minecraft:end_crystal" => &["beam_target"],
        "minecraft:area_effect_cloud" => &["potion_contents"],
        _ => &[],
    };
    for key in unsupported.iter().copied().chain(["leash", "patrol_target", "wander_target"]) {
        if value.get(key).is_some() {
            return Err(invalid(&format!("{path}/{key}"), "该新版实体或方块实体字段尚不能等价转换为 1.20.4"));
        }
    }
    if let Some(components) = value.get("components") {
        let Value::Compound(fields) = components else { return Err(invalid(&format!("{path}/components"), "应为复合标签")); };
        for (key, data) in fields {
            if key != "minecraft:custom_data" || !matches!(data, Value::Compound(_)) {
                return Err(invalid(&format!("{path}/components/{key}"), "该方块实体组件尚不能等价转换为 1.20.4"));
            }
        }
        // custom_data 是任意元数据，不能用其中的历史库存覆盖当前 Items。
    }
    if id == "minecraft:arrow" {
        if let Some(item) = value.get("item").or_else(|| value.get("Item")) {
            if item.get("id").and_then(Value::as_str) != Some("minecraft:arrow") {
                return Err(invalid(&format!("{path}/item"), "药水箭的实体药水效果尚不能等价转换为 1.20.4"));
            }
        }
    }
    if matches!(id.as_str(), "minecraft:furnace" | "minecraft:smoker" | "minecraft:blast_furnace") {
        if value.get("lit_total_time").is_some()
            && value.get("lit_time_remaining").and_then(Value::as_i64) != Some(0)
        {
            return Err(invalid(&format!("{path}/lit_time_remaining"), "燃烧中的熔炉总燃烧时长无法等价转换为 1.20.4"));
        }
        for (from, to) in [("cooking_time_spent", "CookTime"), ("cooking_total_time", "CookTimeTotal"), ("lit_time_remaining", "BurnTime")] {
            rename_short(value, from, to, path)?;
        }
        // 旧版从燃料重新推导总燃烧时长，没有独立持久化字段。
        remove_default(value, "lit_total_time", 0.0, path)?;
    }
    if id == "minecraft:tnt_minecart" {
        if let Some(fuse) = value.get("fuse") {
            let fuse = fuse.as_i64().and_then(|n| i32::try_from(n).ok())
                .ok_or_else(|| invalid(&format!("{path}/fuse"), "引信应为旧版 32 位整数"))?;
            assign(value, "TNTFuse", Value::Int(fuse), path)?;
            remove(value, "fuse");
        }
        remove_default(value, "explosion_speed_factor", 1.0, path)?;
        remove_default(value, "explosion_power", 4.0, path)?;
    }
    if id == "minecraft:tnt" { remove_default(value, "explosion_power", 4.0, path)?; }
    if let Some(name) = id.strip_prefix("minecraft:").filter(|name| !matches!(*name, "boat" | "chest_boat")) {
        if let Some((wood, target)) = name.strip_suffix("_chest_boat").map(|wood| (wood, "chest_boat"))
            .or_else(|| name.strip_suffix("_boat").map(|wood| (wood, "boat")))
            .or_else(|| (name == "bamboo_raft").then_some(("bamboo", "boat")))
            .or_else(|| (name == "bamboo_chest_raft").then_some(("bamboo", "chest_boat")))
        {
            if !["oak", "spruce", "birch", "jungle", "acacia", "dark_oak", "mangrove", "cherry", "bamboo"].contains(&wood) {
                return Err(invalid(path, &format!("目标版本不存在船材质：{wood}")));
            }
            assign(value, "Type", Value::String(wood.into()), path)?;
            value.set("id", Value::String(format!("minecraft:{target}")));
        }
    }
    Ok(())
}

/// 校验顶层实体与乘客类型，避免新版实体在目标游戏中被直接丢弃。
pub fn validate_type(value: &Value, path: &str) -> AppResult<()> {
    let id = value.get("id").and_then(Value::as_str).ok_or_else(|| invalid(path, "实体缺少 id"))?;
    if !id.strip_prefix("minecraft:").is_some_and(|name| include_str!("../data/entities-1.20.4.txt").lines().any(|known| known == name)) {
        return Err(invalid(path, &format!("目标版本不存在实体：{id}")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_attributes_keep_values_and_reject_modifiers_and_unknown_names() {
        for name in ["minecraft:generic.movement_speed", "minecraft:movement_speed"] {
            let attr = Value::Compound(vec![("id".into(), Value::String(name.into())), ("base".into(), Value::Double(0.7))]);
            let mut entity = c("armor_stand", vec![("attributes", Value::List(vec![attr], 10))]);
            downgrade(&mut entity, "stand").unwrap();
            let attr = &entity.get("Attributes").unwrap().as_list().unwrap()[0];
            assert_eq!(attr.get("Name"), Some(&Value::String("minecraft:generic.movement_speed".into())));
            assert_eq!(attr.get("Base"), Some(&Value::Double(0.7)));
            assert!(entity.get("attributes").is_none());
        }
        for name in ["minecraft:generic.jump_strength", "minecraft:jump_strength"] {
            let mut entity = c("armor_stand", vec![("attributes", Value::List(vec![Value::Compound(vec![
                ("id".into(), Value::String(name.into())), ("base".into(), Value::Double(0.8)),
            ])], 10))]);
            assert!(downgrade(&mut entity, "stand").is_err());
        }
        let mut conflict = c("armor_stand", vec![
            ("attributes", Value::List(vec![], 10)),
            ("Attributes", Value::List(vec![Value::Compound(vec![])], 10)),
        ]);
        assert!(downgrade(&mut conflict, "conflict").is_err());
        for name in ["minecraft:generic.movement_speed", "minecraft:scale"] {
            let mut attr = Value::Compound(vec![("id".into(), Value::String(name.into())), ("base".into(), Value::Double(0.7))]);
            if name.contains("movement_speed") { attr.set("modifiers", Value::List(vec![Value::Compound(vec![])], 10)); }
            assert!(downgrade(&mut c("armor_stand", vec![("attributes", Value::List(vec![attr], 10))]), "stand").is_err());
        }
    }

    fn c(id: &str, fields: Vec<(&str, Value)>) -> Value {
        let mut value = Value::Compound(fields.into_iter().map(|(k, v)| (k.into(), v)).collect());
        value.set("id", Value::String(format!("minecraft:{id}")));
        value
    }

    #[test]
    fn furnace_timers_are_restored_as_shorts() {
        for id in ["furnace", "smoker", "blast_furnace"] {
            let mut value = c(id, vec![
                ("cooking_time_spent", Value::Int(17)),
                ("cooking_total_time", Value::Int(200)),
                ("lit_time_remaining", Value::Int(0)),
                ("lit_total_time", Value::Int(0)),
            ]);
            downgrade(&mut value, "furnace").unwrap();
            for (key, expected) in [("CookTime", 17), ("CookTimeTotal", 200), ("BurnTime", 0)] {
                assert_eq!(value.get(key), Some(&Value::Short(expected)));
            }
            for key in ["cooking_time_spent", "cooking_total_time", "lit_time_remaining", "lit_total_time"] {
                assert!(value.get(key).is_none());
            }
        }
    }

    #[test]
    fn timers_reject_conflicts_overflow_and_unrepresentable_fuel_totals() {
        for fields in [
            vec![("cooking_time_spent", Value::Int(40000))],
            vec![("cooking_time_spent", Value::Int(5)), ("CookTime", Value::Short(6))],
            vec![("lit_total_time", Value::Int(1600))],
            vec![("lit_total_time", Value::Int(0)), ("lit_time_remaining", Value::Int(100))],
        ] {
            let mut value = c("furnace", fields);
            assert!(downgrade(&mut value, "furnace").is_err());
        }
    }

    #[test]
    fn wood_specific_boats_restore_type_without_moving_them() {
        for (id, target, wood) in [
            ("oak_boat", "boat", "oak"),
            ("dark_oak_chest_boat", "chest_boat", "dark_oak"),
            ("bamboo_raft", "boat", "bamboo"),
            ("bamboo_chest_raft", "chest_boat", "bamboo"),
        ] {
            let pos = Value::List(vec![Value::Double(1.5), Value::Double(20.0), Value::Double(-5.5)], 6);
            let mut value = c(id, vec![("Pos", pos.clone())]);
            downgrade(&mut value, "boat").unwrap();
            validate_type(&value, "boat").unwrap();
            assert_eq!(value.get("id"), Some(&Value::String(format!("minecraft:{target}"))));
            assert_eq!(value.get("Type"), Some(&Value::String(wood.into())));
            assert_eq!(value.get("Pos"), Some(&pos));
        }
        assert!(downgrade(&mut c("pale_oak_boat", vec![]), "boat").is_err());
        assert!(validate_type(&c("creaking", vec![]), "entity").is_err());
    }

    #[test]
    fn existing_boat_types_and_inventory_remain_unchanged() {
        for id in ["boat", "chest_boat"] {
            let original = c(id, vec![
                ("Type", Value::String("oak".into())), ("Items", Value::List(vec![], 10)),
            ]);
            let mut value = original.clone();
            downgrade(&mut value, "boat").unwrap();
            validate_type(&value, "boat").unwrap();
            assert_eq!(value, original);
        }
    }

    #[test]
    fn tnt_minecart_preserves_fuse_and_rejects_nondefault_explosions() {
        let mut value = c("tnt_minecart", vec![
            ("fuse", Value::Int(42)), ("explosion_speed_factor", Value::Float(1.0)),
            ("explosion_power", Value::Float(4.0)),
        ]);
        downgrade(&mut value, "cart").unwrap();
        assert_eq!(value.get("TNTFuse"), Some(&Value::Int(42)));
        assert!(value.get("fuse").is_none());
        for (key, value) in [("explosion_speed_factor", Value::Float(2.0)), ("explosion_power", Value::Float(6.0))] {
            assert!(downgrade(&mut c("tnt_minecart", vec![(key, value)]), "cart").is_err());
        }
    }

    #[test]
    fn unrepresentable_behaviors_report_the_field_path() {
        for (field, data) in [
            ("lock", Value::Compound(vec![])),
            ("attributes", Value::List(vec![Value::Compound(vec![])], 10)),
            ("acceleration_power", Value::Double(0.1)),
        ] {
            let error = downgrade(&mut c("chest", vec![(field, data)]), "Regions/r/entity").unwrap_err().to_string();
            assert!(error.contains(&format!("Regions/r/entity/{field}")));
        }
    }

    #[test]
    fn renamed_entity_data_is_rejected_before_the_target_can_ignore_it() {
        for (id, key) in [
            ("banner", "patterns"), ("skull", "profile"), ("skull", "custom_name"),
            ("beehive", "bees"), ("beehive", "flower_pos"), ("bee", "hive_pos"),
            ("end_gateway", "exit_portal"), ("end_crystal", "beam_target"),
            ("area_effect_cloud", "potion_contents"), ("cow", "leash"),
            ("pillager", "patrol_target"), ("wandering_trader", "wander_target"),
        ] {
            let error = downgrade(&mut c(id, vec![(key, Value::Compound(vec![]))]), "entity").unwrap_err().to_string();
            assert!(error.contains(&format!("entity/{key}")));
        }
        let arrow = c("arrow", vec![("item", c("tipped_arrow", vec![]))]);
        assert!(downgrade(&mut arrow.clone(), "arrow").is_err());
    }

    #[test]
    fn block_entity_custom_metadata_never_replaces_current_inventory() {
        let metadata = Value::Compound(vec![("minecraft:custom_data".into(), Value::Compound(vec![
            ("BlockEntityTag".into(), Value::Compound(vec![("Items".into(), Value::List(vec![], 10))])),
        ]))]);
        let original = c("shulker_box", vec![
            ("Items", Value::List(vec![c("stone", vec![("Count", Value::Byte(1))])], 10)),
            ("components", metadata),
        ]);
        let mut value = original.clone();
        downgrade(&mut value, "box").unwrap();
        assert_eq!(value, original);
        value.set("components", Value::Compound(vec![("minecraft:custom_name".into(), Value::String("\"命名\"".into()))]));
        assert!(downgrade(&mut value, "box").unwrap_err().to_string().contains("minecraft:custom_name"));
    }
}
