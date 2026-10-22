//! 内部资源包（blocks.dat）的序列化。
//!
//! 是 block/db.rs 读取器的逆运算，格式来源为 legacy-cpp/packbuild.cpp 的
//! SerializePack。导入 Minecraft jar 时用它把解析结果写成 pack 文件。
//!
//! 字符串池必须先于所有段建立：各段存的是字符串下标，边写边收集会让
//! 先写出的段拿到错误下标。
//!
//! 本模块只做序列化，不涉及 zip / PNG / 文件 IO，因此可用「写出再读回」
//! 的往返测试完整验证——这也是保证与读取端一致的最直接手段。
//!
//! 一个容易搞错的点：文件头里的 langCount 是**语言表个数**（不是条目数），
//! 段内先写一个与它相等的重复计数，再逐表写条目数。

use crate::block::db::{BlockDb, LangEntry, Model, ModelElement};
use crate::error::{AppError, AppResult};

/// 小端写入辅助。
///
/// 读取端（block/db.rs 的 BinReader）是手写字节序的，这里保持一致：
/// 既统一了风格，也让本地类型检查不必为 byteorder 额外造桩。
trait WriteLe {
    fn w_u8(&mut self, v: u8) -> AppResult<()>;
    fn w_u16(&mut self, v: u16) -> AppResult<()>;
    fn w_u32(&mut self, v: u32) -> AppResult<()>;
    fn w_i16(&mut self, v: i16) -> AppResult<()>;
    fn w_i32(&mut self, v: i32) -> AppResult<()>;
    fn w_f32(&mut self, v: f32) -> AppResult<()>;
}

impl WriteLe for Vec<u8> {
    fn w_u8(&mut self, v: u8) -> AppResult<()> {
        self.push(v);
        Ok(())
    }
    fn w_u16(&mut self, v: u16) -> AppResult<()> {
        self.extend_from_slice(&v.to_le_bytes());
        Ok(())
    }
    fn w_u32(&mut self, v: u32) -> AppResult<()> {
        self.extend_from_slice(&v.to_le_bytes());
        Ok(())
    }
    fn w_i16(&mut self, v: i16) -> AppResult<()> {
        self.extend_from_slice(&v.to_le_bytes());
        Ok(())
    }
    fn w_i32(&mut self, v: i32) -> AppResult<()> {
        self.extend_from_slice(&v.to_le_bytes());
        Ok(())
    }
    fn w_f32(&mut self, v: f32) -> AppResult<()> {
        self.extend_from_slice(&v.to_le_bytes());
        Ok(())
    }
}
use std::collections::HashMap;

const MAGIC: &[u8; 8] = b"PMBLKDB\0";
const FORMAT_VERSION: u32 = 3;

/// 字符串池：保持插入顺序，同时记录下标
#[derive(Debug, Default)]
struct StringPool {
    items: Vec<String>,
    index: HashMap<String, u32>,
}

impl StringPool {
    fn intern(&mut self, s: &str) -> u32 {
        if let Some(i) = self.index.get(s) {
            return *i;
        }
        let i = self.items.len() as u32;
        self.items.push(s.to_string());
        self.index.insert(s.to_string(), i);
        i
    }
}

