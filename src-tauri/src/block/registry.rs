//! 精确游戏版本的方块注册名、属性域与默认值；独立于可渲染模型。

use crate::block::db::{BlockDef, VersionDef};
use crate::schematic::model::PropDomain;
use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};
use std::sync::OnceLock;

pub type Properties = (PropDomain, BTreeMap<String, String>);
pub type Registry = BTreeMap<String, Properties>;

#[derive(Deserialize)]
struct Reports {
    versions: BTreeMap<i32, usize>,
    snapshots: Vec<Registry>,
}

/// 只查询精确数据版本；不以相邻版本或模型条件推测缺失的注册信息。
pub fn get(data_version: i32) -> Option<&'static Registry> {
    static REPORTS: OnceLock<Reports> = OnceLock::new();
    let reports = REPORTS.get_or_init(|| {
        let bytes = crate::format::nbt::gunzip(include_bytes!("../../data/block-registry.json.gz"))
            .expect("内置方块注册信息压缩数据损坏");
        serde_json::from_slice(&bytes).expect("内置方块注册信息结构损坏")
    });
    reports.versions.get(&data_version).and_then(|i| reports.snapshots.get(*i))
}

/// 修复旧基础库漏收的注册名；不制造模型、不修改磁盘缓存或叠加包。
pub fn complete_names(version: &mut VersionDef) {
    if version.is_overlay { return; }
    let Some(registry) = get(version.data_version) else { return };
    let existing: HashSet<String> = version.blocks.iter().map(|b| b.name.clone()).collect();
    for name in registry.keys() {
        let name = format!("minecraft:{name}");
        if !existing.contains(&name) {
            version.blocks.push(BlockDef { name, variants: Vec::new(), multipart: Vec::new() });
        }
    }
}

#[cfg(test)]
#[path = "../tests/block/registry.rs"]
mod tests;
