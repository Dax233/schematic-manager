//! 前后端交换的数据结构。
//!
//! 这里刻意与内部的 litematic 模型分开：内部模型会随解析需求演进，
//! 而这里的字段是对前端的稳定契约，字段名采用前端惯用的驼峰命名。

use serde::{Deserialize, Serialize};

/// 投影文件的摘要信息（列表展示用，不解码方块）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchematicSummary {
    pub path: String,
    pub file_name: String,
    pub dir_name: String,
    pub file_size: u64,
    /// 解析失败时的原因；为空表示有效
    pub error: String,

    pub format_version: i32,
    pub data_version: i32,
    pub mc_version_name: String,

    pub name: String,
    pub author: String,
    pub description: String,
    pub time_created: i64,
    pub time_modified: i64,

    pub enclosing: [i32; 3],
    pub total_blocks: i64,
    pub total_volume: i64,
    pub region_count: i32,
    pub entity_count: i64,
    pub tile_entity_count: i64,
    pub pending_ticks: i64,
    pub has_preview: bool,
}

/// 某个方块的统计条目
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockCount {
    pub name: String,
    pub state: String,
    /// 堆叠数（按 64 一组换算后的可读形式另由前端处理）
    pub count: u64,
}

/// 实体统计条目
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityCount {
    pub id: String,
    pub count: u64,
}

/// 目录扫描结果
///
/// 带上 skipped：不可读的子目录会被跳过，若不告知用户，
/// 结果看起来是完整的，用户会以为文件真的不存在。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub items: Vec<SchematicSummary>,
    /// 因权限或 IO 错误被跳过的目录数
    pub skipped_dirs: u32,
    /// 被跳过的目录路径（最多记录前若干个，便于用户定位）
    pub skipped_paths: Vec<String>,
}

/// 保存操作的结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveOutcome {
    pub path: String,
    /// 保存前生成的备份文件位置
    pub backup_path: String,
}

/// 完整分析结果
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisResult {
    pub blocks: Vec<BlockCount>,
    pub entities: Vec<EntityCount>,
    pub tile_entities: Vec<EntityCount>,
    pub non_air_total: u64,
    pub air_total: u64,
    pub distinct_block_types: usize,
    pub distinct_states: usize,
}
