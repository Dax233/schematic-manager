//! 把解析后的模型定义转换成可写入资源包的形态。
//!
//! 用途：blockstate 里引用的是模型 id，模型定义里引用的是贴图变量；
//! 构建资源包时要先把这些引用解析成具体下标：
//!   - 模型：<ns>:<path> -> 模型池下标（按内容去重）
//!   - 贴图：`#var` 或路径 -> 贴图路径字符串表下标
//!   - 面的 cullface：面名字符串 -> 面序号（0..5），空串为不剔除
//!   - 面的 UV：缺省时按官方 FaceBakery 规则从元素边界推导
//!
//! 对照 legacy-cpp/packbuild.cpp 的 DefaultUv、FaceIndex 与 BuildModel 后半段。
//!
//! 纯逻辑：调用方提供「按 id 取模型链」与「按路径取贴图下标」的闭包，
//! 因此可完整测试。

use crate::blockdb::{Model, ModelElement, ModelFace};
use crate::blockstate::{ModelDef, ModelElementDef};
use crate::blockstate_chain::{is_force_translucent, resolve_tex_ref, Chain};
use std::collections::HashMap;

/// 六个面名，下标即面序号（与 blockdb::Face 一致）
pub const FACE_NAMES: [&str; 6] = ["down", "up", "north", "south", "west", "east"];

/// 面名对应的序号；未知面名返回 None
pub fn face_index(name: &str) -> Option<u8> {
    FACE_NAMES.iter().position(|n| *n == name).map(|i| i as u8)
}

/// 按官方规则计算缺省 UV（0..16 空间）。
///
/// 规则取自 Minecraft 的 FaceBakery，与 legacy-cpp 的 DefaultUv 一致。
/// 方向弄反会让贴图上下或左右颠倒——这类错误不影响几何，
/// 只让画面看着不对，所以按面逐一核对。
pub fn default_uv(face: u8, from: [f32; 3], to: [f32; 3]) -> [f32; 4] {
    let [x1, y1, z1] = from;
    let [x2, y2, z2] = to;
    match face {
        // down：u 随 +x，v 随 -z
        0 => [x1, 16.0 - z2, x2, 16.0 - z1],
        // up：u 随 +x，v 随 +z
        1 => [x1, z1, x2, z2],
        // north：u 随 -x，v 随 -y
        2 => [16.0 - x2, 16.0 - y2, 16.0 - x1, 16.0 - y1],
        // south：u 随 +x，v 随 -y
        3 => [x1, 16.0 - y2, x2, 16.0 - y1],
        // west：u 随 +z，v 随 -y
        4 => [z1, 16.0 - y2, z2, 16.0 - y1],
        // east：u 随 -z，v 随 -y
        _ => [16.0 - z2, 16.0 - y2, 16.0 - z1, 16.0 - y1],
    }
}

/// 模型转换所需的外部信息
pub struct ModelContext<'a> {
    /// 贴图路径（已规范成 `ns:path`）到贴图池下标的映射
    pub texture_pool: &'a HashMap<String, u32>,
    /// 字符串池：贴图路径写入模型面时使用其下标
    pub string_pool: &'a HashMap<String, u32>,
}

/// 转换结果
#[derive(Debug, Clone, PartialEq)]
pub struct ConvertedModel {
    pub model: Model,
    /// 模型引用到的贴图路径（已解析、已规范），供调用方登记到贴图池
    pub used_textures: Vec<String>,
}

