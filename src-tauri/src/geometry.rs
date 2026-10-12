//! 面的几何：角点生成与法线。
//!
//! 对照 legacy-cpp/render.cpp 的 BuildFaceCorners 与 FaceNormalModel。
//! 元素坐标为模型空间 0..16，四个角点的顺序与 UV 的对应关系
//! 必须与 Minecraft 的模型规范一致，否则贴图会旋转或镜像。

use crate::blockdb::Face;
use crate::camera::Vec3;

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

/// 面在模型空间的朝外法线
pub fn face_normal(face: Face) -> Vec3 {
    match face {
        Face::Down => Vec3::new(0.0, -1.0, 0.0),
        Face::Up => Vec3::new(0.0, 1.0, 0.0),
        Face::North => Vec3::new(0.0, 0.0, -1.0),
        Face::South => Vec3::new(0.0, 0.0, 1.0),
        Face::West => Vec3::new(-1.0, 0.0, 0.0),
        Face::East => Vec3::new(1.0, 0.0, 0.0),
    }
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
    0.85, // Up
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
        // 角点应恰好覆盖元素在另外两个轴上的范围，不多不少
        for f in [Face::Down, Face::Up, Face::North, Face::South, Face::West, Face::East] {
            let c = corners(f);
            for axis in 0..3 {
                let vals: Vec<f32> = c.pos.iter().map(|p| p[axis]).collect();
                let lo = vals.iter().cloned().fold(f32::INFINITY, f32::min);
                let hi = vals.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
                assert!((lo - FROM[axis]).abs() < 1e-5, "{f:?} 轴 {axis} 下界");
                assert!((hi - TO[axis]).abs() < 1e-5, "{f:?} 轴 {axis} 上界");
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
    fn normals_and_cull_offsets_point_outward() {
        // 法线与剔除偏移必须同向，否则会把该剔除的面留下、该留的剔除
        for f in [Face::Down, Face::Up, Face::North, Face::South, Face::West, Face::East] {
            let n = face_normal(f);
            let o = cull_neighbor_offset(f);
            let same = (n.x != 0.0 && n.x as i32 == o[0])
                || (n.y != 0.0 && n.y as i32 == o[1])
                || (n.z != 0.0 && n.z as i32 == o[2]);
            assert!(same, "{f:?} 的法线 {n:?} 与剔除偏移 {o:?} 不同向");
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
