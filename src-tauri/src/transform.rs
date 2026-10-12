//! 模型坐标变换：元素旋转、变体旋转与平移到方块世界坐标。
//!
//! 对照 legacy-cpp/render.cpp 的 RotateElement 与 RotateBlock。
//! 顺序固定为：元素自身旋转 -> 变体旋转 -> 按方块坐标平移（并除以 16）。
//! 顺序错了会让旋转后的模型错位。

use crate::blockdb::ModelElement;

/// 绕某轴旋转一个点（角度为度，右手系）。
fn rotate_axis(p: [f32; 3], axis: u8, deg: f32) -> [f32; 3] {
    let a = deg.to_radians();
    let (s, c) = a.sin_cos();
    let [x, y, z] = p;
    match axis {
        0 => [x, y * c - z * s, y * s + z * c],
        1 => [x * c + z * s, y, -x * s + z * c],
        _ => [x * c - y * s, x * s + y * c, z],
    }
}

/// 应用元素自身的旋转（绕 origin）。
///
/// `rot_kind` 为 0 时绕单一轴旋转 `angle` 度；
/// 非 0 时依次绕 X、Y、Z 旋转 `rot_vec` 中的三个角度。
pub fn rotate_element(p: [f32; 3], e: &ModelElement) -> [f32; 3] {
    if !e.has_rotation {
        return p;
    }
    let o = e.origin;
    let d = [p[0] - o[0], p[1] - o[1], p[2] - o[2]];
    let r = if e.rot_kind == 0 {
        rotate_axis(d, e.axis, e.angle)
    } else {
        let a = rotate_axis(d, 0, e.rot_vec[0]);
        let b = rotate_axis(a, 1, e.rot_vec[1]);
        rotate_axis(b, 2, e.rot_vec[2])
    };
    [r[0] + o[0], r[1] + o[1], r[2] + o[2]]
}

/// 应用方块变体的旋转。
///
/// 参数为 90 度的倍数（0~3），先绕 X 再绕 Y。
/// 旋转以方块中心为参照，坐标范围为 0..16，因此用 16 取补。
pub fn rotate_variant(p: [f32; 3], rot_x_div90: u8, rot_y_div90: u8) -> [f32; 3] {
    let [mut x, mut y, mut z] = p;
    match rot_x_div90 % 4 {
        1 => {
            let (ny, nz) = (16.0 - z, y);
            y = ny;
            z = nz;
        }
        2 => {
            y = 16.0 - y;
            z = 16.0 - z;
        }
        3 => {
            let (ny, nz) = (z, 16.0 - y);
            y = ny;
            z = nz;
        }
        _ => {}
    }
    match rot_y_div90 % 4 {
        1 => {
            let (nx, nz) = (16.0 - z, x);
            x = nx;
            z = nz;
        }
        2 => {
            x = 16.0 - x;
            z = 16.0 - z;
        }
        3 => {
            let (nx, nz) = (z, 16.0 - x);
            x = nx;
            z = nz;
        }
        _ => {}
    }
    [x, y, z]
}

/// 把一个模型空间的角点变换到方块世界坐标。
///
/// 顺序：元素自身旋转 -> 变体旋转 -> 除以 16 并加上方块坐标。
pub fn to_world(
    corner: [f32; 3],
    elem: &ModelElement,
    rot_x_div90: u8,
    rot_y_div90: u8,
    block_pos: [i32; 3],
) -> [f32; 3] {
    let p = rotate_element(corner, elem);
    let p = rotate_variant(p, rot_x_div90, rot_y_div90);
    [
        block_pos[0] as f32 + p[0] / 16.0,
        block_pos[1] as f32 + p[1] / 16.0,
        block_pos[2] as f32 + p[2] / 16.0,
    ]
}