/// 把元素转成 pack 形态。
///
/// `tex_of` 把贴图引用解析成「字符串池下标」：
///   - 变量 `#x` 先沿链解析成路径，再规范成 `ns:path`，最后取字符串下标
///   - 直接给路径的同样规范化后取下标
///
/// 解析失败的面直接跳过而不是写一个占位下标：占位会让模型显示错贴图，
/// 而跳过只让该面缺失，更容易被发现。
pub fn convert_element<F>(
    e: &ModelElementDef,
    chain: &Chain,
    mut tex_of: F,
) -> Option<(ModelElement, Vec<String>)>
where
    F: FnMut(&str) -> Option<(u32, String)>,
{
    if e.faces.is_empty() {
        return None;
    }

    let mut mask: u8 = 0;
    // 面必须按面序存放：读取端是按 faceMask 的位序依次取面的
    let mut by_index: Vec<Option<ModelFace>> = vec![None; 6];
    let mut used = Vec::new();

    for (name, f) in &e.faces {
        let Some(idx) = face_index(name) else {
            continue; // 未知面名（例如 mod 自定义）直接忽略
        };
        let Some((tex_idx, path)) = tex_of(&f.texture) else {
            continue;
        };
        // 沿变量链判断是否强制半透明
        let _ = is_force_translucent(&chain.textures, &f.texture);

        let uv = f
            .uv
            .unwrap_or_else(|| default_uv(idx, e.from, e.to));
        by_index[idx as usize] = Some(ModelFace {
            texture: tex_idx,
            uv,
            cullface: match face_index(&f.cullface) {
                Some(c) => c,
                None => 0xFF, // 空串或未知面名 = 不参与剔除
            },
            // tintindex 在文件里是 i16：-1 表示无染色。
            // 越界值直接按 -1 处理而不是截断——截断会让一个合法外的值
            // 变成某个看起来正常的染色下标，反而更难发现。
            tintindex: i16::try_from(f.tintindex).unwrap_or(-1),
            rotation: f.rotation,
        });
        mask |= 1u8 << idx;
        used.push(path);
    }

    if mask == 0 {
        return None; // 所有面都没解析出来
    }

    // 按面序压紧：只保留已置位的面
    let mut faces = Vec::new();
    for slot in by_index.into_iter() {
        if let Some(f) = slot {
            faces.push(f);
        }
    }
    if faces.len() != mask.count_ones() as usize {
        // 理论上到不了这里；到达说明上面的一致性被破坏，宁可跳过
        return None;
    }

    let (has_rotation, rot_kind, origin, axis, angle, rescale, rot_vec) = match &e.rotation {
        Some(r) => {
            let multi = r.rot_vec.is_some();
            (
                true,
                if multi { 1u8 } else { 0u8 },
                r.origin,
                match r.axis.as_str() {
                    "x" => 0u8,
                    "z" => 2,
                    _ => 1, // 缺省 y
                },
                r.angle,
                r.rescale,
                r.rot_vec.unwrap_or([0.0; 3]),
            )
        }
        None => (false, 0u8, [0.0; 3], 0u8, 0.0, false, [0.0; 3]),
    };

    Some((
        ModelElement {
            from: e.from,
            to: e.to,
            has_rotation,
            rot_kind,
            origin,
            axis,
            angle,
            rescale,
            rot_vec,
            face_mask: mask,
            faces,
        },
        used,
    ))
}

/// 把模型链（已合并 textures 与 elements）转成 pack 形态。
///
/// 返回 None 表示没有任何可用元素——这种模型写入后不可见，
/// 不如让调用方按「模型缺失」处理并计入统计。
pub fn convert_model<F>(
    def: &ModelDef,
    chain: &Chain,
    translucent: bool,
    mut tex_of: F,
) -> Option<ConvertedModel>
where
    F: FnMut(&str) -> Option<(u32, String)>,
{
    // elements 以链上最近一级为准；由调用方预先放进 chain
    let elements = if chain.elements.is_empty() {
        &def.elements
    } else {
        &chain.elements
    };
    if elements.is_empty() {
        return None;
    }

    let mut out_elements = Vec::new();
    let mut used = Vec::new();
    for e in elements {
        if let Some((el, mut u)) = convert_element(e, chain, &mut tex_of) {
            out_elements.push(el);
            used.append(&mut u);
        }
    }
    if out_elements.is_empty() {
        return None;
    }

    used.sort();
    used.dedup();
    Some(ConvertedModel {
        model: Model { translucent, elements: out_elements },
        used_textures: used,
    })
}

