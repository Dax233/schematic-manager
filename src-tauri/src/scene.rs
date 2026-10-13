//! 组装前端渲染所需的完整场景数据。
//!
//! 前端用 Three.js 绘制，因此这里把「解析结果」整理成可直接喂给
//! BufferGeometry 与 DataTexture 的形状：按贴图分批的顶点数据，
//! 外加每个批次要用到的贴图像素。

use crate::blockdb::{BlockDb, TextureInfo};
use crate::error::{AppError, AppResult};
use crate::litematic::Schematic;
use crate::mesh;
use crate::visibility;

/// 一个批次 = 一张贴图 = 前端一个材质
#[derive(Debug, Clone, PartialEq)]
pub struct SceneBatch {
    /// 贴图池下标
    pub texture: u32,
    /// 顶点位置，每 3 个 f32 一个顶点
    pub positions: Vec<f32>,
    /// 顶点 UV，取值 0..16，前端除以 16 归一化
    pub uvs: Vec<f32>,
    /// 每顶点颜色系数（RGB 三通道，已含明暗与染色）
    pub colors: Vec<f32>,
    /// 三角形索引
    pub indices: Vec<u32>,
    pub texture_width: u16,
    pub texture_height: u16,
    /// 贴图像素，RGBA8888，长度 = 宽 * 高 * 4
    pub texture_rgba: Vec<u8>,
}

/// 完整场景
#[derive(Debug, Clone, PartialEq)]
pub struct SceneData {
    pub batches: Vec<SceneBatch>,
    pub total_vertices: usize,
    pub total_triangles: usize,
    /// 投影外框尺寸，供前端设置初始相机
    pub enclosing: [i32; 3],
    /// 投影左下角在世界坐标中的位置，供前端居中
    pub origin: [i32; 3],
    pub mc_version_name: String,
    pub data_version: i32,
    /// 被跳过的方块种类数（该版本方块库里查不到定义的方块）
    pub unresolved_blocks: usize,
}

impl SceneData {
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.batches.is_empty()
    }
}

/// 取一张贴图的 RGBA 像素。越界或长度不足时返回 None。
fn texture_rgba(db: &BlockDb, idx: u32) -> Option<Vec<u8>> {
    let t: &TextureInfo = db.textures.get(idx as usize)?;
    let start = t.offset as usize;
    let end = start + t.len as usize;
    let px = db.pixels.get(start..end)?;
    let want = t.w as usize * t.h as usize * 4;
    if px.len() < want {
        return None;
    }
    Some(px[..want].to_vec())
}

/// 由已载入的投影与方块库组装场景。
///
/// 这样拆分是为了让组装逻辑可脱离文件 IO 单独测试。
pub fn build_from(sc: &Schematic, db: &BlockDb) -> AppResult<SceneData> {
    let summary = sc.summary();
    let version_idx = db
        .pick_version(summary.data_version)
        .ok_or_else(|| AppError::Parse("方块库中没有任何可用版本".into()))?;
    let version = &db.versions[version_idx];

    let quads = visibility::collect_quads(sc, db, version_idx);
    let m = mesh::build(&quads);

    // 统计方块库中查不到定义的方块**种类**，供界面提示。
    // 用集合去重：同一方块会出现在多个区域的调色板中，
    // 逐项累加得到的是「(区域, 方块) 组合数」而不是种类数。
    let mut unresolved: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for r in &sc.regions {
        for b in &r.palette {
            if b.is_air() {
                continue;
            }
            if crate::block_model::find_block(db, version, &b.name).is_none() {
                unresolved.insert(b.name.as_str());
            }
        }
    }
    let unresolved = unresolved.len();

    let mut batches = Vec::with_capacity(m.batches.len());
    for b in m.batches {
        let Some(t) = db.textures.get(b.texture as usize) else { continue };
        let Some(rgba) = texture_rgba(db, b.texture) else {
            // 贴图数据缺失时跳过该批次：前端拿不到像素就只能画占位色块，
            // 反而掩盖真实问题，不如不画。
            continue;
        };
        batches.push(SceneBatch {
            texture: b.texture,
            positions: b.positions,
            uvs: b.uvs,
            colors: b.colors,
            indices: b.indices,
            texture_width: t.w,
            texture_height: t.h,
            texture_rgba: rgba,
        });
    }

    let total_vertices = batches.iter().map(|b| b.positions.len() / 3).sum();
    let total_triangles = batches.iter().map(|b| b.indices.len() / 3).sum();

    // 投影在世界坐标中的起点：取所有区域的最小位置
    let mut origin = [i32::MAX; 3];
    for r in &sc.regions {
        for a in 0..3 {
            origin[a] = origin[a].min(r.pos[a]);
        }
    }
    if sc.regions.is_empty() {
        origin = [0, 0, 0];
    }

    Ok(SceneData {
        batches,
        total_vertices,
        total_triangles,
        enclosing: summary.enclosing,
        origin,
        mc_version_name: summary.mc_version_name,
        data_version: summary.data_version,
        unresolved_blocks: unresolved,
    })
}

