//! .litematic 投影文件模型。
//!
//! 对照 legacy-cpp/litematic.cpp 实现，保留其已验证语义：
//!   - 完整保留解析后的 NBT 树（root），未修改的字段原样写回，零信息损失
//!   - BlockStates 采用「跨 long 连续打包、低位在前」，与 Litematica 一致
//!   - 区域 Size 各分量可能为负（表示向负方向延伸），加载时归一化
//!   - 索引顺序为 X 最快、Z 次之、Y 最慢
//!   - 兼容 v6 的 BlockStatePalette 与 v7 的 Palette
//!
//! 参考格式：
//!   root
//!     MinecraftDataVersion: Int
//!     Version: Int            投影格式版本（v5/v6/v7）
//!     SubVersion: Int
//!     Metadata: Compound { Name, Author, Description, TimeCreated, TimeModified,
//!                          EnclosingSize: {x,y,z}, RegionCount, TotalBlocks, ... }
//!     Regions: Compound { <区域名>: Region }

use crate::error::{AppError, AppResult};
use crate::nbt::{self, Value};
use std::collections::BTreeMap;

/// 单个区域的方块数上限。
///
/// 体积来自文件里的 Size 三个分量，是纯粹不可信的外部输入：不设上界时
/// 三个 i32::MAX 相乘会溢出（debug panic、release 回绕成任意值），
/// 回绕结果还会被用来分配 indices（每个方块 4 字节）。
///
/// 取 5000 万：已远超真实投影的量级（最大规模的工程也用不到千万级），
/// 同时把单次分配限制在约 200 MB，不会因一个畸形文件耗尽内存。
const MAX_REGION_VOLUME: usize = 50_000_000;

/// 方块状态：名称 + 属性（保持顺序，序列化时顺序稳定）
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BlockState {
    pub name: String,
    pub props: Vec<(String, String)>,
}

impl BlockState {
    pub fn new(name: &str) -> Self {
        Self { name: name.to_string(), props: Vec::new() }
    }

    /// "facing=east,half=bottom"
    pub fn prop_str(&self) -> String {
        self.props
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(",")
    }

    /// "minecraft:oak_stairs[facing=east,half=bottom]"
    pub fn key(&self) -> String {
        let ps = self.prop_str();
        if ps.is_empty() {
            self.name.clone()
        } else {
            format!("{}[{}]", self.name, ps)
        }
    }

    /// 去掉命名空间前缀
    #[cfg(test)]
    pub fn display_name(&self) -> &str {
        self.name.split_once(':').map(|(_, n)| n).unwrap_or(&self.name)
    }

    pub fn prop(&self, k: &str) -> Option<&str> {
        self.props.iter().find(|(pk, _)| pk == k).map(|(_, v)| v.as_str())
    }

    pub fn set_prop(&mut self, k: &str, v: &str) {
        if let Some(slot) = self.props.iter_mut().find(|(pk, _)| pk == k) {
            slot.1 = v.to_string();
            return;
        }
        // 属性按字典序排列，与游戏内的规范顺序一致
        self.props.push((k.to_string(), v.to_string()));
        self.props.sort_by(|a, b| a.0.cmp(&b.0));
    }

    /// 是否为空气，用于统计。
    ///
    /// 不含 structure_void：legacy-cpp 统计时只把 air / cave_air / void_air
    /// 计入空气（litematic.cpp:374-377），structure_void 算非空气。
    /// 而渲染时会跳过 structure_void（render.cpp:317）。
    /// 两处口径不同是有意的，因此这里不叫「渲染是否跳过」。
    pub fn is_air(&self) -> bool {
        matches!(
            self.name.as_str(),
            "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
        )
    }
}

/// 单个区域
#[derive(Debug, Clone, Default)]
pub struct Region {
    pub name: String,
    pub pos: [i32; 3],
    pub size: [i32; 3],
    pub volume: usize,
    pub bits: i32,
    pub palette: Vec<BlockState>,
    /// 每个方块的调色板索引，长度 = volume
    pub indices: Vec<i32>,
    pub entity_count: usize,
    pub tile_entity_count: usize,
    pub pending_block_ticks: usize,
    pub pending_fluid_ticks: usize,
}

impl Region {
    /// 线性索引：X 最快、Z 次之、Y 最慢
    pub fn index(&self, x: i32, y: i32, z: i32) -> usize {
        let sx = self.size[0] as usize;
        let sz = self.size[2] as usize;
        x as usize + z as usize * sx + y as usize * sx * sz
    }

    pub fn pos_from_index(&self, i: usize) -> [i32; 3] {
        let (sx, sz) = (self.size[0] as usize, self.size[2] as usize);
        if sx == 0 || sz == 0 {
            return [0, 0, 0];
        }
        let x = i % sx;
        let r = i / sx;
        let z = r % sz;
        let y = r / sz;
        [x as i32, y as i32, z as i32]
    }

    pub fn inside(&self, x: i32, y: i32, z: i32) -> bool {
        x >= 0 && y >= 0 && z >= 0 && x < self.size[0] && y < self.size[1] && z < self.size[2]
    }
}

// ---------------------------------------------------------------- 位打包

/// 读取跨 long 连续打包的第 index 个值。
///
/// Litematica 的打包方式是「整体连续」而非「按 long 对齐」：
/// 第 index 个值从比特位 index*bits 开始，可跨越 long 边界。
pub fn read_bits(arr: &[i64], index: usize, bits: i32) -> i32 {
    if bits <= 0 || bits > 32 {
        return 0;
    }
    let mask: u64 = if bits >= 64 { u64::MAX } else { (1u64 << bits) - 1 };
    let start_offset = match index.checked_mul(bits as usize) {
        Some(v) => v,
        None => return 0,
    };
    let start_arr = start_offset >> 6;
    let end_arr = ((index + 1) * bits as usize - 1) >> 6;
    let start_bit = (start_offset & 63) as u32;

    if start_arr >= arr.len() {
        return 0;
    }
    if start_arr == end_arr {
        return ((arr[start_arr] as u64) >> start_bit & mask) as i32;
    }
    if end_arr >= arr.len() {
        return 0;
    }
    let end_offset = 64 - start_bit;
    let mut v = (arr[start_arr] as u64) >> start_bit;
    v |= (arr[end_arr] as u64) << end_offset;
    (v & mask) as i32
}

