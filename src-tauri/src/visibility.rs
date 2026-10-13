//! 可见面判定：邻面剔除与四边形生成。
//!
//! 对照 legacy-cpp/render.cpp 的 ComputeOpaque 与 RenderSchematic 中的
//! 面遍历。这一层把解析层、模型解析与几何串起来，产出光栅化的输入。

use crate::block_model::resolve_models;
use crate::blockdb::{BlockDb, Face, NO_TEXTURE};
use crate::geometry::{build_face_corners, cull_neighbor_offset, face_shade, rotate_uv};
use crate::transform::{rotate_cull_offset, to_world};
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
///
/// 取 `version_idx` 而不是 `&VersionDef`：面的贴图字段是**字符串表下标**，
/// 只有经 `BlockDb::texture_pool_index` 按版本解析后才能得到贴图池下标
/// （见 `ModelFace::texture` 的说明）。持有版本引用并不足以完成这层解析，
/// 会诱使调用者退化成把字符串下标当池下标用——那正是这里此前的缺陷：
/// 树叶被误判为遮光，邻面被错误剔除，渲染出现空洞。
pub fn is_opaque(db: &BlockDb, version_idx: usize, state: &BlockState) -> bool {
    let Some(version) = db.versions.get(version_idx) else {
        return false;
    };
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
        // 先按版本把字符串表下标解析成贴图池下标，再取贴图属性
        let pool = db.texture_pool_index(version_idx, f.texture);
        match db.textures.get(pool as usize) {
            Some(t) if !t.has_alpha => {}
            _ => return false,
        }
    }
    true
}

/// 由方块状态与面朝向推导出的一个可见四边形
#[derive(Debug, Clone, PartialEq)]
pub struct Quad {
    /// 四个角点，已变换到世界坐标（单位为方块）。
    /// 变换顺序为元素旋转 → 变体旋转 → 除以 16 并平移到方块位置。
    pub corners: [[f32; 3]; 4],
    pub uv: [[f32; 2]; 4],
    /// 贴图池下标（已由字符串表下标解析而来）
    pub texture: u32,
    /// 该面的明暗系数（已含世界法线方向光）
    pub shade: f32,
    /// 该面的染色系数（RGB，各 0..1）。无染色时为全 1。
    pub tint: [f32; 3],
    /// 该四边形所属方块在区域内的坐标
    pub block_pos: [i32; 3],
}

/// 渲染时是否跳过该方块（不产生任何面）。
///
/// 比统计口径多一个 structure_void：legacy-cpp 渲染时跳过它
/// （render.cpp:316-317），而统计把它算作非空气（litematic.cpp:374）。
/// 两处口径不同是有意的，改名以示区别，避免日后被「统一」掉。
pub(crate) fn is_render_skipped(name: &str) -> bool {
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
/// 收集可见四边形。
///
/// 收版本**下标**而非引用：面贴图在文件里是字符串表下标，必须按版本解析成
/// 贴图池下标，而这一步需要回到 db.versions 查该版本的路径映射。
pub fn collect_quads(sc: &Schematic, db: &BlockDb, ver_idx: usize) -> Vec<Quad> {
    let Some(version) = db.versions.get(ver_idx) else {
        return Vec::new();
    };
    let mut out = Vec::new();

    for r in &sc.regions {
        // -1 未计算，0 非不透明，1 不透明
        let mut opaque_cache: Vec<i8> = vec![-1; r.palette.len()];

        for (i, pi) in r.indices.iter().enumerate() {
            let Some(state) = r.palette.get(*pi as usize) else { continue };
            if is_render_skipped(&state.name) {
                continue;
            }
            // 区域在投影中的位置必须计入，否则多区域投影的各区域会
            // 全部重叠在原点。
            // 注意：legacy-cpp 只用 PosFromIndex、从不加 r.pos
            // （render.cpp:323-326 是全仓库唯一一处取方块坐标的地方），
            // 因此多区域投影在旧版下会重叠。这是有意修正，单区域行为不变。
            //
            // 坐标用 checked_add：pos 与 local 都来自文件内容（Size 分量
            // 最大 i32::MAX），相加可能溢出——debug 构建会 panic，
            // release 会回绕成负坐标而把方块画到错误位置。
            // 溢出说明该方块落在可表示范围之外，跳过它而不是给出错误坐标。
            let local = r.pos_from_index(i);
            let (Some(bx), Some(by), Some(bz)) = (
                r.pos[0].checked_add(local[0]),
                r.pos[1].checked_add(local[1]),
                r.pos[2].checked_add(local[2]),
            ) else {
                continue;
            };
            let bp = [bx, by, bz];

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
                                                .map_or(false, |s| is_opaque(db, ver_idx, s));
                                            opaque_cache[idx] = if opaque { 1 } else { 0 };
                                        }
                                        if opaque_cache[idx] == 1 {
                                            continue; // 被完全遮挡
                                        }
                                    }
                                }
                            }
                        }

                        // 面的 rotation 字段只影响贴图方向，不影响几何
                        let corners = build_face_corners(
                            face,
                            elem.from,
                            elem.to,
                            rotate_uv(mf.uv, mf.rotation),
                        );
                        // 角点此刻仍在模型空间：既不随元素旋转、也不随变体
                        // 旋转、也没有平移到方块位置，尺度是方块坐标的 16 倍。
                        // 不在这里变换，所有方块的所有面会重叠在原点。
                        let world = std::array::from_fn(|k| {
                            to_world(
                                corners.pos[k],
                                elem,
                                rm.rot_x_div90,
                                rm.rot_y_div90,
                                bp,
                            )
                        });

                        out.push(Quad {
                            corners: world,
                            uv: corners.uv,
                            // 文件里的贴图字段是字符串表下标，必须按版本
                            // 解析成贴图池下标后才能给下游取像素
                            texture: db.texture_pool_index(ver_idx, mf.texture),
                            shade: shade_for(face, &world),
                            // 染色按方块名决定：同一个 tintindex 在草方块与
                            // 红石线上含义不同（见 tint 模块）
                            tint: crate::tint::tint_color(&state.name, &state.props, mf.tintindex),
                            block_pos: bp,
                        });
                    }
                }
            }
        }
    }
    out
}

