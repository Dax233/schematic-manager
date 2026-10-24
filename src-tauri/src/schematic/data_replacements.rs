//! 发现需要降级替换的物品与实体，并校验用户选定的旧版替代品。
use crate::error::{AppError, AppResult};
use crate::format::nbt::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind { Item, Entity }

/// 一类缺失内容的替换选择；实体目标可包含旧版船的 Type。
#[derive(Debug, Clone)]
pub struct Choice {
    pub kind: Kind,
    pub name: String,
    pub replacement: String,
}

#[derive(Debug, Clone)]
pub struct Missing {
    pub kind: Kind,
    pub name: String,
    pub count: u64,
    pub suggested: String,
    pub options: Vec<String>,
    pub paths: Vec<String>,
}

type Counts = BTreeMap<(Kind,String),(u64,Vec<String>)>;

fn record(counts: &mut Counts, kind: Kind, id: &str, count: u64, path: &str) {
    let entry=counts.entry((kind,id.into())).or_default();
    entry.0+=count;
    entry.1.push(path.into());
}

pub const MARKER: &str = "minecraft:red_stained_glass_pane";
const WOODS: &[&str] = &["oak", "spruce", "birch", "jungle", "acacia", "dark_oak", "mangrove", "cherry", "bamboo"];

fn known(id: &str, names: &str) -> bool {
    id.strip_prefix("minecraft:").is_some_and(|name| names.lines().any(|n| n == name))
}

pub fn known_item(id: &str) -> bool { known(id, include_str!("../../data/items-1.20.4.txt")) }

pub fn boat(id: &str) -> Option<(&str, &str)> {
    let name = id.strip_prefix("minecraft:")?;
    if matches!(name, "boat" | "chest_boat") { return None; }
    name.strip_suffix("_chest_boat").map(|wood| (wood, "chest_boat"))
        .or_else(|| name.strip_suffix("_boat").map(|wood| (wood, "boat")))
        .or_else(|| (name == "bamboo_raft").then_some(("bamboo", "boat")))
        .or_else(|| (name == "bamboo_chest_raft").then_some(("bamboo", "chest_boat")))
}

pub fn entity_target(target: &str) -> Option<(&str, &str)> {
    let (id, wood) = target.split_once("[Type=")?;
    let wood = wood.strip_suffix(']')?;
    (matches!(id, "minecraft:boat" | "minecraft:chest_boat") && WOODS.contains(&wood)).then_some((id, wood))
}

fn missing(kind: Kind, name: String, count: u64, paths: Vec<String>) -> Missing {
    let (suggested, options) = match kind {
        Kind::Item => {
            let suggested = name.strip_prefix("minecraft:pale_oak_")
                .map(|part| format!("minecraft:birch_{part}"))
                .or_else(|| name.strip_prefix("minecraft:stripped_pale_oak_").map(|part| format!("minecraft:stripped_birch_{part}")))
                .filter(|id| known_item(id)).unwrap_or_else(|| MARKER.into());
            let options = if name.starts_with("minecraft:music_disc_") {
                vec![MARKER.into()]
            } else {
                include_str!("../../data/items-1.20.4.txt").lines().map(|n| format!("minecraft:{n}")).collect()
            };
            (suggested, options)
        }
        Kind::Entity => match boat(&name).filter(|(wood, _)| *wood == "pale_oak") {
            Some((_, id)) => (format!("minecraft:{id}[Type=birch]"),
                WOODS.iter().map(|wood| format!("minecraft:{id}[Type={wood}]")).collect()),
            None => (String::new(), vec![]),
        },
    };
    Missing {kind, name, count, suggested, options, paths}
}