/// 收集一个模型链里全部面的贴图路径（去重、排序）。
///
/// 存在的理由：模型面里存的是**字符串表下标**，而该下标来自模型自己引用的
/// 路径，形成顺序依赖。因此分两遍：先用本函数收集全部路径并登记字符串池，
/// 再调用 convert_model（此时路径到下标已完整可查），避免边查边写的借用冲突。
pub fn collect_texture_paths(chain: &Chain, elements: &[crate::blockstate::ModelElementDef], cur_ns: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for e in elements {
        for (_, f) in &e.faces {
            if let Some(p) = resolve_face_texture_path(chain, &f.texture, cur_ns) {
                out.push(p);
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// 把面的贴图引用解析成规范路径（只解析，不查表）。
///
/// 与 `resolve_face_texture` 的区别：这里不要求路径已登记到字符串池，
/// 因此调用方可以在拿到路径后再登记，避免「先有表还是先有路径」的循环。
pub fn resolve_face_texture_path(chain: &Chain, raw: &str, cur_ns: &str) -> Option<String> {
    let path = resolve_tex_ref(&chain.textures, raw)?;
    crate::jar_index::qualify(&path, cur_ns)
}

/// 解析面的贴图引用到「字符串池下标 + 规范路径」。
///
/// 返回 None 表示无法解析：变量缺失、成环、或路径未登记。
pub fn resolve_face_texture(
    chain: &Chain,
    raw: &str,
    cur_ns: &str,
    string_pool: &HashMap<String, u32>,
) -> Option<(u32, String)> {
    let path = resolve_tex_ref(&chain.textures, raw)?;
    let qualified = crate::jar_index::qualify(&path, cur_ns)?;
    let idx = string_pool.get(&qualified).copied()?;
    Some((idx, qualified))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blockstate::ModelFace as DefFace;
    use crate::blockstate_chain::TexValue;

    fn def_face(texture: &str, cullface: &str, uv: Option<[f32; 4]>) -> DefFace {
        DefFace {
            texture: texture.to_string(),
            cullface: cullface.to_string(),
            tintindex: -1,
            rotation: 0,
            uv,
        }
    }

    fn def_elem(faces: Vec<(&str, DefFace)>, from: [f32; 3], to: [f32; 3]) -> ModelElementDef {
        ModelElementDef {
            from,
            to,
            faces: faces.into_iter().map(|(n, f)| (n.to_string(), f)).collect(),
            rotation: None,
        }
    }

    fn chain_with(pairs: &[(&str, &str)]) -> Chain {
        let mut textures = HashMap::new();
        for (k, v) in pairs {
            textures.insert(
                k.to_string(),
                TexValue { sprite: v.to_string(), force_translucent: false },
            );
        }
        Chain { textures, elements: Vec::new(), force_translucent: false }
    }

    // ---- 面名与序号 ----

    #[test]
    fn face_names_map_to_indices_in_order() {
        for (i, n) in FACE_NAMES.iter().enumerate() {
            assert_eq!(face_index(n), Some(i as u8), "{n} 的序号应为 {i}");
        }
        assert_eq!(face_index("nope"), None);
        assert_eq!(face_index(""), None);
    }

    // ---- 缺省 UV ----

    #[test]
    fn default_uv_matches_facebakery_for_cube() {
        // 一个 0..16 的整方块，六面的缺省 UV 都应覆盖整个 0..16
        let from = [0.0, 0.0, 0.0];
        let to = [16.0, 16.0, 16.0];
        for face in 0..6u8 {
            let uv = default_uv(face, from, to);
            assert_eq!(uv, [0.0, 0.0, 16.0, 16.0], "面 {face} 的缺省 UV 应为整块");
        }
    }

    #[test]
    fn default_uv_follows_axis_direction_per_face() {
        // 部分元素：可见各面的 u/v 取值方向
        let from = [2.0, 3.0, 5.0];
        let to = [8.0, 9.0, 11.0];

        // down：u 随 +x（2..8），v 随 -z（16-11=5 .. 16-5=11）
        assert_eq!(default_uv(0, from, to), [2.0, 5.0, 8.0, 11.0]);
        // up：u 随 +x，v 随 +z（5..11）
        assert_eq!(default_uv(1, from, to), [2.0, 5.0, 8.0, 11.0]);
        // north：u 随 -x（16-8=8 .. 16-2=14），v 随 -y（16-9=7 .. 16-3=13）
        assert_eq!(default_uv(2, from, to), [8.0, 7.0, 14.0, 13.0]);
        // south：u 随 +x，v 随 -y
        assert_eq!(default_uv(3, from, to), [2.0, 7.0, 8.0, 13.0]);
        // west：u 随 +z，v 随 -y
        assert_eq!(default_uv(4, from, to), [5.0, 7.0, 11.0, 13.0]);
        // east：u 随 -z（16-11=5 .. 16-5=11），v 随 -y
        assert_eq!(default_uv(5, from, to), [5.0, 7.0, 11.0, 13.0]);
    }

    // ---- 元素转换 ----

    #[test]
    fn explicit_uv_is_kept() {
        let e = def_elem(
            vec![("north", def_face("#all", "", Some([1.0, 2.0, 3.0, 4.0])))],
            [0.0; 3],
            [16.0; 3],
        );
        let ch = chain_with(&[("all", "minecraft:block/stone")]);
        let pool = HashMap::from([("minecraft:block/stone".to_string(), 7u32)]);
        let (el, used) = convert_element(&e, &ch, |raw| {
            resolve_face_texture(&ch, raw, "minecraft", &pool)
        })
        .expect("应转换成功");
        assert_eq!(el.faces[0].uv, [1.0, 2.0, 3.0, 4.0], "显式 UV 不应被覆盖");
        assert_eq!(used, vec!["minecraft:block/stone"]);
    }

    #[test]
    fn missing_uv_falls_back_to_default() {
        let e = def_elem(
            vec![("up", def_face("#all", "", None))],
            [1.0, 2.0, 3.0],
            [5.0, 6.0, 7.0],
        );
        let ch = chain_with(&[("all", "minecraft:block/x")]);
        let pool = HashMap::from([("minecraft:block/x".to_string(), 0u32)]);
        let (el, _) = convert_element(&e, &ch, |raw| {
            resolve_face_texture(&ch, raw, "minecraft", &pool)
        })
        .unwrap();
        assert_eq!(el.faces[0].uv, default_uv(1, [1.0, 2.0, 3.0], [5.0, 6.0, 7.0]));
    }

    #[test]
    fn cullface_name_becomes_index_and_empty_is_no_cull() {
        let e = def_elem(
            vec![
                ("north", def_face("#all", "north", None)),
                ("south", def_face("#all", "", None)),
            ],
            [0.0; 3],
            [16.0; 3],
        );
        let ch = chain_with(&[("all", "minecraft:block/x")]);
        let pool = HashMap::from([("minecraft:block/x".to_string(), 0u32)]);
        let (el, _) = convert_element(&e, &ch, |raw| {
            resolve_face_texture(&ch, raw, "minecraft", &pool)
        })
        .unwrap();
        // 面按序存放：north(2) 在前、south(3) 在后
        assert_eq!(el.face_mask, 0b00001100);
        assert_eq!(el.faces.len(), 2);
        assert_eq!(el.faces[0].cullface, 2, "north 应映射为 2");
        assert_eq!(el.faces[1].cullface, 0xFF, "空 cullface 表示不剔除");
    }

    #[test]
    fn faces_are_ordered_by_face_index_not_input_order() {
        // 输入顺序打乱，输出必须仍按面序排列——读取端按 faceMask 的位序取面
        let e = def_elem(
            vec![
                ("east", def_face("#all", "", None)),
                ("down", def_face("#all", "", None)),
                ("up", def_face("#all", "", None)),
            ],
            [0.0; 3],
            [16.0; 3],
        );
        let ch = chain_with(&[("all", "minecraft:block/x")]);
        let pool = HashMap::from([("minecraft:block/x".to_string(), 0u32)]);
        let (el, _) = convert_element(&e, &ch, |raw| {
            resolve_face_texture(&ch, raw, "minecraft", &pool)
        })
        .unwrap();
        // down=0、up=1、east=5 -> mask 位 0、1、5
        assert_eq!(el.face_mask, 0b0010_0011);
        assert_eq!(el.faces.len(), 3, "面数应与置位数一致");
    }

    #[test]
    fn unresolvable_texture_makes_face_skipped() {
        // 引用不存在的变量：该面跳过，其余面保留
        let e = def_elem(
            vec![
                ("north", def_face("#missing", "", None)),
                ("up", def_face("#all", "", None)),
            ],
            [0.0; 3],
            [16.0; 3],
        );
        let ch = chain_with(&[("all", "minecraft:block/x")]);
        let pool = HashMap::from([("minecraft:block/x".to_string(), 0u32)]);
        let (el, used) = convert_element(&e, &ch, |raw| {
            resolve_face_texture(&ch, raw, "minecraft", &pool)
        })
        .unwrap();
        assert_eq!(el.faces.len(), 1, "无法解析的面应被跳过");
        assert_eq!(el.face_mask, 0b000010, "只剩 up 面");
        assert_eq!(used, vec!["minecraft:block/x"]);
    }

    #[test]
    fn element_with_all_faces_unresolvable_is_dropped() {
        let e = def_elem(
            vec![("north", def_face("#missing", "", None))],
            [0.0; 3],
            [16.0; 3],
        );
        let ch = chain_with(&[]);
        let pool: HashMap<String, u32> = HashMap::new();
        assert!(convert_element(&e, &ch, |raw| {
            resolve_face_texture(&ch, raw, "minecraft", &pool)
        })
        .is_none());
    }

    #[test]
    fn unknown_face_names_are_ignored() {
        let e = def_elem(
            vec![
                ("custom_face", def_face("#all", "", None)),
                ("up", def_face("#all", "", None)),
            ],
            [0.0; 3],
            [16.0; 3],
        );
        let ch = chain_with(&[("all", "minecraft:block/x")]);
        let pool = HashMap::from([("minecraft:block/x".to_string(), 0u32)]);
        let (el, _) = convert_element(&e, &ch, |raw| {
            resolve_face_texture(&ch, raw, "minecraft", &pool)
        })
        .unwrap();
        assert_eq!(el.faces.len(), 1);
        assert_eq!(el.face_mask, 0b000010, "只应保留 up");
    }

    #[test]
    fn element_rotation_is_carried_over() {
        let mut e = def_elem(
            vec![("up", def_face("#all", "", None))],
            [0.0; 3],
            [16.0; 3],
        );
        e.rotation = Some(crate::blockstate::ElementRotation {
            origin: [8.0, 8.0, 8.0],
            axis: "x".to_string(),
            angle: 45.0,
            rescale: true,
            rot_vec: None,
        });
        let ch = chain_with(&[("all", "minecraft:block/x")]);
        let pool = HashMap::from([("minecraft:block/x".to_string(), 0u32)]);
        let (el, _) = convert_element(&e, &ch, |raw| {
            resolve_face_texture(&ch, raw, "minecraft", &pool)
        })
        .unwrap();
        assert!(el.has_rotation);
        assert_eq!(el.rot_kind, 0, "单轴旋转 rot_kind 应为 0");
        assert_eq!(el.axis, 0, "axis=x 应映射为 0");
        assert_eq!(el.angle, 45.0);
        assert!(el.rescale);
        assert_eq!(el.origin, [8.0, 8.0, 8.0]);
    }

    #[test]
    fn three_axis_rotation_sets_kind_and_vector() {
        let mut e = def_elem(
            vec![("up", def_face("#all", "", None))],
            [0.0; 3],
            [16.0; 3],
        );
        e.rotation = Some(crate::blockstate::ElementRotation {
            origin: [0.0; 3],
            axis: "y".to_string(),
            angle: 0.0,
            rescale: false,
            rot_vec: Some([90.0, 0.0, 45.0]),
        });
        let ch = chain_with(&[("all", "minecraft:block/x")]);
        let pool = HashMap::from([("minecraft:block/x".to_string(), 0u32)]);
        let (el, _) = convert_element(&e, &ch, |raw| {
            resolve_face_texture(&ch, raw, "minecraft", &pool)
        })
        .unwrap();
        assert_eq!(el.rot_kind, 1, "多轴旋转 rot_kind 应为 1");
        assert_eq!(el.rot_vec, [90.0, 0.0, 45.0]);
    }

    // ---- 模型转换 ----

    #[test]
    fn convert_model_collects_textures_and_dedups() {
        let def = ModelDef {
            parent: String::new(),
            textures: vec![("all".into(), "#base".into())],
            elements: vec![
                def_elem(vec![("up", def_face("#all", "", None))], [0.0; 3], [16.0; 3]),
                def_elem(vec![("down", def_face("#all", "", None))], [0.0; 3], [16.0; 3]),
            ],
            ambientocclusion: true,
        };
        // #all -> #base -> 实际路径
        let ch = chain_with(&[("all", "#base"), ("base", "minecraft:block/x")]);
        let pool = HashMap::from([("minecraft:block/x".to_string(), 3u32)]);
        let cm = convert_model(&def, &ch, false, |raw| {
            resolve_face_texture(&ch, raw, "minecraft", &pool)
        })
        .unwrap();
        assert_eq!(cm.model.elements.len(), 2);
        assert_eq!(
            cm.used_textures,
            vec!["minecraft:block/x"],
            "两个面用同一贴图时应去重为一条"
        );
    }

    #[test]
    fn convert_model_without_elements_is_none() {
        let def = ModelDef {
            parent: "block/cube".into(),
            textures: vec![],
            elements: vec![],
            ambientocclusion: true,
        };
        let ch = chain_with(&[]);
        let pool: HashMap<String, u32> = HashMap::new();
        assert!(
            convert_model(&def, &ch, false, |raw| {
                resolve_face_texture(&ch, raw, "minecraft", &pool)
            })
            .is_none(),
            "没有元素的模型不应产出（写入后不可见）"
        );
    }

    #[test]
    fn chain_elements_take_precedence_over_def() {
        // chain 里有 elements 时以它为准（父链已合并）
        let def = ModelDef {
            parent: String::new(),
            textures: vec![],
            elements: vec![def_elem(
                vec![("up", def_face("#missing", "", None))],
                [0.0; 3],
                [16.0; 3],
            )],
            ambientocclusion: true,
        };
        let mut ch = chain_with(&[("all", "minecraft:block/x")]);
        ch.elements = vec![def_elem(
            vec![("up", def_face("#all", "", None))],
            [0.0; 3],
            [16.0; 3],
        )];
        let pool = HashMap::from([("minecraft:block/x".to_string(), 0u32)]);
        let cm = convert_model(&def, &ch, false, |raw| {
            resolve_face_texture(&ch, raw, "minecraft", &pool)
        })
        .unwrap();
        assert_eq!(cm.model.elements.len(), 1, "应使用 chain 里的 elements");
    }

    #[test]
    fn translucent_flag_is_carried() {
        let def = ModelDef {
            parent: String::new(),
            textures: vec![],
            elements: vec![def_elem(
                vec![("up", def_face("#all", "", None))],
                [0.0; 3],
                [16.0; 3],
            )],
            ambientocclusion: true,
        };
        let ch = chain_with(&[("all", "minecraft:block/x")]);
        let pool = HashMap::from([("minecraft:block/x".to_string(), 0u32)]);
        let cm = convert_model(&def, &ch, true, |raw| {
            resolve_face_texture(&ch, raw, "minecraft", &pool)
        })
        .unwrap();
        assert!(cm.model.translucent);
    }

    // ---- 贴图引用解析 ----

    #[test]
    fn resolve_face_texture_qualifies_and_looks_up_id() {
        let ch = chain_with(&[("all", "block/stone")]);
        let pool = HashMap::from([("minecraft:block/stone".to_string(), 9u32)]);
        let (idx, path) =
            resolve_face_texture(&ch, "#all", "minecraft", &pool).expect("应解析成功");
        assert_eq!(idx, 9);
        assert_eq!(path, "minecraft:block/stone");
    }

    #[test]
    fn resolve_face_texture_keeps_explicit_namespace() {
        let ch = chain_with(&[("all", "mymod:block/x")]);
        let pool = HashMap::from([("mymod:block/x".to_string(), 2u32)]);
        let (idx, path) =
            resolve_face_texture(&ch, "#all", "minecraft", &pool).expect("应解析成功");
        assert_eq!(idx, 2);
        assert_eq!(path, "mymod:block/x", "已有命名空间不应被覆盖");
    }

    #[test]
    fn resolve_face_texture_accepts_direct_path() {
        let ch = chain_with(&[]);
        let pool = HashMap::from([("mymod:block/x".to_string(), 4u32)]);
        let (idx, _) =
            resolve_face_texture(&ch, "mymod:block/x", "mymod", &pool).expect("应解析成功");
        assert_eq!(idx, 4);
    }

    #[test]
    fn unresolvable_or_unregistered_texture_returns_none() {
        let ch = chain_with(&[]);
        let pool: HashMap<String, u32> = HashMap::new();
        // 变量缺失
        assert!(resolve_face_texture(&ch, "#nope", "minecraft", &pool).is_none());
        // 路径未登记到字符串池
        assert!(resolve_face_texture(&ch, "block/x", "minecraft", &pool).is_none());
    }
}