/// 把方块库序列化成 pack 字节。
///
/// 要求恰好一个版本与至多一个语言表：pack 文件的设计就是单版本，
/// 多版本会让读取端只取到第一个、静默丢弃其余，因此这里直接报错。
pub fn serialize(db: &BlockDb) -> AppResult<Vec<u8>> {
    let version = match db.versions.len() {
        1 => &db.versions[0],
        0 => return Err(AppError::Parse("方块库没有任何版本，无法序列化".into())),
        n => {
            return Err(AppError::Parse(format!(
                "pack 文件只容纳一个版本，当前有 {n} 个"
            )))
        }
    };
    if db.langs.len() > 1 {
        return Err(AppError::Parse(format!(
            "pack 文件只容纳一个语言表，当前有 {} 个",
            db.langs.len()
        )));
    }

    // ---- 先建字符串池 ----
    // 池以 db.strings 为起点：模型面里的 texture 字段存的就是该表的下标，
    // 保留原有下标才能保证写出后仍然可解析。
    let mut pool = StringPool::default();
    for s in &db.strings {
        pool.intern(s);
    }

    let name_idx = pool.intern(&version.name);
    let block_name_idx: Vec<u32> = version.blocks.iter().map(|b| pool.intern(&b.name)).collect();
    let variant_when_idx: Vec<Vec<u32>> = version
        .blocks
        .iter()
        .map(|b| b.variants.iter().map(|v| pool.intern(&v.when)).collect())
        .collect();
    let multipart_when_idx: Vec<Vec<u32>> = version
        .blocks
        .iter()
        .map(|b| b.multipart.iter().map(|v| pool.intern(&v.when)).collect())
        .collect();
    // 贴图路径映射的下标需一并登记，否则读取端取不到路径字符串
    let mut tex_entries: Vec<(u32, u32)> = Vec::with_capacity(version.texture_by_path.len());
    for (path, idx) in &version.texture_by_path {
        tex_entries.push((pool.intern(path), *idx));
    }
    // 顺序固定，保证同一输入产出同一文件
    tex_entries.sort_by_key(|(p, _)| *p);

    let empty: Vec<LangEntry> = Vec::new();
    let langs: &Vec<LangEntry> = db.langs.first().unwrap_or(&empty);
    let lang_idx: Vec<[u32; 3]> = langs
        .iter()
        .map(|e| {
            [
                pool.intern(&e.block_name),
                pool.intern(&e.en),
                pool.intern(&e.zh),
            ]
        })
        .collect();

    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(MAGIC);
    out.w_u32(FORMAT_VERSION)?;
    out.w_u32(1)?; // 版本数
    out.w_u32(db.textures.len() as u32)?;
    out.w_u32(db.models.len() as u32)?;
    // 语言表**个数**：无语言表时也要写 1（读取端按此计数逐表读取）
    out.w_u32(1)?;

    // ---- 字符串池 ----
    out.w_u32(pool.items.len() as u32)?;
    for s in &pool.items {
        out.extend_from_slice(s.as_bytes());
        out.w_u8(0)?;
    }

    // ---- 语言表 ----
    out.w_u32(1)?; // 段内重复计数，须与文件头的个数一致
    out.w_u32(langs.len() as u32)?; // 该表的条目数
    for ids in &lang_idx {
        out.w_u32(ids[0])?;
        out.w_u32(ids[1])?;
        out.w_u32(ids[2])?;
    }

    // ---- 贴图表 ----
    out.w_u32(db.textures.len() as u32)?;
    for t in &db.textures {
        out.w_u16(t.w)?;
        out.w_u16(t.h)?;
        out.w_u32(t.offset)?;
        out.w_u32(t.len)?;
        out.w_u8(u8::from(t.has_alpha))?;
    }
    // 像素池：zlib 压缩，先写原始长度再写压缩长度与内容
    let comp = compress_zlib(&db.pixels)?;
    out.w_u32(db.pixels.len() as u32)?;
    out.w_u32(comp.len() as u32)?;
    out.extend_from_slice(&comp);

    // ---- 模型表 ----
    out.w_u32(db.models.len() as u32)?;
    for m in &db.models {
        write_model(&mut out, m)?;
    }

    // ---- 版本段 ----
    out.w_u32(1)?;
    out.w_u32(name_idx)?;
    out.w_i32(version.data_version)?;
    out.w_u32(0)?; // 语言表下标
    out.w_u32(tex_entries.len() as u32)?;
    for (path_idx, pool_idx) in &tex_entries {
        out.w_u32(*path_idx)?;
        out.w_u32(*pool_idx)?;
    }
    out.w_u32(version.blocks.len() as u32)?;
    for (i, b) in version.blocks.iter().enumerate() {
        out.w_u32(block_name_idx[i])?;
        // 变体与 multipart 的条数写成 u16，必须先校验上限。
        // `as u16` 会静默截断：65536 条会写成 0，而写入端仍照实写 65536 条，
        // 读取端按 0 条读取后，会把变体的字节当成下一个方块的字段继续消费，
        // 整个版本段从此错位——不报错，只是数据全错。
        // 同文件的 write_model 对元素数已有相同的守卫，这里保持一致。
        if b.variants.len() > u16::MAX as usize {
            return Err(AppError::Corrupt(format!(
                "方块 {} 的变体数为 {}，超过上限 {}",
                b.name,
                b.variants.len(),
                u16::MAX
            )));
        }
        out.w_u16(b.variants.len() as u16)?;
        for (k, v) in b.variants.iter().enumerate() {
            write_variant(&mut out, v, variant_when_idx[i][k])?;
        }
        if b.multipart.len() > u16::MAX as usize {
            return Err(AppError::Corrupt(format!(
                "方块 {} 的 multipart 数为 {}，超过上限 {}",
                b.name,
                b.multipart.len(),
                u16::MAX
            )));
        }
        out.w_u16(b.multipart.len() as u16)?;
        for (k, v) in b.multipart.iter().enumerate() {
            write_variant(&mut out, v, multipart_when_idx[i][k])?;
        }
    }

    Ok(out)
}

