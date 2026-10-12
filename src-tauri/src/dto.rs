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

/// 方块替换规则
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceRuleDto {
    pub from_name: String,
    #[serde(default)]
    pub from_props: Vec<(String, String)>,
    pub to_name: String,
    #[serde(default)]
    pub to_props: Vec<(String, String)>,
    #[serde(default = "default_true")]
    pub inherit_props: bool,
}

fn default_true() -> bool {
    true
}

impl From<ReplaceRuleDto> for crate::replace::ReplaceRule {
    fn from(d: ReplaceRuleDto) -> Self {
        Self {
            from_name: d.from_name,
            from_props: d.from_props,
            to_name: d.to_name,
            to_props: d.to_props,
            inherit_props: d.inherit_props,
        }
    }
}

/// 方块替换结果
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceOutcomeDto {
    pub replaced: u64,
    pub palette_changed: usize,
}

impl From<crate::replace::ReplaceOutcome> for ReplaceOutcomeDto {
    fn from(o: crate::replace::ReplaceOutcome) -> Self {
        Self { replaced: o.replaced, palette_changed: o.palette_changed }
    }
}

/// 方块检索结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockHitDto {
    pub name: String,
    pub zh: String,
    pub en: String,
    pub versions: Vec<BlockVersionDto>,
}

/// 方块所在的版本
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockVersionDto {
    pub index: usize,
    pub data_version: i32,
}

/// 版本选项
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionOptionDto {
    pub index: usize,
    pub name: String,
    pub data_version: i32,
    pub is_overlay: bool,
    pub block_count: usize,
}

/// 转换中需要处理的一项
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvertItemDto {
    pub name: String,
    pub count: u64,
    pub suggested: String,
    pub chosen: String,
}

/// 可作为转换目标的版本
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvertTargetDto {
    pub index: usize,
    pub name: String,
    pub data_version: i32,
}

/// 转换结果
// 不能派生 Copy：该结构体含 String 字段，Copy 无法实现。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvertOutcomeDto {
    pub replaced_kinds: usize,
    pub replaced_blocks: u64,
    pub skipped_kinds: usize,
    /// 新文件的路径
    pub output_path: String,
}

/// 一组内容相同的投影文件
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateGroupDto {
    pub size: u64,
    pub paths: Vec<String>,
}

/// 重复文件筛查结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateScanDto {
    pub groups: Vec<DuplicateGroupDto>,
    /// 读取失败、未能参与判定的文件及原因
    pub unreadable: Vec<UnreadableFileDto>,
}

/// 未能读取的文件
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnreadableFileDto {
    pub path: String,
    pub reason: String,
}

/// 一条备份记录
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupEntryDto {
    pub path: String,
    pub stem: String,
    pub op: String,
    /// 纳秒时间戳，以十进制字符串传递。
    /// 纳秒量级约 1.8e18，超出 JavaScript 安全整数范围（约 9e15），
    /// 用数值传递会丢精度，因此这里用字符串。
    pub created_ns: String,
    pub size: u64,
}

/// 场景批次：供前端一个材质使用
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneBatchDto {
    pub texture: u32,
    pub positions: Vec<f32>,
    pub uvs: Vec<f32>,
    pub shades: Vec<f32>,
    pub indices: Vec<u32>,
    pub texture_width: u16,
    pub texture_height: u16,
    /// RGBA8888 像素；前端用 DataTexture 直接建纹理
    pub texture_rgba: Vec<u8>,
}

/// 前端渲染所需的完整场景
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneDto {
    pub batches: Vec<SceneBatchDto>,
    pub total_vertices: usize,
    pub total_triangles: usize,
    pub enclosing: [i32; 3],
    pub origin: [i32; 3],
    pub mc_version_name: String,
    pub data_version: i32,
    pub unresolved_blocks: usize,
}

impl From<crate::scene::SceneData> for SceneDto {
    fn from(s: crate::scene::SceneData) -> Self {
        Self {
            batches: s
                .batches
                .into_iter()
                .map(|b| SceneBatchDto {
                    texture: b.texture,
                    positions: b.positions,
                    uvs: b.uvs,
                    shades: b.shades,
                    indices: b.indices,
                    texture_width: b.texture_width,
                    texture_height: b.texture_height,
                    texture_rgba: b.texture_rgba,
                })
                .collect(),
            total_vertices: s.total_vertices,
            total_triangles: s.total_triangles,
            enclosing: s.enclosing,
            origin: s.origin,
            mc_version_name: s.mc_version_name,
            data_version: s.data_version,
            unresolved_blocks: s.unresolved_blocks,
        }
    }
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