/// 把邻面剔除的方向按变体旋转一并旋转。
///
/// 方块被旋转后，它朝向某个邻居的「那一面」也跟着变了。
/// 若仍按未旋转的方向去找邻居，就会剔除错误的方块——
/// 表现为旋转过的楼梯、栅栏等周围出现空洞或缺面。
///
/// 做法与参考实现一致：把方向向量当作「自方块中心偏移的 16 分之一点」，
/// 旋转后再换算回方向，最后四舍五入回整数偏移。
pub fn rotate_cull_offset(off: [i32; 3], rot_x_div90: u8, rot_y_div90: u8) -> [i32; 3] {
    if variant_rotation_is_identity(rot_x_div90, rot_y_div90) {
        return off;
    }
    let p = [
        off[0] as f32 * 16.0 + 8.0,
        off[1] as f32 * 16.0 + 8.0,
        off[2] as f32 * 16.0 + 8.0,
    ];
    let r = rotate_variant(p, rot_x_div90, rot_y_div90);
    [
        ((r[0] - 8.0) / 16.0).round() as i32,
        ((r[1] - 8.0) / 16.0).round() as i32,
        ((r[2] - 8.0) / 16.0).round() as i32,
    ]
}

/// 变体的旋转是否为单位变换。用于跳过无谓计算。
pub fn variant_rotation_is_identity(rot_x_div90: u8, rot_y_div90: u8) -> bool {
    rot_x_div90 % 4 == 0 && rot_y_div90 % 4 == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn elem(has_rotation: bool, rot_kind: u8, axis: u8, angle: f32, origin: [f32; 3]) -> ModelElement {
        ModelElement {
            from: [0.0, 0.0, 0.0],
            to: [16.0, 16.0, 16.0],
            has_rotation,
            rot_kind,
            origin,
            axis,
            angle,
            rescale: false,
            rot_vec: [0.0; 3],
            face_mask: 0x3F,
            faces: Vec::new(),
        }
    }

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    fn approx3(a: [f32; 3], b: [f32; 3]) -> bool {
        approx(a[0], b[0]) && approx(a[1], b[1]) && approx(a[2], b[2])
    }

    #[test]
    fn no_element_rotation_returns_input() {
        let e = elem(false, 0, 1, 90.0, [8.0, 8.0, 8.0]);
        assert_eq!(rotate_element([1.0, 2.0, 3.0], &e), [1.0, 2.0, 3.0]);
    }

    #[test]
    fn single_axis_rotation_is_around_origin() {
        // 绕 Y 轴 90 度，原点在中心：点 (12,8,8) -> (8,8,4)
        let e = elem(true, 0, 1, 90.0, [8.0, 8.0, 8.0]);
        assert!(approx3(rotate_element([12.0, 8.0, 8.0], &e), [8.0, 8.0, 4.0]));

        // 原点处的点不受影响
        assert!(approx3(rotate_element([8.0, 8.0, 8.0], &e), [8.0, 8.0, 8.0]));

        // 360 度应回到原位
        let e360 = elem(true, 0, 1, 360.0, [8.0, 8.0, 8.0]);
        assert!(approx3(rotate_element([12.0, 8.0, 8.0], &e360), [12.0, 8.0, 8.0]));
    }

    #[test]
    fn three_axis_rotation_uses_rot_vec() {
        let mut e = elem(true, 1, 0, 0.0, [0.0, 0.0, 0.0]);
        e.rot_vec = [90.0, 0.0, 0.0];
        // 绕 X 90 度：y->z 方向，(0,1,0) -> (0,0,1)
        assert!(approx3(rotate_element([0.0, 1.0, 0.0], &e), [0.0, 0.0, 1.0]));
    }

    #[test]
    fn variant_rotation_matches_model_spec() {
        // 绕 X 90 度：(x, y, z) -> (x, 16-z, y)
        assert!(approx3(rotate_variant([1.0, 2.0, 3.0], 1, 0), [1.0, 13.0, 2.0]));
        // 绕 X 180 度
        assert!(approx3(rotate_variant([1.0, 2.0, 3.0], 2, 0), [1.0, 14.0, 13.0]));
        // 绕 X 270 度
        assert!(approx3(rotate_variant([1.0, 2.0, 3.0], 3, 0), [1.0, 3.0, 14.0]));
        // 绕 Y 90 度：(x, y, z) -> (16-z, y, x)
        assert!(approx3(rotate_variant([1.0, 2.0, 3.0], 0, 1), [13.0, 2.0, 1.0]));
        // 绕 Y 180 度
        assert!(approx3(rotate_variant([1.0, 2.0, 3.0], 0, 2), [15.0, 2.0, 13.0]));
        // 绕 Y 270 度
        assert!(approx3(rotate_variant([1.0, 2.0, 3.0], 0, 3), [3.0, 2.0, 15.0]));
    }

    #[test]
    fn variant_rotation_zero_is_identity() {
        assert!(approx3(rotate_variant([1.0, 2.0, 3.0], 0, 0), [1.0, 2.0, 3.0]));
        assert!(approx3(rotate_variant([1.0, 2.0, 3.0], 4, 8), [1.0, 2.0, 3.0]));
        assert!(variant_rotation_is_identity(0, 0));
        assert!(variant_rotation_is_identity(4, 4));
        assert!(!variant_rotation_is_identity(1, 0));
        assert!(!variant_rotation_is_identity(0, 2));
    }

    #[test]
    fn four_rotations_return_to_origin() {
        // 同一轴连续转四次应回到原位
        let mut p = [3.0, 5.0, 7.0];
        for _ in 0..4 {
            p = rotate_variant(p, 1, 0);
        }
        assert!(approx3(p, [3.0, 5.0, 7.0]));

        let mut q = [3.0, 5.0, 7.0];
        for _ in 0..4 {
            q = rotate_variant(q, 0, 3);
        }
        assert!(approx3(q, [3.0, 5.0, 7.0]));
    }

    #[test]
    fn to_world_applies_order_and_translation() {
        let e = elem(false, 0, 0, 0.0, [0.0; 3]);
        // 无旋转时：除以 16 后加上方块坐标
        assert!(approx3(
            to_world([16.0, 0.0, 0.0], &e, 0, 0, [2, 3, 4]),
            [3.0, 3.0, 4.0]
        ));
        // 带变体旋转时，先旋转再平移
        let got = to_world([1.0, 2.0, 3.0], &e, 1, 0, [0, 0, 0]);
        let want = [1.0 / 16.0, 13.0 / 16.0, 2.0 / 16.0];
        assert!(approx3(got, want));
    }

    #[test]
    fn cull_offset_rotates_with_variant() {
        // 无旋转时原样返回
        assert_eq!(rotate_cull_offset([1, 0, 0], 0, 0), [1, 0, 0]);
        // 绕 Y 90 度：+X 方向应变为 -Z 方向
        assert_eq!(rotate_cull_offset([1, 0, 0], 0, 1), [0, 0, -1]);
        // 绕 X 90 度：+Y 方向应变为 +Z 方向
        assert_eq!(rotate_cull_offset([0, 1, 0], 1, 0), [0, 0, 1]);
        // 绕 Y 180 度：+X 变为 -X
        assert_eq!(rotate_cull_offset([1, 0, 0], 0, 2), [-1, 0, 0]);
        // 结果始终是单位偏移（不出现 0 以外的分量组合错误）
        for rx in 0u8..4 {
            for ry in 0u8..4 {
                for off in [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]] {
                    let r = rotate_cull_offset(off, rx, ry);
                    let mag: i32 = r.iter().map(|v| v.abs()).sum();
                    assert_eq!(mag, 1, "rx={rx} ry={ry} off={off:?} -> {r:?} 不是单位偏移");
                }
            }
        }
    }

    #[test]
    fn element_rotation_precedes_variant_rotation() {
        // 两种变换不可交换，顺序必须为「先元素后变体」：
        // 先用元素旋转把点移动到 (0,0,1)，再做变体绕 X 90 度得 (0,15,0)
        let mut e = elem(true, 1, 0, 0.0, [0.0; 3]);
        e.rot_vec = [90.0, 0.0, 0.0];
        let got = to_world([0.0, 1.0, 0.0], &e, 1, 0, [0, 0, 0]);
        assert!(approx3(got, [0.0, 15.0 / 16.0, 0.0]), "实际 {got:?}");

        // 若顺序反了会得到不同结果，这里断言两者确实不同
        let wrong = rotate_element(rotate_variant([0.0, 1.0, 0.0], 1, 0), &e);
        assert!(!approx3(wrong, [0.0, 15.0, 0.0]), "顺序反了应得到不同结果");
    }
}
