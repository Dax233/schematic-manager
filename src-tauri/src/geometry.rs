//! 面的几何：角点生成。
//!
//! 对照 legacy-cpp/render.cpp 的 BuildFaceCorners。元素坐标为模型空间 0..16，
//! 四个角点的顺序与 UV 的对应关系必须与 Minecraft 的模型规范一致，
//! 否则贴图会旋转或镜像。
//!
//! 法线计算不在这里：相机与光照由前端的 three.js 负责，
//! 后端只给出明暗系数（FACE_SHADE）。

use crate::blockdb::Face;

/// 一个面的四个角点及其 UV
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceCorners {
    pub pos: [[f32; 3]; 4],
    pub uv: [[f32; 2]; 4],
}

/// 按面朝向生成角点。
///
/// `from` / `to` 为元素的两个对角（模型空间 0..16），
/// `uv` 为该面的 [u1, v1, u2, v2]。
///
/// 各面的 u/v 增长方向由 Minecraft 模型规范固定，不能随意调换：
///   Down  u:+x v:-z     Up    u:+x v:+z
///   North u:-x v:-y     South u:+x v:-y
///   West  u:+z v:-y     East  u:-z v:-y
/// 按面的 rotation 字段旋转 UV。
///
/// 角度以顺时针 90 度为单位，取值 0 / 90 / 180 / 270。
/// 与 legacy-cpp/render.cpp 的贴图旋转一致：先把 UV 归一化到 0..1，
/// 旋转后再还原，因此这里输入输出都保持 0..16 的原始尺度。
pub fn rotate_uv(uv: [f32; 4], rotation: u16) -> [f32; 4] {
    let [u1, v1, u2, v2] = uv;
    // 归一化到 0..1
    let mut p = [[u1 / 16.0, v1 / 16.0], [u2 / 16.0, v2 / 16.0]];
    for q in p.iter_mut() {
        let (fu, fv) = (q[0], q[1]);
        *q = match rotation {
            90 => [fv, 1.0 - fu],
            180 => [1.0 - fu, 1.0 - fv],
            270 => [1.0 - fv, fu],
            _ => [fu, fv],
        };
    }
    [p[0][0] * 16.0, p[0][1] * 16.0, p[1][0] * 16.0, p[1][1] * 16.0]
}

pub fn build_face_corners(face: Face, from: [f32; 3], to: [f32; 3], uv: [f32; 4]) -> FaceCorners {
    let [x1, y1, z1] = from;
    let [x2, y2, z2] = to;
    let [u1, v1, u2, v2] = uv;

    let (pos, uvs) = match face {
        // u 随 +x，v 随 -z
        Face::Down => (
            [[x1, y1, z2], [x2, y1, z2], [x2, y1, z1], [x1, y1, z1]],
            [[u1, v1], [u2, v1], [u2, v2], [u1, v2]],
        ),
        // u 随 +x，v 随 +z
        Face::Up => (
            [[x1, y2, z1], [x2, y2, z1], [x2, y2, z2], [x1, y2, z2]],
            [[u1, v1], [u2, v1], [u2, v2], [u1, v2]],
        ),
        // u 随 -x，v 随 -y
        Face::North => (
            [[x2, y2, z1], [x1, y2, z1], [x1, y1, z1], [x2, y1, z1]],
            [[u1, v1], [u2, v1], [u2, v2], [u1, v2]],
        ),
        // u 随 +x，v 随 -y
        Face::South => (
            [[x1, y2, z2], [x2, y2, z2], [x2, y1, z2], [x1, y1, z2]],
            [[u1, v1], [u2, v1], [u2, v2], [u1, v2]],
        ),
        // u 随 +z，v 随 -y
        Face::West => (
            [[x1, y2, z1], [x1, y2, z2], [x1, y1, z2], [x1, y1, z1]],
            [[u1, v1], [u2, v1], [u2, v2], [u1, v2]],
        ),
        // u 随 -z，v 随 -y
        Face::East => (
            [[x2, y2, z2], [x2, y2, z1], [x2, y1, z1], [x2, y1, z2]],
            [[u1, v1], [u2, v1], [u2, v2], [u1, v2]],
        ),
    };

    FaceCorners { pos, uv: uvs }
}

/// 邻面剔除时，由 cullface 推出的邻居方块偏移
pub fn cull_neighbor_offset(face: Face) -> [i32; 3] {
    match face {
        Face::Down => [0, -1, 0],
        Face::Up => [0, 1, 0],
        Face::North => [0, 0, -1],
        Face::South => [0, 0, 1],
        Face::West => [-1, 0, 0],
        Face::East => [1, 0, 0],
    }
}

/// 各面的明暗系数。数值来自 legacy-cpp 的 FaceShade，
/// 用于在无光照计算的情况下表现立体感；UP 最亮、DOWN 最暗。
pub const FACE_SHADE: [f32; 6] = [
    0.55, // Down
    1.00, // Up
    0.82, // North
    0.72, // South
    0.88, // West
    0.68, // East
];

