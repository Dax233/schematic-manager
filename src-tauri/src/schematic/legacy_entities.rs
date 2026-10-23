//! 将实体与方块实体的结构字段还原为 1.20.4 格式。
//!
//! 物品递归由 legacy_items 负责；无法等价表达的行为必须在写出前拒绝。
use crate::error::{AppError, AppResult};
use crate::format::nbt::Value;

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
        let old_name = include_str!("../../data/attributes-1.20.4.txt").lines().find(|known| {
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
    if !id.strip_prefix("minecraft:").is_some_and(|name| include_str!("../../data/entities-1.20.4.txt").lines().any(|known| known == name)) {
        return Err(invalid(path, &format!("目标版本不存在实体：{id}")));
    }
    Ok(())
}

#[cfg(test)]
#[path = "../tests/schematic/legacy_entities.rs"]
mod tests;
