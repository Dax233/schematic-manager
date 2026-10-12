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

    /// 是否为空气（minecraft:air / cave_air / void_air）
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

    pub fn block_at(&self, x: i32, y: i32, z: i32) -> Option<&BlockState> {
        if !self.inside(x, y, z) {
            return None;
        }
        let i = self.indices.get(self.index(x, y, z))?;
        self.palette.get(*i as usize)
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
    pub valid: bool,
    pub error: String,
    pub path: String,
    pub format_version: i32,
    pub format_sub_version: i32,
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
    pub preview_pixels: i64,
    pub file_size: u64,
    pub fully_analyzed: bool,
    pub distinct_block_types: usize,
    pub distinct_states: usize,
}

/// 完整分析结果
#[derive(Debug, Clone, Default)]
pub struct Analysis {
    /// 按方块名统计（不含属性）
    pub blocks_by_name: BTreeMap<String, u64>,
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
    let mut s = Summary { valid: true, ..Default::default() };
    s.format_version = root.get("Version").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    s.format_sub_version = root.get("SubVersion").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
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
        s.entity_count = md.get("EntityCount").and_then(|v| v.as_i64()).unwrap_or(0);
        s.tile_entity_count = md.get("TileEntityCount").and_then(|v| v.as_i64()).unwrap_or(0);
        s.pending_ticks = md.get("PendingBlockTicks").and_then(|v| v.as_i64()).unwrap_or(0)
            + md.get("PendingFluidTicks").and_then(|v| v.as_i64()).unwrap_or(0);
        if let Some(es) = md.get("EnclosingSize") {
            if let Some(xyz) = es.as_xyz() {
                s.enclosing = xyz;
            }
        }
        if let Some(pv) = md.get("PreviewImageData") {
            s.has_preview = true;
            s.preview_pixels = match pv {
                Value::IntArray(a) => a.len() as i64,
                _ => 0,
            };
        }
    }

    // Regions 的键数量可能与 Metadata.RegionCount 不一致，以实际为准
    if let Some(Value::Compound(regs)) = root.get("Regions") {
        if s.region_count == 0 {
            s.region_count = regs.len() as i32;
        }
    }
    s
}

/// 解析单个区域
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
            let new_pos = pos[a].wrapping_add(size[a]).wrapping_add(1);
            r.pos[a] = new_pos;
            size[a] = -size[a];
        } else {
            r.pos[a] = pos[a];
        }
    }
    r.size = size;

    // 尺寸为 0 的区域没有方块数据
    if size.iter().any(|&s| s <= 0) {
        return Ok(r);
    }
    r.volume = size[0] as usize * size[1] as usize * size[2] as usize;

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

    // 解码方块索引
    if let Some(longs) = v.get("BlockStates").and_then(|x| x.as_longs()) {
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
    pub loaded: bool,
    pub modified: bool,
}

impl Schematic {
    /// 从文件载入
    pub fn load(path: &str) -> AppResult<Self> {
        let bytes = std::fs::read(path)?;
        let root = nbt::parse_maybe_gzip(&bytes)?;
        let mut sc = Self { root, regions: Vec::new(), path: path.to_string(), loaded: true, modified: false };
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
                        inner.valid = true;
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
        s.valid = true;
        s.fully_analyzed = true;
        let mut types = std::collections::BTreeSet::new();
        let mut states = std::collections::BTreeSet::new();
        for r in &self.regions {
            for b in &r.palette {
                types.insert(b.name.clone());
                states.insert(b.key());
            }
        }
        s.distinct_block_types = types.len();
        s.distinct_states = states.len();
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
                *a.blocks_by_name.entry(b.name.clone()).or_insert(0) += 1;
                *a.blocks_by_state.entry(b.key()).or_insert(0) += 1;
            }
        }
        for r in &self.regions {
            if let Some(Value::Compound(regs)) = self.root.get("Regions") {
                if let Some(rv) = regs.iter().find(|(n, _)| n == &r.name).map(|(_, v)| v) {
                    for key in ["Entities", "TileEntities"] {
                        if let Some(list) = rv.get(key).and_then(|x| x.as_list()) {
                            for e in list {
                                let id = e.get("Id").and_then(|x| x.as_str()).unwrap_or("unknown");
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
        if md.get(key).is_none() {
            return false;
        }
        let mut md = md.clone();
        md.set(key, Value::String(v.to_string()));
        self.root.set("Metadata", md);
        self.modified = true;
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
    pub fn set_data_version(&mut self, v: i32) -> bool {
        if self.root.get("MinecraftDataVersion").is_none() {
            return false;
        }
        self.root.set("MinecraftDataVersion", Value::Int(v));
        self.modified = true;
        true
    }

    /// 把编辑后的索引重新编码回 NBT 并保存
    pub fn save_to(&mut self, path: &str) -> AppResult<()> {
        self.sync_to_nbt()?;
        let bytes = nbt::serialize_gzip(&self.root, "", 6)?;
        std::fs::write(path, bytes)?;
        self.modified = false;
        Ok(())
    }

    pub fn to_bytes(&mut self) -> AppResult<Vec<u8>> {
        self.sync_to_nbt()?;
        nbt::serialize_gzip(&self.root, "", 6)
    }

    /// 把内存中的区域索引写回 NBT 的 BlockStates 字段
    fn sync_to_nbt(&mut self) -> AppResult<()> {
        if !self.modified {
            return Ok(());
        }
        let region_names: Vec<String> = self.regions.iter().map(|r| r.name.clone()).collect();
        for name in region_names {
            let Some(r) = self.regions.iter().find(|r| r.name == name).cloned() else { continue };
            if r.volume == 0 {
                continue;
            }
            let longs = encode_indices(&r.indices, r.bits);
            let Some(Value::Compound(regs)) = self.root.get("Regions").cloned() else { continue };
            let mut regs = regs;
            if let Some(slot) = regs.iter_mut().find(|(n, _)| n == &name) {
                slot.1.set("BlockStates", Value::LongArray(longs));
            }
            self.root.set("Regions", Value::Compound(regs));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            ("BlockStatePalette".into(), Value::List(vec![
                Value::Compound(vec![("Name".into(), Value::String("minecraft:air".into()))]),
                Value::Compound(vec![("Name".into(), Value::String("minecraft:stone".into()))]),
            ])),
            ("BlockStates".into(), Value::LongArray(vec![0i64; (16 * 10 * 16 * 2usize).div_ceil(64)])),
        ]);
        let r = region_from_nbt("t", &region).unwrap();
        assert_eq!(r.size, [16, 10, 16]);
        // 原点应移动到负方向的起点：15 + (-16) + 1 = 0，9 + (-10) + 1 = 0
        assert_eq!(r.pos, [0, 0, -8]);
        assert_eq!(r.volume, 2560);
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

    #[test]
    fn real_sample_loads_and_regions_are_wellformed() {
        let sc = Schematic::load(&sample_path()).expect("载入失败");
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