fn write_variant(
    out: &mut Vec<u8>,
    v: &crate::block::db::Variant,
    when_idx: u32,
) -> AppResult<()> {
    out.w_u32(when_idx)?;
    out.w_u32(v.model_idx)?;
    out.w_u8(v.rot_x_div90)?;
    out.w_u8(v.rot_y_div90)?;
    out.w_u8(u8::from(v.uvlock))?;
    Ok(())
}

fn write_model(out: &mut Vec<u8>, m: &Model) -> AppResult<()> {
    out.w_u8(u8::from(m.translucent))?;
    if m.elements.len() > u16::MAX as usize {
        return Err(AppError::Parse(format!(
            "模型元素数 {} 超出格式上限",
            m.elements.len()
        )));
    }
    out.w_u16(m.elements.len() as u16)?;
    for e in &m.elements {
        write_element(out, e)?;
    }
    Ok(())
}

fn write_element(out: &mut Vec<u8>, e: &ModelElement) -> AppResult<()> {
    for k in 0..3 {
        out.w_f32(e.from[k])?;
    }
    for k in 0..3 {
        out.w_f32(e.to[k])?;
    }
    out.w_u8(u8::from(e.has_rotation))?;
    if e.has_rotation {
        out.w_u8(e.rot_kind)?;
        for k in 0..3 {
            out.w_f32(e.origin[k])?;
        }
        out.w_u8(e.axis)?;
        out.w_f32(e.angle)?;
        out.w_u8(u8::from(e.rescale))?;
        for k in 0..3 {
            out.w_f32(e.rot_vec[k])?;
        }
    }
    out.w_u8(e.face_mask)?;

    // 面的顺序必须与 faceMask 的位序一致：读取端按位依次取面
    let mut fi = 0usize;
    for bit in 0..6u8 {
        if e.face_mask & (1u8 << bit) == 0 {
            continue;
        }
        let f = e.faces.get(fi).ok_or_else(|| {
            AppError::Parse(format!(
                "元素的面数与 faceMask 不符：位 {bit} 已置位但只有 {} 个面",
                e.faces.len()
            ))
        })?;
        fi += 1;
        // 贴图字段是字符串表下标，原样写出即可
        out.w_u32(f.texture)?;
        for k in 0..4 {
            out.w_f32(f.uv[k])?;
        }
        out.w_u8(f.cullface)?;
        out.w_i16(f.tintindex)?;
        out.w_u16(f.rotation)?;
    }
    if fi != e.faces.len() {
        return Err(AppError::Parse(format!(
            "元素的面数与 faceMask 不符：{} 个面但只写出 {fi} 个",
            e.faces.len()
        )));
    }
    Ok(())
}