/// 写入跨 long 连续打包的第 index 个值（只覆盖该值占用的比特位）
pub fn write_bits(arr: &mut [i64], index: usize, bits: i32, value: i32) {
    if bits <= 0 || bits > 32 {
        return;
    }
    let mask: u64 = if bits >= 64 { u64::MAX } else { (1u64 << bits) - 1 };
    let start_offset = index * bits as usize;
    let start_arr = start_offset >> 6;
    let end_arr = ((index + 1) * bits as usize - 1) >> 6;
    let start_bit = (start_offset & 63) as u32;
    let v = (value as u64) & mask;

    if start_arr >= arr.len() {
        return;
    }
    if start_arr == end_arr {
        arr[start_arr] = ((arr[start_arr] as u64 & !(mask << start_bit)) | (v << start_bit)) as i64;
        return;
    }
    if end_arr >= arr.len() {
        return;
    }
    let end_offset = 64 - start_bit;
    arr[start_arr] = ((arr[start_arr] as u64 & !(mask << start_bit)) | (v << start_bit)) as i64;
    arr[end_arr] = ((arr[end_arr] as u64 & !(mask >> end_offset)) | (v >> end_offset)) as i64;
}

/// 调色板大小所需的比特数。Litematica 最少用 2 位。
pub fn bits_for_palette(palette_size: usize) -> i32 {
    if palette_size <= 2 {
        return 2;
    }
    // ceil(log2(n))
    let mut bits = 2;
    while (1usize << bits) < palette_size {
        bits += 1;
    }
    bits as i32
}

/// 把索引数组编码成跨 long 打包的数组
/// 把调色板编码成 NBT 列表。
///
/// 每项为 Compound：Name 必填；Properties 为 属性名 -> 属性值，
/// 无属性时不写该字段——与 legacy-cpp 的写法一致，
/// 多写一个空 Compound 虽不影响读取，但会让文件与原版有差异。
pub fn encode_palette(palette: &[BlockState]) -> Value {
    Value::List(
        palette
            .iter()
            .map(|bs| {
                let mut fields = vec![("Name".to_string(), Value::String(bs.name.clone()))];
                if !bs.props.is_empty() {
                    fields.push((
                        "Properties".to_string(),
                        Value::Compound(
                            bs.props
                                .iter()
                                .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                                .collect(),
                        ),
                    ));
                }
                // 元素类型必须是 Compound，读取端依赖它
                Value::Compound(fields)
            })
            .collect(),
        10, // TAG_Compound
    )
}

pub fn encode_indices(indices: &[i32], bits: i32) -> Vec<i64> {
    if bits <= 0 || indices.is_empty() {
        return Vec::new();
    }
    let total_bits = indices.len() * bits as usize;
    let mut arr = vec![0i64; total_bits.div_ceil(64)];
    for (i, v) in indices.iter().enumerate() {
        write_bits(&mut arr, i, bits, *v);
    }
    arr
}

/// 把跨 long 打包的数组解码成索引数组
pub fn decode_indices(arr: &[i64], count: usize, bits: i32) -> Vec<i32> {
    (0..count).map(|i| read_bits(arr, i, bits)).collect()
}

// ---------------------------------------------------------------- 摘要

/// 只读元数据（用于列表快速扫描，不解码方块）
#[derive(Debug, Clone, Default)]
pub struct Summary {
    pub error: String,
    pub path: String,
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
    pub file_size: u64,
}

/// 完整分析结果
#[derive(Debug, Clone, Default)]
pub struct Analysis {
    /// 按完整状态统计（含属性）
    pub blocks_by_state: BTreeMap<String, u64>,
    pub entities_by_id: BTreeMap<String, u64>,
    pub tile_entities_by_id: BTreeMap<String, u64>,
    pub non_air_total: u64,
    pub air_total: u64,
}

// ---------------------------------------------------------------- 载入

/// 从 NBT 根节点构造 Summary（不解析方块数据）
pub fn summary_from_nbt(root: &Value) -> Summary {
    let mut s = Summary::default();
    s.format_version = root.get("Version").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    s.data_version = root
        .get("MinecraftDataVersion")
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32;
    s.mc_version_name = crate::mcver::version_name_from_data_version(s.data_version);

    if let Some(md) = root.get("Metadata") {
        let gs = |k: &str| md.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        s.name = gs("Name");
        s.author = gs("Author");
        s.description = gs("Description");
        s.time_created = md.get("TimeCreated").and_then(|v| v.as_i64()).unwrap_or(0);
        s.time_modified = md.get("TimeModified").and_then(|v| v.as_i64()).unwrap_or(0);
        s.total_blocks = md.get("TotalBlocks").and_then(|v| v.as_i64()).unwrap_or(0);
        s.total_volume = md.get("TotalVolume").and_then(|v| v.as_i64()).unwrap_or(0);
        s.region_count = md.get("RegionCount").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        // 实体/方块实体/待处理刻**不在 Metadata 里**：实测真实投影的 Metadata
        // 只有 TimeCreated、TimeModified、EnclosingSize、Description、
        // RegionCount、TotalBlocks、TotalVolume、Author、Name 这几项。
        // 这些数量必须按区域内的列表求和（legacy-cpp 的 Peek 也是这么做的，
        // 见 legacy-cpp/src/litematic.cpp:274-284）。此前读 Metadata 的同名键，
        // 一个都取不到，界面上「实体数量 / 方块实体 / 待处理刻」恒为 0。
        if let Some(es) = md.get("EnclosingSize") {
            if let Some(xyz) = es.as_xyz() {
                s.enclosing = xyz;
            }
        }
        if md.get("PreviewImageData").is_some() {
            s.has_preview = true;
        }
    }

    // 按区域内的列表统计数量。RegionCount 与实体数量都以实际内容为准：
    // Metadata 里可能缺失这些字段，而文件本身是有内容的。
    if let Some(Value::Compound(regs)) = root.get("Regions") {
        if s.region_count == 0 {
            s.region_count = regs.len() as i32;
        }
        for (_, rv) in regs.iter() {
            let len = |k: &str| -> i64 {
                rv.get(k).and_then(|x| x.as_list()).map_or(0, |l| l.len() as i64)
            };
            s.entity_count += len("Entities");
            s.tile_entity_count += len("TileEntities");
            s.pending_ticks += len("PendingBlockTicks") + len("PendingFluidTicks");
        }
    }
    s
}