/// 求一个面的明暗系数。
///
/// 底数是该面在**模型空间**的固定系数（顶面最亮、底面最暗），
/// 再乘一个按**世界空间法线**算的方向光——后者不能省：
///
/// 模型空间的面号在方块被变体旋转后已不再对应该面的实际朝向。
/// 例如活塞 `facing=up` 时绕 X 转 270°，模型的「顶面」在世界里朝下；
/// 若仍按模型面号取系数，就会把朝下的面照成最亮、朝上的面照成最暗——
/// 观感上就是「旋转过的方块明暗不对」。对照 legacy-cpp/render.cpp 的
/// emitQuad，它同样在 FaceShade 之上再乘一次按几何法线的 lambert。
fn shade_for(face: Face, corners: &[[f32; 3]; 4]) -> f32 {
    let base = face_shade(face);
    let Some(n) = face_normal(corners) else {
        return base;
    };
    // 方向光：与 legacy 保持一致，让结构有稳定的立体感
    let lambert = 0.62
        + 0.38 * (-n[1] * 0.65 + n[2] * 0.25 - n[0] * 0.2 + 0.5).max(0.0);
    base * lambert.clamp(0.6, 1.15)
}

/// 由四个角点求面的单位法线（前三点构成三角形）。
///
/// 退化三角形（面积为 0）返回 None，由调用方退回按面号取系数——
/// 这类面本身不可见，不值得为它编一个方向。
fn face_normal(corners: &[[f32; 3]; 4]) -> Option<[f32; 3]> {
    let a = corners[0];
    let b = corners[1];
    let c = corners[2];
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let n = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if len < 1e-9 {
        return None;
    }
    Some([n[0] / len, n[1] / len, n[2] / len])
}