/// zlib 压缩（PNG/资源包使用的容器格式）
fn compress_zlib(data: &[u8]) -> AppResult<Vec<u8>> {
    use std::io::Write;
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(9));
    e.write_all(data)?;
    Ok(e.finish()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::db::{BlockDef, ModelFace, TextureInfo, Variant, VersionDef};

    fn face(texture: u32) -> ModelFace {
        ModelFace {
            texture,
            uv: [0.0, 0.0, 16.0, 16.0],
            cullface: 0xFF,
            tintindex: -1,
            rotation: 0,
        }
    }

    fn element(faces: Vec<ModelFace>, mask: u8) -> ModelElement {
        ModelElement {
            from: [0.0, 0.0, 0.0],
            to: [16.0, 16.0, 16.0],
            has_rotation: false,
            rot_kind: 0,
            origin: [0.0; 3],
            axis: 0,
            angle: 0.0,
            rescale: false,
            rot_vec: [0.0; 3],
            face_mask: mask,
            faces,
        }
    }

    /// 造一个内容较全的方块库：字符串表、方块、变体、multipart、模型、贴图
    fn sample_db() -> BlockDb {
        let strings = vec![
            "1.20.1".to_string(),
            "minecraft:stone".to_string(),
            "block/stone".to_string(),
            "facing=north".to_string(),
            "Stone".to_string(),
            "石头".to_string(),
        ];
        let model = Model {
            translucent: false,
            elements: vec![
                element(vec![face(2), face(2)], 0b000011),
                element(vec![face(2)], 0b100000),
            ],
        };
        let version = VersionDef {
            name: "1.20.1".into(),
            data_version: 3465,
            lang_idx: 0,
            texture_by_path: HashMap::from([("block/stone".to_string(), 0u32)]),
            blocks: vec![BlockDef {
                name: "minecraft:stone".into(),
                variants: vec![Variant {
                    when: String::new(),
                    model_idx: 0,
                    rot_x_div90: 1,
                    rot_y_div90: 2,
                    uvlock: true,
                }],
                multipart: vec![Variant {
                    when: "facing=north".into(),
                    model_idx: 0,
                    rot_x_div90: 0,
                    rot_y_div90: 0,
                    uvlock: false,
                }],
            }],
            langs: Vec::new(),
            is_overlay: false,
        };
        BlockDb {
            strings,
            langs: vec![vec![LangEntry {
                block_name: "minecraft:stone".into(),
                en: "Stone".into(),
                zh: "石头".into(),
            }]],
            // 贴图与像素默认留空，由需要它们的用例自行填充。
            // 本地 flate2 桩早已改为产出合法的 stored 块，像素池的往返在本地
            // 也能验证（见 texture_section_round_trips_fields_and_pixels）。
            textures: Vec::new(),
            models: vec![model],
            versions: vec![version],
            pixels: Vec::new(),
        }
    }

    #[test]
    fn round_trip_preserves_versions_and_blocks() {
        let db = sample_db();
        let bytes = serialize(&db).expect("序列化失败");
        let back = BlockDb::load_from_bytes(&bytes, false).expect("读回失败");

        assert_eq!(back.versions.len(), 1);
        let v = &back.versions[0];
        assert_eq!(v.name, "1.20.1");
        assert_eq!(v.data_version, 3465);
        assert_eq!(v.blocks.len(), 1);

        let b = &v.blocks[0];
        assert_eq!(b.name, "minecraft:stone");
        assert_eq!(b.variants.len(), 1);
        assert_eq!(b.variants[0].when, "");
        assert_eq!(b.variants[0].model_idx, 0);
        assert_eq!(b.variants[0].rot_x_div90, 1);
        assert_eq!(b.variants[0].rot_y_div90, 2);
        assert!(b.variants[0].uvlock);
        assert_eq!(b.multipart.len(), 1);
        assert_eq!(b.multipart[0].when, "facing=north");
    }

    #[test]
    fn round_trip_preserves_strings_and_texture_map() {
        let db = sample_db();
        let bytes = serialize(&db).unwrap();
        let back = BlockDb::load_from_bytes(&bytes, false).unwrap();

        // 原字符串表的下标必须保持不变：模型面里存的就是这些下标
        for (i, s) in db.strings.iter().enumerate() {
            assert_eq!(&back.strings[i], s, "字符串 {i} 发生了变化");
        }
        assert_eq!(
            back.versions[0].texture_by_path.get("block/stone"),
            Some(&0),
            "贴图路径到池下标的映射应保留"
        );
    }

    #[test]
    fn round_trip_preserves_language_table() {
        let db = sample_db();
        let back = BlockDb::load_from_bytes(&serialize(&db).unwrap(), false).unwrap();
        assert_eq!(back.langs.len(), 1, "语言表个数应为 1");
        assert_eq!(back.langs[0].len(), 1);
        let e = &back.langs[0][0];
        assert_eq!(e.block_name, "minecraft:stone");
        assert_eq!(e.en, "Stone");
        assert_eq!(e.zh, "石头");
        // 版本上的语言表也要挂对
        assert_eq!(back.versions[0].langs.len(), 1);
        assert_eq!(back.versions[0].langs[0].zh, "石头");
    }

    /// 贴图段的往返：字段与像素内容都要逐字节保留。
    ///
    /// 这条测试此前是**恒真**的：夹具的 textures 与 pixels 都是空的，
    /// 两边长度都是 0，贴图段写不写、读不读都能通过；注释还写着「由 CI 的
    /// cargo test 覆盖」，而 CI 跑的仍是同一个空夹具。现在填入真实的贴图
    /// 记录与像素，逐字段与逐字节比对。
    ///
    /// 已验证有效性：把写入端的像素截短一个字节后这条会失败
    /// （读取端报「贴图 1 的范围超出像素池长度」），而旧写法照样通过。
    #[test]
    fn texture_section_round_trips_fields_and_pixels() {
        let mut db = sample_db();
        db.textures = vec![
            TextureInfo {
                w: 16,
                h: 16,
                offset: 0,
                len: 256,
                has_alpha: true,
            },
            TextureInfo {
                w: 1,
                h: 1,
                offset: 256,
                len: 4,
                has_alpha: false,
            },
        ];
        // 像素按位置取不同值：这样「偏移写错」或「长度截断」都能暴露，
        // 全填同一个值则错位也看不出来
        db.pixels = (0..260).map(|i| (i % 251) as u8).collect();

        let back = BlockDb::load_from_bytes(&serialize(&db).unwrap(), false).unwrap();

        assert_eq!(back.textures.len(), 2, "贴图记录数应保留");
        for (i, t) in db.textures.iter().enumerate() {
            assert_eq!(back.textures[i].w, t.w, "第 {i} 条贴图的宽");
            assert_eq!(back.textures[i].h, t.h, "第 {i} 条贴图的高");
            assert_eq!(back.textures[i].offset, t.offset, "第 {i} 条贴图的偏移");
            assert_eq!(back.textures[i].len, t.len, "第 {i} 条贴图的长度");
            assert_eq!(
                back.textures[i].has_alpha, t.has_alpha,
                "第 {i} 条贴图的 alpha 标记"
            );
        }
        assert_eq!(back.pixels, db.pixels, "像素池应逐字节一致");

        // 贴图切片也要能取到正确的像素：只比对整个池会漏掉偏移算错的情况
        assert_eq!(
            back.texture_pixels(0).map(|p| p.to_vec()),
            Some(db.pixels[0..256].to_vec()),
            "第 0 条贴图的像素切片"
        );
        assert_eq!(
            back.texture_pixels(1).map(|p| p.to_vec()),
            Some(db.pixels[256..260].to_vec()),
            "第 1 条贴图的像素切片"
        );
    }

    #[test]
    fn round_trip_preserves_models_and_faces() {
        let db = sample_db();
        let back = BlockDb::load_from_bytes(&serialize(&db).unwrap(), false).unwrap();
        assert_eq!(back.models.len(), 1);
        let m = &back.models[0];
        assert!(!m.translucent);
        assert_eq!(m.elements.len(), 2, "元素数应保留");

        let e0 = &m.elements[0];
        assert_eq!(e0.from, [0.0, 0.0, 0.0]);
        assert_eq!(e0.to, [16.0, 16.0, 16.0]);
        assert_eq!(e0.face_mask, 0b000011);
        assert_eq!(e0.faces.len(), 2, "面数应与 faceMask 一致");
        assert_eq!(e0.faces[0].texture, 2, "贴图字符串下标应保留");
        assert_eq!(e0.faces[0].uv, [0.0, 0.0, 16.0, 16.0]);

        let e1 = &m.elements[1];
        assert_eq!(e1.face_mask, 0b100000);
        assert_eq!(e1.faces.len(), 1);
    }

    #[test]
    fn serialization_is_deterministic() {
        let db = sample_db();
        let a = serialize(&db).unwrap();
        let b = serialize(&db).unwrap();
        assert_eq!(a, b, "同一输入必须产出完全相同的字节");
    }

    #[test]
    fn rejects_multiple_versions() {
        let mut db = sample_db();
        let extra = db.versions[0].clone();
        db.versions.push(extra);
        let e = serialize(&db).unwrap_err().to_string();
        assert!(e.contains("只容纳一个版本"), "应说明版本数限制：{e}");
    }

    #[test]
    fn rejects_empty_db() {
        let mut db = sample_db();
        db.versions.clear();
        let e = serialize(&db).unwrap_err().to_string();
        assert!(e.contains("没有任何版本"), "应说明缺少版本：{e}");
    }

    #[test]
    fn rejects_multiple_lang_tables() {
        let mut db = sample_db();
        let extra = db.langs[0].clone();
        db.langs.push(extra);
        let e = serialize(&db).unwrap_err().to_string();
        assert!(e.contains("只容纳一个语言表"), "应说明语言表限制：{e}");
    }

    /// 变体数超过 u16 时必须报错，而不是截断成 0。
    ///
    /// 截断的后果不是 panic 而是**级联静默错位**：写入端声明 0 条却照实
    /// 写 65536 条，读取端按 0 条读取后，把变体的字节当成下一个方块的
    /// 字段继续消费，整个版本段从此错位，且全程不报错。
    #[test]
    fn rejects_variant_count_over_u16() {
        let mut db = sample_db();
        let v = db.versions[0].blocks[0].variants[0].clone();
        db.versions[0].blocks[0].variants = vec![v; u16::MAX as usize + 1];
        let e = serialize(&db).unwrap_err().to_string();
        assert!(
            e.contains("变体数") && e.contains("超过上限"),
            "应说明变体数超限：{e}"
        );
    }

    /// multipart 数同理。
    #[test]
    fn rejects_multipart_count_over_u16() {
        let mut db = sample_db();
        let v = db.versions[0].blocks[0].multipart[0].clone();
        db.versions[0].blocks[0].multipart = vec![v; u16::MAX as usize + 1];
        let e = serialize(&db).unwrap_err().to_string();
        assert!(
            e.contains("multipart") && e.contains("超过上限"),
            "应说明 multipart 数超限：{e}"
        );
    }

    /// 恰好等于上限时不应被误判为超限。
    #[test]
    fn accepts_variant_count_at_u16_limit() {
        let mut db = sample_db();
        let v = db.versions[0].blocks[0].variants[0].clone();
        db.versions[0].blocks[0].variants = vec![v; u16::MAX as usize];
        let r = serialize(&db);
        assert!(
            r.is_ok(),
            "恰好 65535 条变体应可写出，实际：{:?}",
            r.err().map(|e| e.to_string())
        );
    }

    #[test]
    fn face_count_mismatch_is_reported() {
        // faceMask 置了两位但只给一个面：必须报错而不是写出错位数据
        let mut db = sample_db();
        db.models[0].elements[0].faces.truncate(1);
        let e = serialize(&db).unwrap_err().to_string();
        assert!(e.contains("面数"), "应指出面数与 faceMask 不符：{e}");
    }

    #[test]
    fn extra_faces_are_reported() {
        let mut db = sample_db();
        // 多给一个面：多余的面不会被写出，必须报错以免静默丢弃
        db.models[0].elements[0].faces.push(face(2));
        let e = serialize(&db).unwrap_err().to_string();
        assert!(e.contains("面数"), "应指出面数不符：{e}");
    }

    #[test]
    fn empty_lang_table_is_written_as_one_table_with_zero_entries() {
        let mut db = sample_db();
        db.langs.clear();
        let back = BlockDb::load_from_bytes(&serialize(&db).unwrap(), false).unwrap();
        assert_eq!(back.langs.len(), 1, "无语言表也要写一个空表");
        assert!(back.langs[0].is_empty());
    }

    #[test]
    fn header_declares_one_lang_table_count() {
        // 文件头的 langCount 是语言表个数，写错会让读取端整体错位
        let db = sample_db();
        let bytes = serialize(&db).unwrap();
        // 魔数 8 + format 4 + versionCount 4 + textureCount 4 + modelCount 4 = 24
        let lang_count = u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]);
        assert_eq!(lang_count, 1);
    }

    #[test]
    fn empty_db_with_no_models_or_textures_still_round_trips() {
        let mut db = sample_db();
        db.models.clear();
        db.textures.clear();
        let back = BlockDb::load_from_bytes(&serialize(&db).unwrap(), false).unwrap();
        assert!(back.models.is_empty());
        assert!(back.textures.is_empty());
        assert_eq!(back.versions.len(), 1);
    }
}
