//! 可见面判定：邻面剔除与四边形生成。
//!
//! 对照 legacy-cpp/render.cpp 的 ComputeOpaque 与 RenderSchematic 中的
//! 面遍历。这一层把解析层、模型解析与几何串起来，产出光栅化的输入。

use crate::block_model::resolve_models;
use crate::blockdb::{BlockDb, Face, VersionDef, NO_TEXTURE};
use crate::geometry::{build_face_corners, cull_neighbor_offset, face_shade};
use crate::transform::rotate_cull_offset;
use crate::litematic::{BlockState, Schematic};

/// 判断一个方块状态是否「完全遮光」，即能否剔除相邻方块朝向它的面。
///
/// 判定条件刻意保守——只有普通完整不透明立方体才算：
///   - 恰好一个模型，且非半透明
///   - 恰好一个元素，无旋转、无 rescale
///   - 元素范围恰为 (0,0,0)-(16,16,16)
///   - 六个面齐全，且每面贴图都存在、都不含透明通道
///
/// 台阶、楼梯、玻璃、树叶等都不满足，因此不会错误地剔掉它们背后的面。
pub fn is_opaque(db: &BlockDb, version: &VersionDef, state: &BlockState) -> bool {
    let models = resolve_models(db, version, state);
    if models.len() != 1 {
        return false;
    }
    let Some(model) = db.models.get(models[0].model_idx as usize) else {
        return false;
    };
    if model.translucent || model.elements.len() != 1 {
        return false;
    }
    let e = &model.elements[0];
    if e.has_rotation || e.rescale {
        return false;
    }
    if e.from != [0.0, 0.0, 0.0] || e.to != [16.0, 16.0, 16.0] {
        return false;
    }
    // 六个面齐全（位 0~5 全置）
    if e.face_mask != 0x3F {
        return false;
    }
    // 每个面都要有贴图，且贴图不含透明通道
    for f in &e.faces {
        if f.texture == NO_TEXTURE {
            return false;
        }
        match db.textures.get(f.texture as usize) {
            Some(t) if !t.has_alpha => {}
            _ => return false,
        }
    }
    true
}

/// 由方块状态与面朝向推导出的一个可见四边形
#[derive(Debug, Clone, PartialEq)]
pub struct Quad {
    /// 四个角点，模型空间 0..16（尚未平移到方块位置）
    pub corners: [[f32; 3]; 4],
    pub uv: [[f32; 2]; 4],
    /// 贴图池下标
    pub texture: u32,
    /// 该面的明暗系数
    pub shade: f32,
    /// 该四边形所属方块在区域内的坐标
    pub block_pos: [i32; 3],
}

/// 是否为空气（不产生任何面）
fn is_air(name: &str) -> bool {
    matches!(
        name,
        "minecraft:air"
            | "minecraft:cave_air"
            | "minecraft:void_air"
            | "minecraft:structure_void"
    )
}

/// 收集整个投影的可见四边形。
///
/// 不透明判定按调色板索引缓存：同一区域内每种方块状态只计算一次，
/// 避免逐方块重复解析模型。
pub fn collect_quads(sc: &Schematic, db: &BlockDb, version: &VersionDef) -> Vec<Quad> {
    let mut out = Vec::new();

    for r in &sc.regions {
        // -1 未计算，0 非不透明，1 不透明
        let mut opaque_cache: Vec<i8> = vec![-1; r.palette.len()];

        for (i, pi) in r.indices.iter().enumerate() {
            let Some(state) = r.palette.get(*pi as usize) else { continue };
            if is_air(&state.name) {
                continue;
            }
            let bp = r.pos_from_index(i);

            let models = resolve_models(db, version, state);
            for rm in &models {
                let Some(model) = db.models.get(rm.model_idx as usize) else { continue };
                for elem in &model.elements {
                    for face_idx in 0..6u8 {
                        let face = match face_idx {
                            0 => Face::Down,
                            1 => Face::Up,
                            2 => Face::North,
                            3 => Face::South,
                            4 => Face::West,
                            _ => Face::East,
                        };
                        if elem.face_mask & (1u8 << face_idx) == 0 {
                            continue;
                        }
                        let Some(mf) = elem.face(face) else { continue };

                        // 邻面剔除。cullface 为 0xFF 表示不参与剔除。
                        if mf.cullface != 0xFF {
                            // 剔除方向必须随变体旋转：方块转向后，它朝向某个
                            // 邻居的那一面也变了，否则会剔除错误的邻居。
                            let off = rotate_cull_offset(
                                cull_neighbor_offset(cullface_to_face(mf.cullface)),
                                rm.rot_x_div90,
                                rm.rot_y_div90,
                            );
                            let n = [bp[0] + off[0], bp[1] + off[1], bp[2] + off[2]];
                            if r.inside(n[0], n[1], n[2]) {
                                // 区域外视为空气，不剔除：否则结构的外表面会缺失
                                let ni = r.index(n[0], n[1], n[2]);
                                if let Some(npi) = r.indices.get(ni) {
                                    let idx = *npi as usize;
                                    if idx < opaque_cache.len() {
                                        if opaque_cache[idx] < 0 {
                                            // 注意：Rust 只为 u8 实现了 From<bool>，
                                            // i8::from(bool) 不存在，必须显式取值
                                            let opaque = r
                                                .palette
                                                .get(idx)
                                                .map_or(false, |s| is_opaque(db, version, s));
                                            opaque_cache[idx] = if opaque { 1 } else { 0 };
                                        }
                                        if opaque_cache[idx] == 1 {
                                            continue; // 被完全遮挡
                                        }
                                    }
                                }
                            }
                        }

                        let corners = build_face_corners(face, elem.from, elem.to, mf.uv);
                        out.push(Quad {
                            corners: corners.pos,
                            uv: corners.uv,
                            texture: mf.texture,
                            shade: face_shade(face),
                            block_pos: bp,
                        });
                    }
                }
            }
        }
    }
    out
}

