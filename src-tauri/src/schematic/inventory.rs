//! 只读取实际库存字段，汇总填充材料；不遍历任意元数据或展示装备。
use crate::error::{AppError, AppResult};
use crate::format::nbt::Value;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::OnceLock;

#[derive(Deserialize)]
struct StackSizes {
    versions: BTreeMap<i32, usize>,
    snapshots: Vec<BTreeMap<String, u32>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemCount {
    pub name: String,
    pub count: u64,
    pub boxes: Option<f64>,
    pub zh_name: String,
    pub en_name: String,
}

fn stack_size(version: i32, id: &str, item: &Value) -> Option<u32> {
    if let Some(value) = item.get("components").and_then(|v|v.get("minecraft:max_stack_size")) {
        return value.as_i64().filter(|v| (1..=99).contains(v)).map(|v|v as u32);
    }
    static SIZES: OnceLock<StackSizes> = OnceLock::new();
    let sizes = SIZES.get_or_init(|| {
        let bytes = crate::format::nbt::gunzip(include_bytes!("../../data/item-stacks.json.gz")).expect("内置物品堆叠数据损坏");
        serde_json::from_slice(&bytes).expect("内置物品堆叠数据格式错误")
    });
    sizes.versions.get(&version).and_then(|i| sizes.snapshots.get(*i))
        .and_then(|table|table.get(id.strip_prefix("minecraft:")?)).copied()
}

struct Counter { version: i32, rows: BTreeMap<String, ItemCount> }

impl Counter {
    fn item(&mut self, item: &Value, path: &str, multiplier: u64) -> AppResult<()> {
        if matches!(item, Value::Compound(fields) if fields.is_empty()) {return Ok(());}
        let id = item.get("id").and_then(Value::as_str).ok_or_else(||AppError::Parse(format!("{path}: 库存物品缺少 id")))?;
        if id == "minecraft:air" {return Ok(());}
        let count = item.get("count").or_else(||item.get("Count"));
        let n = count.and_then(Value::as_i64).or_else(||(count.is_none() && self.version >= 3837).then_some(1))
            .filter(|n|*n >= 0).ok_or_else(||AppError::Parse(format!("{path}: 库存物品数量无效")))? as u64;
        let n = n.checked_mul(multiplier).ok_or_else(||AppError::Parse(format!("{path}: 嵌套物品数量溢出")))?;
        if n == 0 {return Ok(());}
        let capacity = stack_size(self.version, id, item);
        let row = self.rows.entry(id.into()).or_insert_with(||ItemCount{name:id.into(), count:0, boxes:Some(0.0), zh_name:String::new(), en_name:String::new()});
        row.count = row.count.checked_add(n).ok_or_else(||AppError::Parse(format!("{path}: 物品总数溢出")))?;
        row.boxes = row.boxes.zip(capacity).map(|(boxes, size)| boxes + n as f64 / (27.0 * size as f64));
        // 新版 container 是当前库存；旧 block_entity_data 可能同时含有冗余备份。
        if let Some(data) = item.get("components") {
            if let Some(items) = data.get("minecraft:container") {return self.component_items(items, path, n);}
            if let Some(items) = data.get("minecraft:bundle_contents") {
                for (i,item) in self.list(items,path)?.iter().enumerate() {self.item(item,&format!("{path}/bundle_contents[{i}]"),n)?;}
                return Ok(());
            }
            if let Some(block) = data.get("minecraft:block_entity_data") {return self.contents(block, path, n);}
        }
        if let Some(block) = item.get("tag").and_then(|tag|tag.get("BlockEntityTag")) {self.contents(block, path, n)?;}
        if id == "minecraft:bundle" {
            if let Some(tag) = item.get("tag") {self.contents(tag,path,n)?;}
        }
        Ok(())
    }

    fn list<'a>(&self, value: &'a Value, path: &str) -> AppResult<&'a [Value]> {
        value.as_list().ok_or_else(||AppError::Parse(format!("{path}: 库存字段不是列表")))
    }

    fn component_items(&mut self, value: &Value, path: &str, multiplier: u64) -> AppResult<()> {
        for (i, entry) in self.list(value,path)?.iter().enumerate() {
            let path = format!("{path}/container[{i}]");
            let item = entry.get("item").ok_or_else(||AppError::Parse(format!("{path}: 缺少 item")))?;
            self.item(item,&path,multiplier)?;
        }
        Ok(())
    }

    fn slots(&mut self, value: &Value, path: &str, multiplier: u64) -> AppResult<()> {
        if let Some(items) = value.get("components").and_then(|v|v.get("minecraft:container")) {
            return self.component_items(items,path,multiplier);
        }
        for key in ["Items", "Inventory"] {
            if let Some(items) = value.get(key) {
                for (i, item) in self.list(items,path)?.iter().enumerate() {self.item(item,&format!("{path}/{key}[{i}]"),multiplier)?;}
            }
        }
        Ok(())
    }

    fn contents(&mut self, value: &Value, path: &str, multiplier: u64) -> AppResult<()> {
        self.slots(value,path,multiplier)?;
        for key in ["RecordItem", "Book", "item"] {
            if let Some(item) = value.get(key) {self.item(item,&format!("{path}/{key}"),multiplier)?;}
        }
        Ok(())
    }

    fn entity(&mut self, value: &Value, path: &str) -> AppResult<()> {
        self.slots(value,path,1)?;
        if let Some(passengers) = value.get("Passengers") {
            for (i,p) in self.list(passengers,path)?.iter().enumerate() {self.entity(p,&format!("{path}/Passengers[{i}]"))?;}
        }
        Ok(())
    }
}

/// 汇总投影全部子区的库存，按物品 ID 合并；盒数为理论容量折算，不按名称/附魔拆分。
pub fn count(root: &Value, version: i32) -> AppResult<Vec<ItemCount>> {
    let mut counter = Counter {version, rows:BTreeMap::new()};
    if let Some(Value::Compound(regions)) = root.get("Regions") {
        for (name,region) in regions {
            for key in ["TileEntities", "Entities"] {
                if let Some(entries) = region.get(key) {
                    for (i,entry) in counter.list(entries,key)?.iter().enumerate() {
                        let path = format!("Regions/{name}/{key}[{i}]");
                        if key == "Entities" {counter.entity(entry,&path)?;} else {counter.contents(entry,&path,1)?;}
                    }
                }
            }
        }
    }
    let mut rows:Vec<_> = counter.rows.into_values().collect();
    rows.sort_by(|a,b|b.count.cmp(&a.count).then_with(||a.name.cmp(&b.name)));
    Ok(rows)
}

#[cfg(test)]
#[path = "../tests/schematic/inventory.rs"]
mod tests;