pub fn face_shade(face: Face) -> f32 {
    FACE_SHADE[face as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    const FROM: [f32; 3] = [1.0, 2.0, 3.0];
    const TO: [f32; 3] = [5.0, 6.0, 7.0];
    const UV: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

    fn corners(f: Face) -> FaceCorners {
        build_face_corners(f, FROM, TO, UV)
    }

    #[test]
    fn corners_lie_on_the_face_plane() {
        // 每个面的四个角点必须落在该面所在的那个平面上
        for (f, axis, want) in [
            (Face::Down, 1, FROM[1]),
            (Face::Up, 1, TO[1]),
            (Face::North, 2, FROM[2]),
            (Face::South, 2, TO[2]),
            (Face::West, 0, FROM[0]),
            (Face::East, 0, TO[0]),
        ] {
            for c in corners(f).pos {
                assert!(
                    (c[axis] - want).abs() < 1e-5,
                    "{f:?} 的角点 {c:?} 不在平面上（轴 {axis} 应为 {want}）"
                );
            }
        }
    }

    #[test]
    fn corners_span_the_element_bounds() {
        // 角点应恰好覆盖元素范围。注意：面自身所在的那个轴是「平的」，
        // 四个角点取同一值（即该面），不会跨 FROM..TO；只有另外两个轴
        // 才跨越整个元素范围。
        for (f, plane_axis) in [
            (Face::Down, 1usize),
            (Face::Up, 1),
            (Face::North, 2),
            (Face::South, 2),
            (Face::West, 0),
            (Face::East, 0),
        ] {
            let c = corners(f);
            for axis in 0..3 {
                let vals: Vec<f32> = c.pos.iter().map(|p| p[axis]).collect();
                let lo = vals.iter().cloned().fold(f32::INFINITY, f32::min);
                let hi = vals.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
                if axis == plane_axis {
                    assert!((hi - lo).abs() < 1e-5, "{f:?} 轴 {axis} 应为平面（四角同值）");
                } else {
                    assert!((lo - FROM[axis]).abs() < 1e-5, "{f:?} 轴 {axis} 下界");
                    assert!((hi - TO[axis]).abs() < 1e-5, "{f:?} 轴 {axis} 上界");
                }
            }
        }
    }

    #[test]
    fn uv_corners_are_assigned_in_order() {
        // 四个角点依次对应 (u1,v1) (u2,v1) (u2,v2) (u1,v2)
        let uv = [0.1f32, 0.2, 0.8, 0.9];
        for f in [Face::Down, Face::Up, Face::North, Face::South, Face::West, Face::East] {
            let c = build_face_corners(f, FROM, TO, uv);
            assert_eq!(c.uv[0], [0.1, 0.2], "{f:?}");
            assert_eq!(c.uv[1], [0.8, 0.2], "{f:?}");
            assert_eq!(c.uv[2], [0.8, 0.9], "{f:?}");
            assert_eq!(c.uv[3], [0.1, 0.9], "{f:?}");
        }
    }

    #[test]
    fn uv_u_direction_matches_model_spec() {
        // 对每个面，取 uv[0] 与 uv[1] 对应的两个角点，
        // 它们的空间增量方向应符合该面的 u 增长方向
        let uv = [0.0f32, 0.0, 1.0, 1.0];
        for (f, axis, sign) in [
            (Face::Down, 0, 1.0),   // u 随 +x
            (Face::Up, 0, 1.0),     // u 随 +x
            (Face::North, 0, -1.0), // u 随 -x
            (Face::South, 0, 1.0),  // u 随 +x
            (Face::West, 2, 1.0),   // u 随 +z
            (Face::East, 2, -1.0),  // u 随 -z
        ] {
            let c = build_face_corners(f, FROM, TO, uv);
            let delta = c.pos[1][axis] - c.pos[0][axis];
            assert!(
                delta * sign > 0.0,
                "{f:?} 的 u 方向不符：轴 {axis} 增量 {delta}，期望符号 {sign}"
            );
        }
    }

    #[test]
    fn degenerate_element_does_not_panic() {
        // 零体积元素（from == to）在真实模型中存在，不应 panic 或产生 NaN
        for f in [Face::Down, Face::Up, Face::North, Face::South, Face::West, Face::East] {
            let c = build_face_corners(f, [2.0; 3], [2.0; 3], UV);
            for p in c.pos {
                assert!(p.iter().all(|v| v.is_finite()));
            }
        }
    }

    #[test]
    fn cull_offsets_point_outward() {
        // 剔除偏移必须指向该面朝外的邻居方块；写死期望值而不是拿另一份
        // 实现对照——否则两份实现同时写错时会互相印证成「通过」。
        // 坐标约定：X 向东为正，Y 向上为正，Z 向南为正（与 Minecraft 一致）。
        let cases = [
            (Face::Down, [0, -1, 0]),
            (Face::Up, [0, 1, 0]),
            (Face::North, [0, 0, -1]),
            (Face::South, [0, 0, 1]),
            (Face::West, [-1, 0, 0]),
            (Face::East, [1, 0, 0]),
        ];
        for (f, want) in cases {
            assert_eq!(cull_neighbor_offset(f), want, "{f:?} 的剔除偏移不正确");
        }
    }

    #[test]
    fn shade_is_within_unit_range_per_axis() {
        // 明暗系数只需落在 (0,1] 且顶面比底面亮——不锁定具体数值，
        // 这些系数会随观感调整
        for f in [Face::Down, Face::Up, Face::North, Face::South, Face::West, Face::East] {
            let s = face_shade(f);
            assert!(s > 0.0 && s <= 1.0, "{f:?} 明暗系数越界：{s}");
        }
        assert!(
            face_shade(Face::Up) > face_shade(Face::Down),
            "顶面应比底面亮，否则立体感方向相反"
        );
    }
}