fn cullface_to_face(v: u8) -> Face {
    match v {
        0 => Face::Down,
        1 => Face::Up,
        2 => Face::North,
        3 => Face::South,
        4 => Face::West,
        _ => Face::East,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blockdb::{Model, ModelElement, ModelFace, TextureInfo};

    /// 造一个「完整不透明立方体」模型
    fn full_cube(texture: u32, has_alpha: bool) -> (Model, TextureInfo) {
        let faces: Vec<ModelFace> = (0..6)
            .map(|_| ModelFace {
                texture,
                uv: [0.0, 0.0, 16.0, 16.0],
                cullface: 0xFF,
                tintindex: -1,
                rotation: 0,
            })
            .collect();
        (
            Model {
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
            },
            TextureInfo { w: 16, h: 16, offset: 0, len: 1024, has_alpha },
        )
    }

    fn build(texture_alpha: bool, mutate: impl FnOnce(&mut Model)) -> (BlockDb, VersionDef) {
        let (mut m, ti) = full_cube(0, texture_alpha);
        mutate(&mut m);
        let version = VersionDef {
            name: "1.20.1".into(),
            data_version: 3465,
            lang_idx: 0,
            texture_by_path: std::collections::HashMap::new(),
            blocks: vec![crate::blockdb::BlockDef {
                name: "minecraft:stone".into(),
                variants: vec![crate::blockdb::Variant {
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
        let db = BlockDb {
            format_version: 3,
            strings: Vec::new(),
            langs: Vec::new(),
            textures: vec![ti],
            models: vec![m],
            versions: vec![version.clone()],
            pixels: vec![0u8; 1024],
        };
        (db, version)
    }

    #[test]
    fn plain_full_cube_is_opaque() {
        let (db, v) = build(false, |_| {});
        assert!(is_opaque(&db, &v, &BlockState::new("minecraft:stone")));
    }

    #[test]
    fn textured_transparency_is_not_opaque() {
        // 贴图含透明通道（玻璃、树叶）不能剔除邻居
        let (db, v) = build(true, |_| {});
        assert!(!is_opaque(&db, &v, &BlockState::new("minecraft:stone")));
    }

    #[test]
    fn partial_and_rotated_elements_are_not_opaque() {
        // 非完整立方体（台阶）
        let (db, v) = build(false, |m| m.elements[0].to[1] = 8.0);
        assert!(!is_opaque(&db, &v, &BlockState::new("minecraft:stone")));

        // 带旋转
        let (db, v) = build(false, |m| m.elements[0].has_rotation = true);
        assert!(!is_opaque(&db, &v, &BlockState::new("minecraft:stone")));

        // 带 rescale
        let (db, v) = build(false, |m| m.elements[0].rescale = true);
        assert!(!is_opaque(&db, &v, &BlockState::new("minecraft:stone")));
    }

    #[test]
    fn missing_faces_are_not_opaque() {
        // 缺一个面（例如只有五个面的方块）不能剔除邻居
        let (db, v) = build(false, |m| m.elements[0].face_mask = 0x1F);
        assert!(!is_opaque(&db, &v, &BlockState::new("minecraft:stone")));
    }

    #[test]
    fn translucent_model_is_not_opaque() {
        let (db, v) = build(false, |m| m.translucent = true);
        assert!(!is_opaque(&db, &v, &BlockState::new("minecraft:stone")));
    }

    #[test]
    fn missing_texture_is_not_opaque() {
        let (db, v) = build(false, |m| {
            for f in m.elements[0].faces.iter_mut() {
                f.texture = NO_TEXTURE;
            }
        });
        assert!(!is_opaque(&db, &v, &BlockState::new("minecraft:stone")));
    }

    #[test]
    fn unknown_block_is_not_opaque() {
        let (db, v) = build(false, |_| {});
        assert!(!is_opaque(&db, &v, &BlockState::new("mod:unknown")));
        assert!(!is_opaque(&db, &v, &BlockState::new("minecraft:air")));
    }

    #[test]
    fn air_blocks_produce_no_quads() {
        let (db, v) = build(false, |_| {});
        let sc = Schematic {
            root: crate::nbt::Value::Compound(vec![]),
            regions: vec![crate::litematic::Region {
                name: "r".into(),
                pos: [0, 0, 0],
                size: [1, 1, 1],
                volume: 1,
                bits: 2,
                palette: vec![BlockState::new("minecraft:air")],
                indices: vec![0],
                ..Default::default()
            }],
            path: String::new(),
            loaded: true,
            meta_modified: false,
            blocks_modified: false,
        };
        assert!(collect_quads(&sc, &db, &v).is_empty(), "空气不应产生任何面");
    }
}
