//! 预览渲染的相机与视角。
//!
//! 对照 legacy-cpp/render.cpp 的 BuildCamera 与 Camera::Project。
//! 这里只做正交投影的坐标变换，不涉及光栅化，便于单独测试。

/// 三维向量
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    pub fn dot(self, o: Self) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    pub fn cross(self, o: Self) -> Self {
        Self::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }

    pub fn length(self) -> f32 {
        self.dot(self).sqrt()
    }

    pub fn normalized(self) -> Self {
        let l = self.length();
        if l <= f32::EPSILON {
            self
        } else {
            Self::new(self.x / l, self.y / l, self.z / l)
        }
    }
}

/// 预设视角
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewPreset {
    Front = 0,
    Left45,
    Right45,
    Back,
}

impl ViewPreset {
    pub fn from_index(i: i32) -> Self {
        match i {
            1 => Self::Left45,
            2 => Self::Right45,
            3 => Self::Back,
            _ => Self::Front,
        }
    }

    /// 该预设对应的 (yaw, pitch)，单位度
    pub fn angles(self) -> (f32, f32) {
        match self {
            Self::Front => (0.0, 12.0),
            Self::Left45 => (-45.0, 30.0),
            Self::Right45 => (45.0, 30.0),
            Self::Back => (180.0, 12.0),
        }
    }

    pub fn id_name(self) -> &'static str {
        match self {
            Self::Front => "front",
            Self::Left45 => "left45",
            Self::Right45 => "right45",
            Self::Back => "back",
        }
    }
}

/// 正交相机：由视线方向与右向量确定的屏幕基
#[derive(Debug, Clone, Copy)]
pub struct Camera {
    /// 视线方向（由相机指向场景）
    pub forward: Vec3,
    /// 屏幕右方向
    pub right: Vec3,
    /// 屏幕上方向
    pub up: Vec3,
}

impl Camera {
    /// 按预设视角与覆盖角度构建相机。
    ///
    /// yaw_override 为 None 时使用预设自带的 yaw；pitch 始终以传入值为准
    /// （与 legacy-cpp 的行为一致：界面上的拖动旋转只覆盖这两个角度）。
    pub fn build(preset: ViewPreset, yaw_override: Option<f32>, pitch_deg: f32) -> Self {
        let (preset_yaw, _) = preset.angles();
        let yaw = yaw_override.unwrap_or(preset_yaw);
        let yr = yaw.to_radians();
        let pr = pitch_deg.to_radians();
        let (sy, cy) = yr.sin_cos();
        let (sp, cp) = pr.sin_cos();

        let forward = Vec3::new(cp * sy, -sp, cp * cy);
        let right = Vec3::new(cy, 0.0, -sy);
        let up = forward.cross(right).normalized();
        Self { forward, right, up }
    }