/// 只遍历游戏实际使用的库存与实体字段，不把任意自定义数据当作当前物品。
pub fn analyze(root: &Value, source: i32, target: i32) -> AppResult<Vec<Missing>> {
    if source < 3837 || target >= 3837 { return Ok(vec![]); }
    if source > 4189 || target != 3700 {
        return Err(AppError::UnsupportedVersion("物品和实体降级目前支持 1.20.5–1.21.4 → 1.20.4".into()));
    }
    let mut counts = BTreeMap::new();
    if let Some(Value::Compound(regions)) = root.get("Regions") {
        for (name, region) in regions {
            for key in ["Entities", "TileEntities"] {
                if let Some(entries) = region.get(key).and_then(Value::as_list) {
                    for (i,entry) in entries.iter().enumerate() { scan_contents(entry, key == "Entities", &format!("Regions/{name}/{key}[{i}]"), &mut counts); }
                }
            }
        }
    }
    Ok(counts.into_iter().map(|((kind,name),(count,paths))| missing(kind,name,count,paths)).collect())
}

fn scan_item(value: &Value, path: &str, counts: &mut Counts) {
    if let Some(id) = value.get("id").and_then(Value::as_str) {
        if !known_item(id) {
            let count = value.get("count").or_else(|| value.get("Count")).and_then(Value::as_i64).unwrap_or(1).max(0) as u64;
            record(counts,Kind::Item,id,count,path);
        }
    }
    if let Some(data) = value.get("components") {
        if let Some(items) = data.get("minecraft:container").and_then(Value::as_list) {
            for (i,entry) in items.iter().enumerate() { if let Some(item) = entry.get("item") { scan_item(item,&format!("{path}/components/minecraft:container[{i}]/item"),counts); } }
        }
        if let Some(block) = data.get("minecraft:block_entity_data") { scan_contents(block,false,&format!("{path}/components/minecraft:block_entity_data"),counts); }
    }
}

fn scan_contents(value: &Value, entity: bool, path: &str, counts: &mut Counts) {
    if entity {
        if let Some(id) = value.get("id").and_then(Value::as_str) {
            let supported_boat = boat(id).is_some_and(|(wood,_)| WOODS.contains(&wood));
            if !supported_boat && !known(id,include_str!("../../data/entities-1.20.4.txt")) {
                record(counts,Kind::Entity,id,1,path);
            }
        }
    }
    for key in ["Items","Inventory","HandItems","ArmorItems"] {
        if let Some(items) = value.get(key).and_then(Value::as_list) { for (i,item) in items.iter().enumerate() {scan_item(item,&format!("{path}/{key}[{i}]"),counts);} }
    }
    for key in ["Item","SelectedItem","RecordItem","Book","item","SaddleItem","ArmorItem","DecorItem"] {
        if let Some(item) = value.get(key) { scan_item(item,&format!("{path}/{key}"),counts); }
    }
    if let Some(items) = value.get("Passengers").and_then(Value::as_list) {
        for (i,item) in items.iter().enumerate() { scan_contents(item,true,&format!("{path}/Passengers[{i}]"),counts); }
    }
    if let Some(entity) = value.get("SpawnData").and_then(|v| v.get("entity")) { scan_contents(entity,true,&format!("{path}/SpawnData/entity"),counts); }
    if let Some(entries) = value.get("SpawnPotentials").and_then(Value::as_list) {
        for (i,entry) in entries.iter().enumerate() {
            if let Some(entity) = entry.get("data").and_then(|v| v.get("entity")) {scan_contents(entity,true,&format!("{path}/SpawnPotentials[{i}]/data/entity"),counts);}
        }
    }
}

pub fn validate(missing: &[Missing], choices: &[Choice]) -> AppResult<()> {
    let mut seen = std::collections::BTreeSet::new();
    for choice in choices {
        if !seen.insert((choice.kind,choice.name.as_str())) {
            return Err(AppError::Parse(format!("替换条目重复：{}",choice.name)));
        }
        let row = missing.iter().find(|row| row.kind==choice.kind && row.name==choice.name)
            .ok_or_else(|| AppError::Parse(format!("替换清单已变化，请重新分析：{}",choice.name)))?;
        if !row.options.contains(&choice.replacement) {
            return Err(AppError::UnsupportedVersion(format!("{} 没有选择受支持的替代品：{}；位置：{}",choice.name,choice.replacement,row.paths.join("、"))));
        }
    }
    if missing.len()!=choices.len() {return Err(AppError::Parse("物品或实体替换选择不完整，请重新分析".into()));}
    Ok(())
}