/// 解析单个区域
/// 由区域尺寸算出方块数，并校验不会溢出、不超过上限。
///
/// 单独成函数是为了能直接测试边界：区域解析到这一步已经需要构造完整的
/// NBT 与足够长的 BlockStates，而这里只需三个整数。
///
/// 调用方必须先确认三个分量均为正（负值表示向负方向延伸，会先被归一化）。
fn region_volume(size: [i32; 3], name: &str) -> AppResult<usize> {
    let mut volume: usize = 1;
    for a in 0..3 {
        volume = volume
            .checked_mul(size[a] as usize)
            .ok_or_else(|| AppError::Corrupt(format!("区域 {name} 的尺寸过大，体积计算溢出")))?;
    }
    if volume > MAX_REGION_VOLUME {
        return Err(AppError::Corrupt(format!(
            "区域 {name} 的体积为 {volume}，超过上限 {MAX_REGION_VOLUME}（约合 {} MB 的方块索引）",
            MAX_REGION_VOLUME * 4 / 1024 / 1024
        )));
    }
    Ok(volume)
}

fn region_from_nbt(name: &str, v: &Value) -> AppResult<Region> {
    let mut r = Region { name: name.to_string(), ..Default::default() };

    let pos = v
        .get("Position")
        .and_then(|p| p.as_xyz())
        .ok_or_else(|| AppError::Parse(format!("区域 {name} 缺少 Position")))?;
    let mut size = v
        .get("Size")
        .and_then(|p| p.as_xyz())
        .ok_or_else(|| AppError::Parse(format!("区域 {name} 缺少 Size")))?;

    // Size 分量可能为负：表示向负方向延伸。
    // 此时原点要相应前移，尺寸取绝对值，否则后续索引计算全部错位。
    for a in 0..3 {
        if size[a] < 0 {
            // 取绝对值必须检查：i32::MIN 的相反数无法用 i32 表示。
            // 不检查的话，debug 构建会 panic，release 构建回绕后仍是负数，
            // 于是 r.pos 已被写成错位的值，区域却以「尺寸为 0」被放行，
            // 后续按 r.pos 计算场景原点就会把整个预览框到别处去。
            //
            // 顺序很重要：必须先用**原始负值**算出前移后的原点，再取绝对值。
            // 反过来会把正的尺寸当成偏移量，原点被推远一个尺寸（实测
            // Size.x=-16、Position.x=15 时得到 32 而非 0）。
            let new_pos = pos[a].wrapping_add(size[a]).wrapping_add(1);
            size[a] = size[a]
                .checked_neg()
                .ok_or_else(|| AppError::Corrupt(format!("区域 {name} 的 Size 分量无法取绝对值")))?;
            r.pos[a] = new_pos;
        } else {
            r.pos[a] = pos[a];
        }
    }
    r.size = size;

    // 尺寸为 0 的区域没有方块数据
    if size.iter().any(|&s| s <= 0) {
        return Ok(r);
    }

    // 体积必须用 checked 运算并设上界。
    // 三个分量各自可达 i32::MAX，直接相乘会溢出：debug 构建 panic，
    // release 构建回绕成一个任意值，随后被用来分配 indices
    // （每个方块 4 字节），既可能给出错误结果也可能耗尽内存。
    // 该值只受文件内容控制，因此必须当成不可信输入处理。
    r.volume = region_volume(size, name)?;

    // v7 用 Palette，v6 用 BlockStatePalette
    let pal_val = v
        .get("BlockStatePalette")
        .or_else(|| v.get("Palette"))
        .ok_or_else(|| AppError::Parse(format!("区域 {name} 缺少调色板")))?;
    let pal_list = pal_val
        .as_list()
        .ok_or_else(|| AppError::Parse(format!("区域 {name} 的调色板不是列表")))?;
    for item in pal_list {
        let n = item.get("Name").and_then(|x| x.as_str()).unwrap_or("minecraft:air");
        let mut bs = BlockState::new(n);
        if let Some(Value::Compound(props)) = item.get("Properties") {
            for (k, pv) in props {
                if let Some(val) = pv.as_str() {
                    bs.props.push((k.clone(), val.to_string()));
                }
            }
            bs.props.sort_by(|a, b| a.0.cmp(&b.0));
        }
        r.palette.push(bs);
    }

    r.bits = bits_for_palette(r.palette.len());

    // 解码方块索引。
    // 必须先用「文件声明的 Size」换算出所需 long 数并校验：
    // BlockStates 数组偏短时 read_bits 会一律返回 0，而索引 0 通常是空气，
    // 于是统计结果静默偏小——用户会看到一个「看起来正常但数字是错的」投影。
    if let Some(longs) = v.get("BlockStates").and_then(|x| x.as_longs()) {
        // volume 与 bits 都在上面受过约束（bits 最大 32），这里不会溢出
        let need = (r.volume * r.bits as usize).div_ceil(64);
        if longs.len() < need {
            return Err(AppError::Corrupt(format!(
                "区域 {name} 的 BlockStates 长度不足：按尺寸与位数需要 {need} 个 long，实际 {}",
                longs.len()
            )));
        }
        r.indices = decode_indices(longs, r.volume, r.bits);
        // 越界索引视为空气，避免后续取值 panic
        let pal_len = r.palette.len() as i32;
        for i in r.indices.iter_mut() {
            if *i < 0 || *i >= pal_len {
                *i = 0;
            }
        }
    } else {
        r.indices = vec![0; r.volume];
    }

    r.entity_count = v.get("Entities").and_then(|x| x.as_list()).map_or(0, |l| l.len());
    r.tile_entity_count = v.get("TileEntities").and_then(|x| x.as_list()).map_or(0, |l| l.len());
    r.pending_block_ticks =
        v.get("PendingBlockTicks").and_then(|x| x.as_list()).map_or(0, |l| l.len());
    r.pending_fluid_ticks =
        v.get("PendingFluidTicks").and_then(|x| x.as_list()).map_or(0, |l| l.len());

    Ok(r)
}

/// 已载入的投影文件
pub struct Schematic {
    pub root: Value,
    pub regions: Vec<Region>,
    pub path: String,
    /// 元数据（名称/作者/描述/数据版本）被改动
    pub meta_modified: bool,
    /// 方块索引被改动。只有它为真时才需要重新编码 BlockStates，
    /// 否则改一个描述也会重写全部区域的方块数据。
    pub blocks_modified: bool,
}

impl Schematic {
    /// 从文件载入
    pub fn load(path: &str) -> AppResult<Self> {
        let bytes = std::fs::read(path)?;
        let root = nbt::parse_maybe_gzip(&bytes)?;
        let mut sc = Self {
            root,
            regions: Vec::new(),
            path: path.to_string(),
            meta_modified: false,
            blocks_modified: false,
        };
        sc.rebuild_regions()?;
        Ok(sc)
    }