/// 从文件载入投影并组装场景，方块库由调用方提供。
///
/// 拆成独立入口是为了让调用方复用已载入的方块库：
/// 它解压后约 5.6 MB、含 4 万余条方块定义，每次预览都重新解析会明显变慢。
pub fn build_with_db(schematic_path: &str, db: &BlockDb) -> AppResult<SceneData> {
    if db.versions.is_empty() {
        return Err(AppError::Parse("方块库为空".into()));
    }
    let sc = Schematic::load(schematic_path)?;
    build_from(&sc, db)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blockdb::NO_TEXTURE;
    use crate::blockdb::{BlockDef, Model, ModelElement, ModelFace, Variant, VersionDef};
    use crate::litematic::{BlockState, Region};
    use std::collections::HashMap;

    /// 造一个方块库：单个版本，一个完整不透明立方体方块
    fn test_db() -> BlockDb {
        // cullface 填对应面序号（0..5），与真实模型一致；
        // 填 0xFF 表示不参与剔除，那样相邻方块之间不会互相隐藏面。
        let faces: Vec<ModelFace> = (0..6u8)
            .map(|i| ModelFace {
                // 0 是字符串表下标而非贴图池下标：
                // 文件里该字段指向贴图路径，必须经版本映射才能得到池下标。
                // 测试库故意让两者不同（strings[0] 对应池下标 1），
                // 否则"当成池下标直接用"这个缺陷在测试中不会暴露。
                texture: 0,
                uv: [0.0, 0.0, 16.0, 16.0],
                cullface: i,
                tintindex: -1,
                rotation: 0,
            })
            .collect();
        let model = Model {
            translucent: false,
            elements: vec![ModelElement {
                from: [0.0, 0.0, 0.0],
                to: [16.0, 16.0, 16.0],
                has_rotation: false,
                rot_kind: 0,
                origin: [0.0; 3],
                axis: 0,
                angle: 0.0,
                rescale: false,
                rot_vec: [0.0; 3],
                face_mask: 0x3F,
                faces,
            }],
        };
        let version = VersionDef {
            name: "1.20.1".into(),
            data_version: 3465,
            lang_idx: 0,
            texture_by_path: HashMap::from([("block/test".to_string(), 1u32)]),
            blocks: vec![BlockDef {
                name: "minecraft:stone".into(),
                variants: vec![Variant {
                    when: String::new(),
                    model_idx: 0,
                    rot_x_div90: 0,
                    rot_y_div90: 0,
                    uvlock: false,
                }],
                multipart: Vec::new(),
            }],
            langs: Vec::new(),
            is_overlay: false,
        };
        BlockDb {
            strings: vec!["block/test".to_string()],
            langs: Vec::new(),
            // 池下标 1 才是该方块要用的贴图；池下标 0 是干扰项，
            // 若把字符串下标当池下标用就会取到它
            textures: vec![
                TextureInfo { w: 2, h: 2, offset: 0, len: 16, has_alpha: false },
                TextureInfo { w: 2, h: 2, offset: 0, len: 16, has_alpha: false },
            ],
            models: vec![model],
            versions: vec![version],
            pixels: (0..16).collect(),
        }
    }

    /// 一个 2x1x1 的投影，全部为石头（非空气，会互相剔除部分面）
    fn test_schematic() -> Schematic {
        Schematic {
            root: crate::nbt::Value::Compound(vec![
                // 缺这一项时 summary 的 data_version 为 0，选版本会落到别的版本
                ("MinecraftDataVersion".into(), crate::nbt::Value::Int(3465)),
                (
                "Metadata".into(),
                crate::nbt::Value::Compound(vec![
                    ("Name".into(), crate::nbt::Value::String("测试".into())),
                    ("EnclosingSize".into(), crate::nbt::Value::Compound(vec![
                        ("x".into(), crate::nbt::Value::Int(2)),
                        ("y".into(), crate::nbt::Value::Int(1)),
                        ("z".into(), crate::nbt::Value::Int(1)),
                    ])),
                ]),
                ),
            ]),
            regions: vec![Region {
                name: "r".into(),
                pos: [0, 0, 0],
                size: [2, 1, 1],
                volume: 2,
                bits: 2,
                palette: vec![BlockState::new("minecraft:stone")],
                indices: vec![0, 0],
                ..Default::default()
            }],
            path: String::new(),
            meta_modified: false,
            blocks_modified: false,
        }
    }

    #[test]
    fn builds_batches_with_texture_pixels() {
        let sc = test_schematic();
        let db = test_db();
        let scene = build_from(&sc, &db).expect("组装失败");

        assert!(!scene.is_empty(), "应产出至少一个批次");
        assert_eq!(scene.batches.len(), 1);
        let b = &scene.batches[0];
        // 面的贴图字段是字符串表下标 0，经版本映射后应得到贴图池下标 1。
        // 若直接把字符串下标当池下标用，这里会得到 0 并失败——
        // 这正是要防的缺陷：取到的是完全无关的贴图。
        assert_eq!(b.texture, 1, "字符串表下标必须解析成贴图池下标");
        assert_eq!(b.texture_width, 2);
        assert_eq!(b.texture_height, 2);
        assert_eq!(b.texture_rgba.len(), 2 * 2 * 4, "贴图像素长度应为 宽*高*4");
        assert_eq!(b.texture_rgba, (0..16).collect::<Vec<u8>>());
        assert_eq!(scene.total_vertices, b.positions.len() / 3);
        assert_eq!(scene.total_triangles, b.indices.len() / 3);
    }

    #[test]
    fn enclosing_and_origin_are_reported() {
        let sc = test_schematic();
        let db = test_db();
        let scene = build_from(&sc, &db).unwrap();
        assert_eq!(scene.enclosing, [2, 1, 1]);
        assert_eq!(scene.origin, [0, 0, 0]);
        assert_eq!(scene.data_version, 3465);
    }

    #[test]
    fn adjacent_blocks_hide_shared_faces() {
        // 两个相邻的完整立方体：贴合的那两个面应被剔除，
        // 只留下 2 个方块 × 6 面 - 2 个共享面 × 2 = 10 个面
        let sc = test_schematic();
        let db = test_db();
        let scene = build_from(&sc, &db).unwrap();
        let faces = scene.total_triangles / 2;
        assert_eq!(faces, 10, "相邻面应被剔除，实际 {faces} 个面");
    }

    #[test]
    fn unknown_block_is_counted_as_unresolved() {
        let mut sc = test_schematic();
        sc.regions[0].palette = vec![BlockState::new("mod:unknown")];
        let db = test_db();
        let scene = build_from(&sc, &db).unwrap();
        assert_eq!(scene.unresolved_blocks, 1, "查不到定义的方块应被统计");
        assert!(scene.is_empty(), "没有模型就不应产出批次");
    }

    #[test]
    fn missing_texture_pixels_skips_batch() {
        let mut db = test_db();
        // 声明长度为 16 但像素池只有 4 字节，数据不足
        db.textures[0].len = 16;
        db.pixels = vec![0u8; 4];
        let sc = test_schematic();
        let scene = build_from(&sc, &db).unwrap();
        assert!(scene.is_empty(), "贴图数据不足时应跳过该批次而不是画错色");
    }

    #[test]
    fn air_only_schematic_yields_empty_scene() {
        let mut sc = test_schematic();
        sc.regions[0].palette = vec![BlockState::new("minecraft:air")];
        let db = test_db();
        let scene = build_from(&sc, &db).unwrap();
        assert!(scene.is_empty());
        assert_eq!(scene.total_triangles, 0);
    }

    #[test]
    fn no_available_version_is_an_error() {
        let mut db = test_db();
        db.versions.clear();
        let sc = test_schematic();
        let e = build_from(&sc, &db).unwrap_err().to_string();
        assert!(e.contains("版本"), "错误信息应说明缺少可用版本：{e}");
    }

    #[test]
    fn sentinel_texture_produces_no_batch() {
        let mut db = test_db();
        for f in db.models[0].elements[0].faces.iter_mut() {
            f.texture = NO_TEXTURE;
        }
        let sc = test_schematic();
        let scene = build_from(&sc, &db).unwrap();
        assert!(scene.is_empty(), "哨兵贴图不应产生批次");
    }
}