fn cullface_to_face(v: u8) -> Face {    match v {
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
    use crate::blockdb::{Model, ModelElement, ModelFace, TextureInfo, VersionDef};

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
            // 真实数据里面的贴图字段是字符串表下标，须经此映射得到池下标
            texture_by_path: std::collections::HashMap::from([(
                "block/stone".to_string(),
                0u32,
            )]),
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
            strings: vec!["block/stone".to_string()],
            langs: Vec::new(),
            textures: vec![ti],
            models: vec![m],
            versions: vec![version.clone()],
            pixels: vec![0u8; 1024],
        };
        (db, version)
    }

    /// 收集到的四边形必须已经落到该方块的世界坐标上。
    ///
    /// 这条测试防的是「角点停留在模型空间」：若未做旋转与平移，
    /// 所有方块的所有面会重叠在原点且尺度放大 16 倍，
    /// 而只检查数量的测试完全发现不了。
    #[test]
    fn quads_are_transformed_to_block_world_position() {
        let (db, _v) = build(false, |_| {});
        let sc = Schematic {
            root: crate::nbt::Value::Compound(vec![]),
            regions: vec![crate::litematic::Region {
                name: "r".into(),
                pos: [3, 5, 7],
                size: [1, 1, 1],
                volume: 1,
                bits: 2,
                palette: vec![BlockState::new("minecraft:stone")],
                indices: vec![0],
                ..Default::default()
            }],
            path: String::new(),
            meta_modified: false,
            blocks_modified: false,
        };
        let quads = collect_quads(&sc, &db, 0);
        assert!(!quads.is_empty());

        // 区域位于 (3,5,7)，方块尺寸为 1，因此所有角点都应落在
        // [3,4] × [5,6] × [7,8] 之内。若未平移，坐标会落在 0..16。
        for q in &quads {
            for c in &q.corners {
                for (axis, v) in c.iter().enumerate() {
                    let lo = [3.0, 5.0, 7.0][axis];
                    let hi = lo + 1.0;
                    assert!(
                        *v >= lo - 1e-4 && *v <= hi + 1e-4,
                        "角点 {c:?} 超出方块范围；轴 {axis} 应在 {lo}..{hi}，\
                         说明角点没有被变换到世界坐标"
                    );
                }
            }
        }
    }

    /// 坐标相加溢出时必须跳过该方块，而不是 panic 或给出回绕的坐标。
    ///
    /// pos 与 local 都来自文件内容：Size 分量最大 i32::MAX，pos 也可接近
    /// 该值。原先直接相加，debug 构建（cargo test）会 panic
    /// `attempt to add with overflow`，release 构建会回绕成负坐标，
    /// 把方块画到完全错误的位置。
    #[test]
    fn overflowing_block_position_is_skipped_not_wrapped() {
        let (db, _v) = build(false, |_| {});
        let sc = Schematic {
            root: crate::nbt::Value::Compound(vec![]),
            regions: vec![crate::litematic::Region {
                name: "r".into(),
                // 原点取 i32::MAX：再加任何非零偏移都会溢出
                pos: [i32::MAX, i32::MAX, i32::MAX],
                // 尺寸 2 使 local 可以为 1，从而触发相加溢出
                size: [2, 2, 2],
                volume: 8,
                bits: 2,
                palette: vec![BlockState::new("minecraft:stone")],
                indices: vec![0; 8],
                ..Default::default()
            }],
            path: String::new(),
            meta_modified: false,
            blocks_modified: false,
        };
        // 不应 panic；可以输出为空（全部跳过），但绝不能带出错坐标
        let quads = collect_quads(&sc, &db, 0);
        for q in &quads {
            for c in &q.corners {
                for v in c {
                    assert!(
                        v.is_finite(),
                        "角点坐标必须是有限值，实际 {c:?}"
                    );
                    assert!(
                        *v >= i32::MAX as f32 - 2.0,
                        "不应出现回绕后的负坐标，实际 {c:?}"
                    );
                }
            }
        }
    }

    /// 面的贴图字段必须按版本解析成贴图池下标。
    ///
    /// 两者数值空间无关：文件里的取值指向字符串表。若直接当池下标用，
    /// 多数面会取不到贴图，少数会取到完全无关的贴图。
    #[test]
    fn face_texture_is_resolved_through_version_map() {
        // full_cube 的贴图字段为 1（字符串表下标），strings[1] 指向池下标 5
        let (mut m, ti) = full_cube(1, false);
        for f in m.elements[0].faces.iter_mut() {
            f.texture = 1;
        }
        let version = VersionDef {
            name: "1.20.1".into(),
            data_version: 3465,
            lang_idx: 0,
            texture_by_path: std::collections::HashMap::from([(
                "block/stone".to_string(),
                5u32,
            )]),
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
        let mut textures = vec![ti.clone(); 6];
        textures[5] = ti;
        let db = BlockDb {
            // 下标 1 是路径，下标 0 留空以便越界时报错
            strings: vec![String::new(), "block/stone".to_string()],
            langs: Vec::new(),
            textures,
            models: vec![m],
            versions: vec![version],
            pixels: vec![0u8; 6 * 1024],
        };
        let sc = Schematic {
            root: crate::nbt::Value::Compound(vec![]),
            regions: vec![crate::litematic::Region {
                name: "r".into(),
                pos: [0, 0, 0],
                size: [1, 1, 1],
                volume: 1,
                bits: 2,
                palette: vec![BlockState::new("minecraft:stone")],
                indices: vec![0],
                ..Default::default()
            }],
            path: String::new(),
            meta_modified: false,
            blocks_modified: false,
        };
        let quads = collect_quads(&sc, &db, 0);
        assert!(!quads.is_empty());
        for q in &quads {
            assert_eq!(
                q.texture, 5,
                "贴图应为解析后的池下标 5，而不是字符串表下标 1\
                 （若得到 1 说明未做解析，会取到完全无关的贴图）"
            );
        }
    }

    #[test]
    fn plain_full_cube_is_opaque() {
        let (db, _v) = build(false, |_| {});
        assert!(is_opaque(&db, 0, &BlockState::new("minecraft:stone")));
    }

    /// 不透明判定必须**按版本解析**贴图字段，不能把字符串表下标当池下标用。
    ///
    /// 面的贴图字段是字符串表下标（见 ModelFace::texture 的说明），两者数值
    /// 空间无关：随包库里字符串表 6263 项、贴图池只有 1685 项，约一半的取值
    /// 超出池长度。误用会让半透明方块被判成「完全遮光」，进而剔除邻面、
    /// 渲染出洞。
    ///
    /// 这里刻意构造「字符串下标 1 → 池下标 5」的错位，并让两个位置的
    /// has_alpha 相反：只有走对解析路径才能得到「含透明 → 不遮光」的结论。
    #[test]
    fn opacity_resolves_texture_through_version_not_as_pool_index() {
        // 池下标 5 的贴图含透明（树叶/玻璃），池下标 1 的不含。
        // 面的字段是字符串表下标 1，字符串表 [1] = "block/leaves" 映射到池下标 5。
        let (mut m, _) = full_cube(1, true);
        for f in m.elements[0].faces.iter_mut() {
            f.texture = 1;
        }
        let version = VersionDef {
            name: "1.20.1".into(),
            data_version: 3465,
            lang_idx: 0,
            texture_by_path: std::collections::HashMap::from([(
                "block/leaves".to_string(),
                5u32,
            )]),
            blocks: vec![crate::blockdb::BlockDef {
                name: "minecraft:oak_leaves".into(),
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
        // 池下标 1 不透明，池下标 5 含透明
        let opaque_tex = TextureInfo { w: 16, h: 16, offset: 0, len: 1024, has_alpha: false };
        let alpha_tex = TextureInfo { w: 16, h: 16, offset: 0, len: 1024, has_alpha: true };
        let mut textures = vec![opaque_tex; 6];
        textures[1] = opaque_tex;
        textures[5] = alpha_tex;
        let db = BlockDb {
            strings: vec![String::new(), "block/leaves".to_string()],
            langs: Vec::new(),
            textures,
            models: vec![m],
            versions: vec![version.clone()],
            pixels: vec![0u8; 6 * 1024],
        };

        assert!(
            !is_opaque(&db, 0, &BlockState::new("minecraft:oak_leaves")),
            "贴图字段是字符串表下标，必须按版本解析到含透明的池下标 5；\
             直接拿 1 当池下标会读到不透明贴图，从而错误地判定遮光并剔除邻面"
        );
    }

    #[test]
    fn textured_transparency_is_not_opaque() {
        // 贴图含透明通道（玻璃、树叶）不能剔除邻居
        let (db, _v) = build(true, |_| {});
        assert!(!is_opaque(&db, 0, &BlockState::new("minecraft:stone")));
    }

    #[test]
    fn partial_and_rotated_elements_are_not_opaque() {
        // 非完整立方体（台阶）
        let (db, _v) = build(false, |m| m.elements[0].to[1] = 8.0);
        assert!(!is_opaque(&db, 0, &BlockState::new("minecraft:stone")));

        // 带旋转
        let (db, _v) = build(false, |m| m.elements[0].has_rotation = true);
        assert!(!is_opaque(&db, 0, &BlockState::new("minecraft:stone")));

        // 带 rescale
        let (db, _v) = build(false, |m| m.elements[0].rescale = true);
        assert!(!is_opaque(&db, 0, &BlockState::new("minecraft:stone")));
    }

    #[test]
    fn missing_faces_are_not_opaque() {
        // 缺一个面（例如只有五个面的方块）不能剔除邻居
        let (db, _v) = build(false, |m| m.elements[0].face_mask = 0x1F);
        assert!(!is_opaque(&db, 0, &BlockState::new("minecraft:stone")));
    }

    #[test]
    fn translucent_model_is_not_opaque() {
        let (db, _v) = build(false, |m| m.translucent = true);
        assert!(!is_opaque(&db, 0, &BlockState::new("minecraft:stone")));
    }

    #[test]
    fn missing_texture_is_not_opaque() {
        let (db, _v) = build(false, |m| {
            for f in m.elements[0].faces.iter_mut() {
                f.texture = NO_TEXTURE;
            }
        });
        assert!(!is_opaque(&db, 0, &BlockState::new("minecraft:stone")));
    }

    #[test]
    fn unknown_block_is_not_opaque() {
        let (db, _v) = build(false, |_| {});
        assert!(!is_opaque(&db, 0, &BlockState::new("mod:unknown")));
        assert!(!is_opaque(&db, 0, &BlockState::new("minecraft:air")));
    }

    #[test]
    fn air_blocks_produce_no_quads() {
        // 版本由 db 自己持有，这里只传下标；测试库只有一个版本
        let (db, _v) = build(false, |_| {});
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
            meta_modified: false,
            blocks_modified: false,
        };
        assert!(collect_quads(&sc, &db, 0).is_empty(), "空气不应产生任何面");
    }
}