    /// 只读元数据扫描（不解码方块，适合批量列表）
    pub fn peek(path: &str) -> Summary {
        let mut s = Summary { path: path.to_string(), ..Default::default() };
        match std::fs::read(path) {
            Ok(bytes) => {
                s.file_size = bytes.len() as u64;
                match nbt::parse_maybe_gzip(&bytes) {
                    Ok(root) => {
                        let mut inner = summary_from_nbt(&root);
                        inner.path = path.to_string();
                        inner.file_size = s.file_size;
                        return inner;
                    }
                    Err(e) => {
                        s.error = e.to_string();
                        return s;
                    }
                }
            }
            Err(e) => {
                s.error = e.to_string();
                s
            }
        }
    }

    fn rebuild_regions(&mut self) -> AppResult<()> {
        self.regions.clear();
        if let Some(Value::Compound(regs)) = self.root.get("Regions") {
            for (name, v) in regs {
                self.regions.push(region_from_nbt(name, v)?);
            }
        }
        Ok(())
    }

    pub fn summary(&self) -> Summary {
        let mut s = summary_from_nbt(&self.root);
        s.path = self.path.clone();
        s
    }

    /// 统计方块与实体
    pub fn analyze(&self) -> Analysis {
        let mut a = Analysis::default();
        for r in &self.regions {
            for idx in &r.indices {
                let Some(b) = r.palette.get(*idx as usize) else { continue };
                if b.is_air() {
                    a.air_total += 1;
                    continue;
                }
                a.non_air_total += 1;
                *a.blocks_by_state.entry(b.key()).or_insert(0) += 1;
            }
        }
        for r in &self.regions {
            if let Some(Value::Compound(regs)) = self.root.get("Regions") {
                if let Some(rv) = regs.iter().find(|(n, _)| n == &r.name).map(|(_, v)| v) {
                    for key in ["Entities", "TileEntities"] {
                        if let Some(list) = rv.get(key).and_then(|x| x.as_list()) {
                            for e in list {
                                // NBT 的键区分大小写，实体与方块实体的标识字段
                                // 都是小写 `id`（legacy-cpp 也取 "id"）。
                                // 写成 "Id" 会一个都匹配不到，全部归入 unknown，
                                // 界面上就只剩一行 unknown。
                                let id = e.get("id").and_then(|x| x.as_str()).unwrap_or("unknown");
                                let map = if key == "Entities" {
                                    &mut a.entities_by_id
                                } else {
                                    &mut a.tile_entities_by_id
                                };
                                *map.entry(id.to_string()).or_insert(0) += 1;
                            }
                        }
                    }
                }
            }
        }
        a
    }

    // --- 元数据编辑（保持 NBT 结构，仅改对应字段） ---

    fn meta_set_str(&mut self, key: &str, v: &str) -> bool {
        let Some(md) = self.root.get("Metadata") else { return false };
        let Some(old) = md.get(key) else { return false };
        // 值未变化时不算改动：否则重复保存相同内容也会触发备份与写盘
        if old.as_str() == Some(v) {
            return false;
        }
        let mut md = md.clone();
        md.set(key, Value::String(v.to_string()));
        self.root.set("Metadata", md);
        self.meta_modified = true;
        true
    }