    /// 把世界坐标投影到屏幕坐标。
    ///
    /// 返回 (屏幕 x, 屏幕 y, 深度)。深度取沿视线方向的投影，
    /// 数值越小越靠近相机，用于深度缓冲。
    pub fn project(&self, p: Vec3) -> (f32, f32, f32) {
        (
            p.dot(self.right),
            -p.dot(self.up),
            p.dot(self.forward),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn basis_is_orthonormal() {
        for preset in [
            ViewPreset::Front,
            ViewPreset::Left45,
            ViewPreset::Right45,
            ViewPreset::Back,
        ] {
            for pitch in [-89.0f32, -30.0, 0.0, 12.0, 30.0, 89.0] {
                let c = Camera::build(preset, None, pitch);
                assert!(approx(c.forward.length(), 1.0), "{preset:?} {pitch}");
                assert!(approx(c.right.length(), 1.0));
                assert!(approx(c.up.length(), 1.0));
                // 三个轴两两正交
                assert!(approx(c.forward.dot(c.right), 0.0), "forward·right 应为 0");
                assert!(approx(c.forward.dot(c.up), 0.0), "forward·up 应为 0");
                assert!(approx(c.right.dot(c.up), 0.0), "right·up 应为 0");
            }
        }
    }

    #[test]
    fn yaw_override_replaces_preset_yaw() {
        let a = Camera::build(ViewPreset::Front, Some(90.0), 0.0);
        let b = Camera::build(ViewPreset::Front, None, 0.0);
        assert!(!approx(a.forward.x, b.forward.x), "覆盖 yaw 后朝向应不同");

        // 90 度 yaw 时，forward 指向 +X
        assert!(approx(a.forward.x, 1.0), "实际 {}", a.forward.x);
        assert!(approx(a.forward.z, 0.0), "实际 {}", a.forward.z);
    }

    #[test]
    fn pitch_controls_vertical_look() {
        // 俯仰为正表示向下看：forward 的 y 分量为 -sin(pitch)
        let down = Camera::build(ViewPreset::Front, Some(0.0), 30.0);
        assert!(approx(down.forward.y, -0.5), "30 度俯视时 y 应为 -0.5");

        let up = Camera::build(ViewPreset::Front, Some(0.0), -30.0);
        assert!(approx(up.forward.y, 0.5), "仰视时 y 应为 +0.5");

        let level = Camera::build(ViewPreset::Front, Some(0.0), 0.0);
        assert!(approx(level.forward.y, 0.0), "水平时 y 应为 0");
        assert!(approx(level.forward.z, 1.0), "yaw 为 0 时朝向 +Z");
    }

    #[test]
    fn projection_maps_axes_to_screen() {
        // 水平正视：世界 +X 落在屏幕右侧，世界 +Y 落在屏幕上方（屏幕 y 向下为正）
        let c = Camera::build(ViewPreset::Front, Some(0.0), 0.0);
        let (x, y, _) = c.project(Vec3::new(1.0, 0.0, 0.0));
        assert!(approx(x, 1.0), "世界 +X 应映射到屏幕 +x，实际 {x}");
        assert!(approx(y, 0.0));

        let (x, y, _) = c.project(Vec3::new(0.0, 1.0, 0.0));
        assert!(approx(x, 0.0));
        assert!(approx(y, -1.0), "世界 +Y 应映射到屏幕 -y（向上），实际 {y}");
    }

    #[test]
    fn depth_increases_away_from_camera() {
        let c = Camera::build(ViewPreset::Front, Some(0.0), 0.0);
        // yaw=0 时朝向 +Z，因此 z 越大深度越大
        let (_, _, near) = c.project(Vec3::new(0.0, 0.0, 1.0));
        let (_, _, far) = c.project(Vec3::new(0.0, 0.0, 5.0));
        assert!(far > near, "深度应随距离增大：near={near} far={far}");
    }

    #[test]
    fn preset_angles_are_stable() {
        assert_eq!(ViewPreset::from_index(0), ViewPreset::Front);
        assert_eq!(ViewPreset::from_index(1), ViewPreset::Left45);
        assert_eq!(ViewPreset::from_index(2), ViewPreset::Right45);
        assert_eq!(ViewPreset::from_index(3), ViewPreset::Back);
        // 越界索引回退到正面，而不是 panic
        assert_eq!(ViewPreset::from_index(99), ViewPreset::Front);
        assert_eq!(ViewPreset::from_index(-1), ViewPreset::Front);
    }

    #[test]
    fn vector_helpers() {
        let a = Vec3::new(1.0, 0.0, 0.0);
        let b = Vec3::new(0.0, 1.0, 0.0);
        assert!(approx(a.dot(b), 0.0));
        let c = a.cross(b);
        assert!(approx(c.z, 1.0));
        assert!(approx(Vec3::new(3.0, 4.0, 0.0).length(), 5.0));
        assert!(approx(Vec3::new(0.0, 5.0, 0.0).normalized().y, 1.0));
        // 零向量归一化不应产生 NaN
        assert!(Vec3::new(0.0, 0.0, 0.0).normalized().length().is_finite());
    }
}
