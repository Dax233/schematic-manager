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

/// 一个已导入的资源包
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackEntryDto {
    /// 资源包文件名（在 packs 目录内）
    pub file: String,
    pub name: String,
    /// 来源 jar 路径
    pub source: String,
    /// 类型：0 内置、1 MC 版本包、2 模组包
    pub kind: i32,
    /// 类型的中文名，供界面直接显示
    pub kind_label: String,
    pub data_version: i32,
    /// 该数据版本对应的游戏版本名
    pub mc_version: String,
    pub enabled: bool,
    /// 资源包文件大小；文件缺失时为 0
    pub size: u64,
    /// 文件是否实际存在。清单可能与磁盘不一致（用户手工删过文件），
    /// 界面据此提示而不是显示一个点开就报错的条目。
    pub exists: bool,
}

/// 导入 jar 的结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportOutcomeDto {
    /// 落盘的资源包文件名
    pub file: String,
    pub name: String,
    pub kind: i32,
    pub kind_label: String,
    pub data_version: i32,
    pub mc_version: String,
    /// 产出统计
    pub blocks: usize,
    pub models: usize,
    pub textures: usize,
    /// 解析失败而被跳过的 blockstate 数
    pub failed_blockstates: usize,
    /// 因无法解析模型而被跳过的方块数
    pub failed_blocks: usize,
    /// 命名空间数
    pub namespaces: usize,
    /// 资源包文件大小
    pub size: u64,
}


/// 一个已启用却未能载入的资源包
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackLoadFailureDto {
    pub file: String,
    /// 失败原因
    pub reason: String,
}

/// 清单读取结果，含被跳过的行
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackListDto {
    pub packs: Vec<PackEntryDto>,
    /// 已启用但载入或合并失败的包：`(文件名, 原因)`。
    ///
    /// 必须上报：这类包在界面上看起来是启用状态，实际却不参与解析，
    /// 用户会以为贴图已经生效而困惑于「为什么没变化」。
    pub load_failures: Vec<PackLoadFailureDto>,
    /// 清单里解析失败而被跳过的行说明。
    /// 必须上报：用户手工编辑过清单时，静默丢弃会让「导入的包不见了」
    /// 无从解释。
    pub skipped: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 取排序后的键名。
    fn keys(v: &serde_json::Value) -> Vec<String> {
        let mut k: Vec<String> = v
            .as_object()
            .expect("应是对象")
            .keys()
            .cloned()
            .collect();
        k.sort();
        k
    }

    /// 断言键名集合（**忽略顺序**）。
    ///
    /// 两端都排序：手写顺序极易出错，例如 `failedBlocks` 是
    /// `failedBlockstates` 的前缀、排序时在前，而按业务语序写就会写成
    /// 相反的顺序。这类错误只在 CI 才暴露（本地 check 不编译 cfg(test)）。
    fn assert_keys(v: &serde_json::Value, expected: &[&str]) {
        let mut want: Vec<String> = expected.iter().map(|s| s.to_string()).collect();
        want.sort();
        assert_eq!(keys(v), want);
    }

    /// 前端的字段名是驼峰；后端靠 `#[serde(rename_all = "camelCase")]` 转换。
    /// 这条契约没有任何编译期保障：改名或漏写属性都不会编译失败，
    /// 只会在运行时表现为「前端读到的字段是 undefined」。
    ///
    /// 因此这里对每个跨边界的 DTO 断言其**序列化后的键名**，
    /// 与前端 `src/types.ts` 的字段逐一对应。
    ///
    /// 注意：本地检查环境用 serde 桩（无过程宏，rename_all 不生效），
    /// 该测试只在 CI 的真实 serde 下运行。
    #[test]
    fn pack_entry_keys_match_frontend() {
        let dto = PackEntryDto {
            file: "a.pack".into(),
            name: "A".into(),
            source: "/j/a.jar".into(),
            kind: 2,
            kind_label: "模组包".into(),
            data_version: 3465,
            mc_version: "1.20.1".into(),
            enabled: true,
            size: 123,
            exists: true,
        };
        let v = serde_json::to_value(&dto).expect("序列化失败");
        assert_keys(&v, &[
                "dataVersion", "enabled", "exists", "file", "kind", "kindLabel",
                "mcVersion", "name", "size", "source",
            ]);
    }

    #[test]
    fn import_outcome_keys_match_frontend() {
        let dto = ImportOutcomeDto {
            file: "a.pack".into(),
            name: "A".into(),
            kind: 1,
            kind_label: "MC 版本包".into(),
            data_version: 3465,
            mc_version: "1.20.1".into(),
            blocks: 1,
            models: 2,
            textures: 3,
            // 易错项：不是 failedBlockStates，而是一个词
            failed_blockstates: 4,
            failed_blocks: 5,
            namespaces: 6,
            size: 7,
        };
        let v = serde_json::to_value(&dto).expect("序列化失败");
        assert_keys(&v, &[
                "blocks", "dataVersion", "failedBlockstates", "failedBlocks", "file",
                "kind", "kindLabel", "mcVersion", "models", "name", "namespaces",
                "size", "textures",
            ]);
    }

    #[test]
    fn pack_list_keys_match_frontend() {
        let dto = PackListDto {
            packs: Vec::new(),
            load_failures: vec![PackLoadFailureDto {
                file: "broken.pack".into(),
                reason: "载入失败：文件不存在".into(),
            }],
            skipped: vec!["第 3 行有误".into()],
        };
        let v = serde_json::to_value(&dto).expect("序列化失败");
        assert_keys(&v, &["loadFailures", "packs", "skipped"]);
    }


    /// 载入失败项的字段也要对上：前端按 file/reason 渲染
    #[test]
    fn pack_load_failure_keys_match_frontend() {
        let dto = PackLoadFailureDto {
            file: "a.pack".into(),
            reason: "载入失败".into(),
        };
        let v = serde_json::to_value(&dto).expect("序列化失败");
        assert_keys(&v, &["file", "reason"]);
    }

    /// 纳秒时间戳必须以字符串传递：数值会超出 JavaScript 的安全整数范围
    #[test]
    fn backup_entry_created_ns_is_a_string() {
        let dto = BackupEntryDto {
            path: "/b/x".into(),
            stem: "x".into(),
            op: "编辑".into(),
            created_ns: "1791826869720983600".into(),
            size: 12,
        };
        let v = serde_json::to_value(&dto).expect("序列化失败");
        assert!(
            v.get("createdNs").expect("应有 createdNs").is_string(),
            "createdNs 必须是字符串，用数值会丢精度"
        );
    }

    #[test]
    fn convertible_keys_are_camel_case() {
        // 抽查几个既有 DTO，防止后续改动破坏既有契约
        let save = SaveOutcome {
            path: "/x".into(),
            backup_path: "/b/x".into(),
        };
        let v = serde_json::to_value(&save).unwrap();
        assert_keys(&v, &["backupPath", "path"]);

        let ver = VersionOptionDto {
            index: 0,
            name: "1.20.1".into(),
            data_version: 3465,
            is_overlay: false,
            block_count: 1005,
        };
        let v = serde_json::to_value(&ver).unwrap();
        assert_keys(&v, &["blockCount", "dataVersion", "index", "isOverlay", "name"]);
    }
}