    pub fn set_meta_name(&mut self, v: &str) -> bool {
        self.meta_set_str("Name", v)
    }
    pub fn set_meta_author(&mut self, v: &str) -> bool {
        self.meta_set_str("Author", v)
    }
    pub fn set_meta_description(&mut self, v: &str) -> bool {
        self.meta_set_str("Description", v)
    }
    /// 把编辑后的索引重新编码回 NBT 并保存
    pub fn save_to(&mut self, path: &str) -> AppResult<()> {
        self.sync_to_nbt()?;
        let bytes = nbt::serialize_gzip(&self.root, "", 6)?;
        // 先写临时文件再改名，避免中途失败留下半个文件
        let tmp = format!("{path}.tmp");
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, path)?;
        self.meta_modified = false;
        self.blocks_modified = false;
        Ok(())
    }

    /// 把内存中的区域索引写回 NBT 的 BlockStates 字段
    fn sync_to_nbt(&mut self) -> AppResult<()> {
        // 只有方块数据真的被改动过才重新编码 BlockStates，
        // 否则改一个描述也会把全部区域的方块数据重写一遍。
        if !self.blocks_modified {
            return Ok(());
        }
        let region_names: Vec<String> = self.regions.iter().map(|r| r.name.clone()).collect();
        let mut regs = match self.root.get("Regions").cloned() {
            Some(Value::Compound(v)) => v,
            _ => {
                return Err(AppError::Parse(
                    "投影的 Regions 字段缺失或类型不正确，无法写回方块数据".into(),
                ))
            }
        };
        for name in region_names {
            let Some(mut r) = self.regions.iter().find(|r| r.name == name).cloned() else {
                continue;
            };
            if r.volume == 0 {
                continue;
            }
            let slot = regs.iter_mut().find(|(n, _)| n == &name).ok_or_else(|| {
                AppError::Parse(format!("NBT 中找不到区域 {name}，无法写回方块数据"))
            })?;

            // 调色板必须一并写回：替换会向 palette 追加新状态，
            // 只写索引的话，盘上的调色板仍是旧长度，新索引越界，
            // 游戏与 Litematica 会读到错误方块或直接判为损坏。
            //
            // 键名沿用文件原有的那个：v6 用 BlockStatePalette、v7 用 Palette。
            // 一律写 BlockStatePalette 会让 v7 文件多出一个字段而原字段仍是旧值。
            let pal_key = if slot.1.get("Palette").is_some() {
                "Palette"
            } else {
                "BlockStatePalette"
            };
            slot.1.set(pal_key, encode_palette(&r.palette));

            // 位宽也要按新的调色板长度重算：调色板增长后仍用旧位宽，
            // write_bits 会按掩码静默截断索引，写出的就是错数据。
            r.bits = bits_for_palette(r.palette.len());
            slot.1.set(
                "BlockStates",
                Value::LongArray(encode_indices(&r.indices, r.bits)),
            );
        }
        self.root.set("Regions", Value::Compound(regs));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;


    /// 方块改动后写回，调色板与位宽都必须同步更新。
    ///
    /// 防的是：替换向调色板追加了新状态，写回时却只写 BlockStates。
    /// 那样盘上的调色板仍是旧长度，新索引越界；调色板增长后位宽不变，
    /// write_bits 还会按掩码静默截断索引。产出的文件游戏读不出。
    ///
    /// 这里直接检查写回的 NBT，不经过文件与压缩，因此不受测试桩限制。
    #[test]
    fn sync_writes_palette_and_recomputes_bits() {
        // 初始调色板 1 项、位宽 2；改动后扩到 4 项，位宽应升到 3
        let region_nbt = Value::Compound(vec![
            (
                "BlockStatePalette".into(),
                encode_palette(&[BlockState::new("minecraft:stone")]),
            ),
            ("BlockStates".into(), Value::LongArray(vec![0i64])),
            ("Size".into(), Value::Compound(vec![
                ("x".into(), Value::Int(4)),
                ("y".into(), Value::Int(1)),
                ("z".into(), Value::Int(1)),
            ])),
            ("Position".into(), Value::Compound(vec![
                ("x".into(), Value::Int(0)),
                ("y".into(), Value::Int(0)),
                ("z".into(), Value::Int(0)),
            ])),
        ]);
        let mut sc = Schematic {
            root: Value::Compound(vec![(
                "Regions".into(),
                Value::Compound(vec![("r".into(), region_nbt)]),
            )]),
            regions: vec![Region {
                name: "r".into(),
                pos: [0, 0, 0],
                size: [4, 1, 1],
                volume: 4,
                bits: 2,
                palette: vec![BlockState::new("minecraft:stone")],
                indices: vec![0, 0, 0, 0],
                ..Default::default()
            }],
            path: String::new(),
            meta_modified: false,
            blocks_modified: false,
        };

        // 模拟替换：调色板扩到 4 项，最后两个方块改用第 4 项（下标 3）
        sc.regions[0].palette.push(BlockState::new("minecraft:gold_block"));
        sc.regions[0].palette.push(BlockState::new("minecraft:dirt"));
        sc.regions[0].palette.push(BlockState::new("minecraft:oak_planks"));
        sc.regions[0].indices = vec![0, 3, 1, 3];
        sc.blocks_modified = true;

        sc.sync_to_nbt().expect("写回失败");

        let regs = match sc.root.get("Regions") {
            Some(Value::Compound(v)) => v.clone(),
            _ => panic!("Regions 缺失"),
        };
        let rv = regs.iter().find(|(n, _)| n == "r").map(|(_, v)| v).expect("区域 r 缺失");

        // 调色板必须写回，且长度与内存一致
        let pal = match rv.get("BlockStatePalette") {
            Some(Value::List(items, elem)) => {
                assert_eq!(*elem, 10, "元素类型应为 Compound");
                items.clone()
            }
            other => panic!("应写回 BlockStatePalette，实际 {other:?}"),
        };
        assert_eq!(pal.len(), 4, "调色板应写回全部 4 项");
        let name_of = |v: &Value| match v {
            Value::Compound(f) => match f.iter().find(|(k, _)| k == "Name").map(|(_, v)| v) {
                Some(Value::String(s)) => s.clone(),
                _ => String::new(),
            },
            _ => String::new(),
        };
        assert_eq!(name_of(&pal[0]), "minecraft:stone");
        assert_eq!(name_of(&pal[3]), "minecraft:oak_planks", "新增项应写回");

        // 位宽必须按新的调色板长度重算：4 项需要 2 位，5 项才需要 3 位，
        // 但这里重要的是「重新计算过」而不是沿用旧值
        let expect_bits = bits_for_palette(4);
        let arr = match rv.get("BlockStates") {
            Some(Value::LongArray(a)) => a.clone(),
            _ => panic!("BlockStates 缺失"),
        };
        assert_eq!(
            read_bits(&arr, 1, expect_bits),
            3,
            "索引应能按新位宽正确读回（旧位宽会截断）"
        );
        assert_eq!(read_bits(&arr, 0, expect_bits), 0);
        assert_eq!(read_bits(&arr, 3, expect_bits), 3);
    }

    #[test]
    fn bits_for_palette_matches_litematica() {
        // 最少 2 位
        assert_eq!(bits_for_palette(0), 2);
        assert_eq!(bits_for_palette(1), 2);
        assert_eq!(bits_for_palette(2), 2);
        assert_eq!(bits_for_palette(3), 2);
        assert_eq!(bits_for_palette(4), 2);
        assert_eq!(bits_for_palette(5), 3);
        assert_eq!(bits_for_palette(8), 3);
        assert_eq!(bits_for_palette(9), 4);
        assert_eq!(bits_for_palette(16), 4);
        assert_eq!(bits_for_palette(17), 5);
        assert_eq!(bits_for_palette(61), 6);
    }

    #[test]
    fn bits_roundtrip_within_single_long() {
        for bits in [2, 3, 5, 8] {
            let mut arr = vec![0i64; 8];
            let vals: Vec<i32> = (0..10).map(|i| (i * 3) % (1 << bits)).collect();
            for (i, v) in vals.iter().enumerate() {
                write_bits(&mut arr, i, bits, *v);
            }
            for (i, v) in vals.iter().enumerate() {
                assert_eq!(read_bits(&arr, i, bits), *v, "bits={bits} i={i}");
            }
        }
    }

    #[test]
    fn bits_roundtrip_crossing_long_boundary() {
        // 用不能被 64 整除的位数，强制出现跨 long 的情况
        const COUNT: usize = 500;
        for bits in [2, 3, 5, 6, 7, 11, 13] {
            // 数组必须按实际位数需求分配：COUNT*bits 个比特。
            // 之前写死 64 个 long，对 11 位这种位数来说只够 372 个值，
            // 超出部分被 write_bits 的越界保护静默丢弃，read_bits 又返回 0，
            // 于是断言报出「写进去 1332、读出来 0」。
            let mut arr = vec![0i64; (COUNT * bits as usize).div_ceil(64)];
            let mask = (1i32 << bits) - 1;
            let vals: Vec<i32> = (0..COUNT as i32).map(|i: i32| (i.wrapping_mul(2654435761u32 as i32)) & mask).collect();
            for (i, v) in vals.iter().enumerate() {
                write_bits(&mut arr, i, bits, *v);
            }
            for (i, v) in vals.iter().enumerate() {
                assert_eq!(read_bits(&arr, i, bits), *v, "bits={bits} i={i}");
            }
        }
    }

    #[test]
    fn encode_decode_indices_roundtrip() {
        let indices: Vec<i32> = (0..1000).map(|i| (i % 37) as i32).collect();
        let bits = bits_for_palette(37);
        let packed = encode_indices(&indices, bits);
        // 打包后的长度应为 ceil(1000*bits/64)
        assert_eq!(packed.len(), (1000 * bits as usize).div_ceil(64));
        let back = decode_indices(&packed, indices.len(), bits);
        assert_eq!(indices, back);
    }

    #[test]
    fn write_bits_does_not_clobber_neighbours() {
        // 相邻值互不干扰
        let bits = 3;
        let mut arr = vec![0i64; 4];
        for i in 0..20 {
            write_bits(&mut arr, i, bits, 5);
        }
        for i in 0..20 {
            assert_eq!(read_bits(&arr, i, bits), 5);
        }
        // 改写其中一个，其余不变
        write_bits(&mut arr, 7, bits, 2);
        for i in 0..20 {
            let want = if i == 7 { 2 } else { 5 };
            assert_eq!(read_bits(&arr, i, bits), want, "i={i}");
        }
    }

    #[test]
    fn write_bits_out_of_range_is_ignored() {
        // 数组容量不足时不应越界写入，也不应 panic。
        // 这正是上面那个测试曾经踩到的路径，显式固化下来。
        let mut arr = vec![0i64; 4];
        for i in 0..1000 {
            write_bits(&mut arr, i, 11, 1332);
        }
        for i in 0..1000 {
            let v = read_bits(&arr, i, 11);
            // 落在数组范围内的位置写成功，范围外的读回 0
            let in_range = (i + 1) * 11 <= arr.len() * 64;
            if in_range {
                assert_eq!(v, 1332, "i={i}");
            } else {
                assert_eq!(v, 0, "i={i}");
            }
        }
    }

    #[test]
    fn read_bits_beyond_end_returns_zero() {
        let arr = vec![0i64; 1];
        assert_eq!(read_bits(&arr, 1000, 4), 0);
    }

    #[test]
    fn region_index_order_is_x_then_z_then_y() {
        let r = Region { size: [4, 3, 2], ..Default::default() };
        // x 变化最快
        assert_eq!(r.index(1, 0, 0), 1);
        assert_eq!(r.index(0, 0, 1), 4);
        assert_eq!(r.index(0, 1, 0), 8);
        // 往返一致
        for y in 0..3 {
            for z in 0..2 {
                for x in 0..4 {
                    assert_eq!(r.pos_from_index(r.index(x, y, z)), [x, y, z]);
                }
            }
        }
    }

    #[test]
    fn negative_size_is_normalized() {
        // Size = {x:-16, y:-10, z:16}, Position = {x:15, y:9, z:-8}
        let region = Value::Compound(vec![
            ("Position".into(), Value::Compound(vec![
                ("x".into(), Value::Int(15)), ("y".into(), Value::Int(9)), ("z".into(), Value::Int(-8)),
            ])),
            ("Size".into(), Value::Compound(vec![
                ("x".into(), Value::Int(-16)), ("y".into(), Value::Int(-10)), ("z".into(), Value::Int(16)),
            ])),
            ("BlockStatePalette".into(), Value::List(
                vec![
                    Value::Compound(vec![("Name".into(), Value::String("minecraft:air".into()))]),
                    Value::Compound(vec![("Name".into(), Value::String("minecraft:stone".into()))]),
                ],
                crate::nbt::TAG_COMPOUND,
            )),
            ("BlockStates".into(), Value::LongArray(vec![0i64; (16 * 10 * 16 * 2usize).div_ceil(64)])),
        ]);
        let r = region_from_nbt("t", &region).unwrap();
        assert_eq!(r.size, [16, 10, 16]);
        // 原点应移动到负方向的起点：15 + (-16) + 1 = 0，9 + (-10) + 1 = 0
        assert_eq!(r.pos, [0, 0, -8]);
        assert_eq!(r.volume, 2560);
    }

    /// Size 为 i32::MIN 时不能 panic，也不能放行一个位置错乱的区域。
    ///
    /// i32::MIN 的相反数无法用 i32 表示：不检查时 debug 构建在取负处 panic，
    /// release 构建回绕后仍是负数，区域会以「尺寸为 0」被放行，而 r.pos 已经
    /// 被写成 -2147483647 之类的值——后续按它计算场景原点，整个预览会被
    /// 框到极远处。这里要求明确报错。
    #[test]
    fn integer_min_size_is_rejected_not_wrapped() {
        let region = region_fixture([0, 0, 0], [i32::MIN, 1, 1], 8);
        let e = region_from_nbt("neg", &region)
            .expect_err("i32::MIN 无法取绝对值，必须报错而不是静默放行");
        let msg = e.to_string();
        assert!(
            msg.contains("绝对值") || msg.contains("Size"),
            "错误信息应指出问题所在，实际：{msg}"
        );
    }

    /// 构造一个用于测试区域解析的最小 NBT 结构。
    /// `size` 与 `pos` 直接给出，调色板只放两项，BlockStates 尽量给足。
    fn region_fixture(pos: [i32; 3], size: [i32; 3], longs: usize) -> Value {
        let ints3 = |a: [i32; 3]| {
            Value::Compound(vec![
                ("x".into(), Value::Int(a[0])),
                ("y".into(), Value::Int(a[1])),
                ("z".into(), Value::Int(a[2])),
            ])
        };
        Value::Compound(vec![
            ("Position".into(), ints3(pos)),
            ("Size".into(), ints3(size)),
            (
                "BlockStatePalette".into(),
                Value::List(
                    vec![
                        Value::Compound(vec![("Name".into(), Value::String("minecraft:air".into()))]),
                        Value::Compound(vec![("Name".into(), Value::String("minecraft:stone".into()))]),
                    ],
                    crate::nbt::TAG_COMPOUND,
                ),
            ),
            ("BlockStates".into(), Value::LongArray(vec![0i64; longs])),
        ])
    }

    /// 尺寸大到体积溢出时，必须报错而不是 panic 或回绕。
    ///
    /// 三个 i32::MAX 相乘约 9.9e27，超出 64 位 usize 上限（约 1.8e19），
    /// 在 debug 构建会 panic、release 会回绕；回绕值随即被用来分配
    /// indices（每方块 4 字节）。该值完全由文件内容决定，因此一个畸形的
    /// .litematic 就能触发，必须挡住。
    #[test]
    fn volume_computation_overflow_is_rejected() {
        let err = region_volume([i32::MAX, i32::MAX, i32::MAX], "huge")
            .expect_err("乘积累积溢出应被拒绝");
        assert!(
            err.to_string().contains("溢出"),
            "错误信息应说明计算溢出，实际：{err}"
        );
    }

    /// 乘积不溢出但超过上限时，同样必须拒绝。
    ///
    /// 与溢出分支区分开：这里的乘积在 usize 范围内。
    #[test]
    fn volume_above_limit_is_rejected() {
        // 100000^3 = 1e15，远超 MAX_REGION_VOLUME 但远小于 usize 上限
        let err = region_volume([100_000, 100_000, 100_000], "big")
            .expect_err("超过上限应被拒绝");
        assert!(
            err.to_string().contains("上限"),
            "错误信息应提到上限，实际：{err}"
        );
    }

    /// 恰好等于上限时应被接受，避免边界判错。
    #[test]
    fn volume_at_exact_limit_is_accepted() {
        // 50_000_000 = 50000 * 1000 * 1，正好是 MAX_REGION_VOLUME
        let v = region_volume([50_000, 1_000, 1], "edge").expect("恰好达上限应被接受");
        assert_eq!(v, MAX_REGION_VOLUME);
    }

    /// 超过上限一个方块即被拒绝，确认上限不是 < 与 <= 的关系判错。
    #[test]
    fn volume_one_over_limit_is_rejected() {
        // 断言错误内容：只断言 is_err 时，任何原因（例如乘法溢出走了另一条
        // 分支）都会让这条通过，就测不出「恰好超限」这个边界
        let err = region_volume([MAX_REGION_VOLUME as i32 + 1, 1, 1], "over")
            .expect_err("超过上限 1 应被拒绝");
        assert!(
            err.to_string().contains("上限"),
            "应指出超出上限，实际：{err}"
        );
    }

    /// 真实量级的尺寸照常通过。
    #[test]
    fn typical_region_volume_is_accepted() {
        // 一个 256×256×256 的区块级投影
        assert_eq!(region_volume([256, 256, 256], "t").unwrap(), 256 * 256 * 256);
        // 256×128×256 ≈ 840 万，是相当大规模的工程
        assert_eq!(region_volume([256, 128, 256], "t").unwrap(), 256 * 128 * 256);
    }

    /// 正常尺寸仍照常解析，确认上限没有误伤端到端路径。
    #[test]
    fn normal_region_volume_still_parses() {
        let longs = (16usize * 10 * 16 * 2).div_ceil(64);
        let r = region_fixture([0, 0, 0], [16, 10, 16], longs);
        let parsed = region_from_nbt("ok", &r).expect("正常尺寸应能解析");
        assert_eq!(parsed.volume, 2560);
        assert_eq!(parsed.size, [16, 10, 16]);
        assert_eq!(parsed.indices.len(), 2560);
    }

    // ---- region_from_nbt 的错误分支 ----
    //
    // 这些分支此前全部没有测试：唯一触及该函数的用例走的是正常路径。
    // 畸形文件是必须处理的输入——.litematic 可能被截断、被别的工具改写，
    // 也可能根本是另一种格式的文件被误当成投影打开。

    /// 从 fixture 中删掉指定字段，构造缺字段的区域
    fn region_without(drop: &str) -> Value {
        let longs = (16usize * 10 * 16 * 2).div_ceil(64);
        match region_fixture([0, 0, 0], [16, 10, 16], longs) {
            Value::Compound(fields) => Value::Compound(
                fields.into_iter().filter(|(k, _)| k != drop).collect(),
            ),
            other => other,
        }
    }

    #[test]
    fn region_without_position_is_rejected() {
        let err = region_from_nbt("r", &region_without("Position"))
            .expect_err("缺少 Position 应报错");
        assert!(
            err.to_string().contains("Position"),
            "错误信息应指出缺少 Position，实际：{err}"
        );
    }

    #[test]
    fn region_without_size_is_rejected() {
        let err =
            region_from_nbt("r", &region_without("Size")).expect_err("缺少 Size 应报错");
        assert!(
            err.to_string().contains("Size"),
            "错误信息应指出缺少 Size，实际：{err}"
        );
    }

    #[test]
    fn region_without_palette_is_rejected() {
        let err = region_from_nbt("r", &region_without("BlockStatePalette"))
            .expect_err("缺少调色板应报错");
        assert!(
            err.to_string().contains("调色板"),
            "错误信息应指出缺少调色板，实际：{err}"
        );
    }

    /// 调色板字段存在但类型不对（这里是字符串而非列表）也要报错。
    #[test]
    fn region_with_non_list_palette_is_rejected() {
        let longs = (16usize * 10 * 16 * 2).div_ceil(64);
        let v = match region_fixture([0, 0, 0], [16, 10, 16], longs) {
            Value::Compound(mut fields) => {
                for (k, val) in fields.iter_mut() {
                    if k == "BlockStatePalette" {
                        *val = Value::String("not a list".into());
                    }
                }
                Value::Compound(fields)
            }
            other => other,
        };
        let err = region_from_nbt("r", &v).expect_err("调色板类型不对应报错");
        assert!(
            err.to_string().contains("不是列表"),
            "错误信息应指出类型不对，实际：{err}"
        );
    }

    /// BlockStates 长度不足时必须报错，而不是当成「全是空气」静默通过。
    ///
    /// 这是本项目特意加的守卫：read_bits 在数组偏短时一律返回 0，
    /// 而索引 0 通常是空气，于是统计结果会静默偏小——用户看到一个
    /// 「看起来正常但数字是错的」投影，比直接报错更难发现。
    #[test]
    fn region_with_short_blockstates_is_rejected() {
        let need = (16usize * 10 * 16 * 2).div_ceil(64);
        // 少给一个 long
        let v = region_fixture([0, 0, 0], [16, 10, 16], need - 1);
        let err = region_from_nbt("r", &v).expect_err("BlockStates 偏短应报错");
        assert!(
            err.to_string().contains("长度不足"),
            "错误信息应指出长度不足，实际：{err}"
        );
    }

    /// 缺少 BlockStates 字段时应视为空区域（全为索引 0），而不是报错。
    ///
    /// 这与上一条的区别：字段缺失是合法的（空区域），长度不足才是损坏。
    #[test]
    fn region_without_blockstates_is_all_air() {
        let v = region_without("BlockStates");
        let r = region_from_nbt("r", &v).expect("缺少 BlockStates 应可解析为空区域");
        assert_eq!(r.volume, 2560);
        assert!(
            r.indices.iter().all(|&i| i == 0),
            "缺少 BlockStates 时全部索引应为 0"
        );
    }

    #[test]
    fn block_state_key_and_props() {
        let mut b = BlockState::new("minecraft:oak_stairs");
        b.set_prop("half", "bottom");
        b.set_prop("facing", "east");
        // 属性按字典序
        assert_eq!(b.prop_str(), "facing=east,half=bottom");
        assert_eq!(b.key(), "minecraft:oak_stairs[facing=east,half=bottom]");
        assert_eq!(b.display_name(), "oak_stairs");
        assert!(!b.is_air());
        assert!(BlockState::new("minecraft:cave_air").is_air());
    }

    // ---- 真实样本的往返与一致性验证 ----
    //
    // 这些测试读取 tests/data 下的真实 .litematic。样本是仓库的一部分，
    // 因此缺失时直接失败，不做静默跳过。

    fn sample_path() -> String {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/data/28速单区块简单熔炉.litematic"
        )
        .to_string()
    }

    #[test]
    fn real_sample_nbt_roundtrips_without_field_loss() {
        let bytes = std::fs::read(sample_path())
            .unwrap_or_else(|e| panic!("缺少测试样本 {}: {e}", sample_path()));

        let root = crate::nbt::parse_maybe_gzip(&bytes).expect("解析失败");
        let out = crate::nbt::serialize_gzip(&root, "", 6).expect("序列化失败");
        let back = crate::nbt::parse_maybe_gzip(&out).expect("再解析失败");

        // 写入路径必须零信息损失：任何字段的丢失或顺序改变都会在这里暴露
        assert_eq!(root, back, "NBT 树在往返后必须完全一致");
    }

    /// 实体与方块实体数量必须来自区域内的列表，而不是 Metadata。
    ///
    /// 真实投影的 Metadata 里**没有** EntityCount / TileEntityCount /
    /// PendingBlockTicks 这几项（样本的 Metadata 只有 9 个键，全是名称、
    /// 作者、时间、体积一类），数量只能按区域列表求和。
    /// 此前读 Metadata 的同名键，界面上这三项恒为 0。
    #[test]
    fn real_sample_counts_entities_from_region_lists() {
        let sc = Schematic::load(&sample_path()).expect("载入失败");
        let s = sc.summary();

        // 样本实测：5 个实体、131 个方块实体、0 个待处理刻
        assert_eq!(s.entity_count, 5, "应按区域列表统计实体数，实际 {}", s.entity_count);
        assert_eq!(
            s.tile_entity_count, 131,
            "应按区域列表统计方块实体数，实际 {}",
            s.tile_entity_count
        );
        assert_eq!(s.pending_ticks, 0, "样本没有待处理刻");

        // 汇总值必须等于各区域之和，而不是某个区域的值
        let by_region: i64 = sc.regions.iter().map(|r| r.entity_count as i64).sum();
        assert_eq!(
            s.entity_count, by_region,
            "汇总值应等于各区域实体数之和"
        );
    }

    /// 实体的标识字段是小写 `id`，不是 `Id`。
    ///
    /// NBT 的键区分大小写。写成 "Id" 会一个都匹配不到，界面上的「实体」
    /// 列表就只剩一行 unknown，用户看不到实际有哪些实体。
    #[test]
    fn real_sample_entity_ids_are_read_not_all_unknown() {
        let sc = Schematic::load(&sample_path()).expect("载入失败");
        let a = sc.analyze();

        assert!(
            !a.entities_by_id.contains_key("unknown"),
            "实体标识必须能读到；全部落入 unknown 说明读错了键名，实际 {:?}",
            a.entities_by_id
        );
        assert_eq!(
            a.entities_by_id.get("minecraft:item").copied(),
            Some(4),
            "样本含 4 个 minecraft:item，实际 {:?}",
            a.entities_by_id
        );
        assert_eq!(
            a.entities_by_id.get("minecraft:hopper_minecart").copied(),
            Some(1),
            "样本含 1 个 minecraft:hopper_minecart，实际 {:?}",
            a.entities_by_id
        );
    }

    #[test]
    fn real_sample_loads_and_regions_are_wellformed() {        let sc = Schematic::load(&sample_path()).expect("载入失败");
        assert!(!sc.regions.is_empty(), "应解析出至少一个区域");

        for r in &sc.regions {
            assert!(
                r.size.iter().all(|&v| v > 0),
                "归一化后尺寸必须全为正，实际 {:?}",
                r.size
            );
            assert_eq!(
                r.indices.len(),
                r.volume,
                "索引数量必须等于区域体积"
            );
            assert!(!r.palette.is_empty(), "调色板不应为空");
            assert!(
                r.indices.iter().all(|i| (*i as usize) < r.palette.len()),
                "索引必须落在调色板范围内"
            );
        }
    }

    #[test]
    fn decoded_block_count_matches_declared_total() {
        // 用文件自身声明的 TotalBlocks 校验位打包是否正确。
        // 这比断言一个魔数更有意义：它检验的是解码结果与文件元数据的自洽性。
        let sc = Schematic::load(&sample_path()).expect("载入失败");
        let declared = sc
            .root
            .get("Metadata")
            .and_then(|m| m.get("TotalBlocks"))
            .and_then(|v| v.as_i64())
            .expect("样本应含 Metadata.TotalBlocks");
        assert!(declared > 0, "TotalBlocks 应大于 0");

        let decoded = sc.analyze().non_air_total as i64;
        assert_eq!(
            decoded, declared,
            "解码出的非空气方块数应与元数据 TotalBlocks 一致"
        );
    }

    #[test]
    fn metadata_edit_roundtrips_through_disk() {
        // 完整走一遍写入路径：载入 -> 改描述 -> 落盘 -> 重新载入。
        // 同时确认方块数据与其它元数据未被牵连改动。
        let dir = std::env::temp_dir().join(format!(
            "sm_meta_roundtrip_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let dst = dir.join("copy.litematic");
        std::fs::copy(sample_path(), &dst).unwrap();
        let dst_s = dst.to_string_lossy().into_owned();

        let before = Schematic::load(&dst_s).unwrap();
        let blocks_before = before.analyze().non_air_total;

        let mut sc = Schematic::load(&dst_s).unwrap();
        assert!(sc.set_meta_description("往返测试写入的描述"));
        sc.save_to(&dst_s).expect("保存失败");

        let after = Schematic::load(&dst_s).unwrap();
        let md = after.root.get("Metadata").expect("元数据应仍然存在");
        assert_eq!(
            md.get("Description").and_then(|v| v.as_str()),
            Some("往返测试写入的描述"),
            "描述应被正确写入"
        );
        // 方块数据不能因为改元数据而受影响
        assert_eq!(
            after.analyze().non_air_total,
            blocks_before,
            "改元数据不应影响方块数据"
        );
        // 其余元数据字段应原样保留
        assert_eq!(
            before.root.get("Metadata").and_then(|m| m.get("Name")).and_then(|v| v.as_str()),
            after.root.get("Metadata").and_then(|m| m.get("Name")).and_then(|v| v.as_str()),
            "未修改的元数据字段应保持不变"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

}
