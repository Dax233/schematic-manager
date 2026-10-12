//! 三角形光栅化：重心坐标、深度缓冲与透明混合。
//!
//! 对照 legacy-cpp/render.cpp 中的 drawTri 拉姆达。
//! 颜色为 ARGB8888，深度越小越靠近相机。

/// 输出图像，ARGB8888，行优先
#[derive(Debug, Clone)]
pub struct Image {
    pub w: i32,
    pub h: i32,
    pub px: Vec<u32>,
}

impl Image {
    pub fn new(w: i32, h: i32, fill: u32) -> Self {
        let n = (w.max(0) as usize) * (h.max(0) as usize);
        Self { w, h, px: vec![fill; n] }
    }

    pub fn inside(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < self.w && y < self.h
    }

    pub fn at(&self, x: i32, y: i32) -> u32 {
        self.px[(y as usize) * (self.w as usize) + (x as usize)]
    }

    pub fn set(&mut self, x: i32, y: i32, c: u32) {
        let i = (y as usize) * (self.w as usize) + (x as usize);
        self.px[i] = c;
    }
}

/// 屏幕空间顶点。z 为深度，u/v 为贴图坐标。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub u: f32,
    pub v: f32,
}

impl Vertex {
    pub fn new(x: f32, y: f32, z: f32, u: f32, v: f32) -> Self {
        Self { x, y, z, u, v }
    }
}

/// 按系数压暗颜色（只作用于 RGB，保留 alpha）。
pub fn shade_color(c: u32, shade: f32) -> u32 {
    let a = (c >> 24) & 0xFF;
    let r = (c >> 16) & 0xFF;
    let g = (c >> 8) & 0xFF;
    let b = c & 0xFF;
    let f = |v: u32| -> u32 {
        let x = (v as f32 * shade).round();
        x.clamp(0.0, 255.0) as u32
    };
    (a << 24) | (f(r) << 16) | (f(g) << 8) | f(b)
}

/// 缺失贴图的占位图案：紫黑棋盘格（与 Minecraft 一致）。
pub fn missing_texture_color(x: i32, y: i32) -> u32 {
    if ((x >> 1) + (y >> 1)) & 1 != 0 {
        0xFFFF00FF
    } else {
        0xFF000000
    }
}

/// 把一个三角形光栅化到图像上。
///
/// `sample` 由调用方提供贴图采样：给定贴图坐标返回 ARGB 颜色，
/// 返回 None 表示该处应跳过（例如超出贴图范围）。这样栅格化本身
/// 不依赖贴图资源，便于单独测试。
///
/// 半透明三角形不写深度，避免遮挡其后方的几何。
pub fn draw_tri<F>(
    img: &mut Image,
    zbuf: &mut [f32],
    v0: Vertex,
    v1: Vertex,
    v2: Vertex,
    sample: F,
    shade: f32,
    transparent: bool,
) where
    F: Fn(f32, f32) -> Option<u32>,
{
    let area = (v1.x - v0.x) * (v2.y - v0.y) - (v2.x - v0.x) * (v1.y - v0.y);
    // 退化三角形（面积为 0）直接丢弃，否则后续会除以 0
    if area.abs() < 1e-9 {
        return;
    }
    let inv = 1.0 / area;

    // 包围盒，并夹到图像范围内
    let mut x0 = v0.x.min(v1.x).min(v2.x).floor() as i32;
    let mut x1 = v0.x.max(v1.x).max(v2.x).ceil() as i32;
    let mut y0 = v0.y.min(v1.y).min(v2.y).floor() as i32;
    let mut y1 = v0.y.max(v1.y).max(v2.y).ceil() as i32;
    x0 = x0.max(0);
    y0 = y0.max(0);
    x1 = x1.min(img.w - 1);
    y1 = y1.min(img.h - 1);
    if x0 > x1 || y0 > y1 {
        return;
    }

    for y in y0..=y1 {
        for x in x0..=x1 {
            // 以像素中心采样
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;

            let w0 = ((v1.x - v0.x) * (py - v0.y) - (px - v0.x) * (v1.y - v0.y)) * inv;
            let w1 = ((px - v0.x) * (v2.y - v0.y) - (v2.x - v0.x) * (py - v0.y)) * inv;
            let w2 = 1.0 - w0 - w1;
            // 三个重心坐标均为非负才在三角形内
            if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                continue;
            }

            let z = w2 * v0.z + w1 * v1.z + w0 * v2.z;
            let zi = (y as usize) * (img.w as usize) + (x as usize);
            if zi >= zbuf.len() {
                continue;
            }
            if z > zbuf[zi] {
                continue; // 被更近的几何挡住
            }

            let u = w2 * v0.u + w1 * v1.u + w0 * v2.u;
            let v = w2 * v0.v + w1 * v1.v + w0 * v2.v;
            let Some(c) = sample(u, v) else { continue };
            let sa = (c >> 24) & 0xFF;
            if sa == 0 {
                continue; // 完全透明，不产生像素
            }

            let c = shade_color(c, shade);

            if !transparent {
                zbuf[zi] = z;
                img.set(x, y, c);
            } else {
                let dst = img.at(x, y);
                let da = (dst >> 24) & 0xFF;
                // 源覆盖目标：na = sa + da*(1-sa)
                let na = sa + da * (255 - sa) / 255;
                let mix = |sh: u32| -> u32 {
                    let s = (c >> sh) & 0xFF;
                    let d = (dst >> sh) & 0xFF;
                    let num = s * sa + d * da * (255 - sa) / 255;
                    num / na.max(1)
                };
                img.set(x, y, (na << 24) | (mix(16) << 16) | (mix(8) << 8) | mix(0));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 恒返回固定颜色的采样器
    fn solid(c: u32) -> impl Fn(f32, f32) -> Option<u32> {
        move |_, _| Some(c)
    }

    fn opaque_white() -> u32 {
        0xFFFFFFFF
    }

    #[test]
    fn covers_expected_pixels_only() {
        let mut img = Image::new(10, 10, 0);
        let mut zbuf = vec![f32::MAX; 100];
        // 覆盖像素 (2..=6, 2..=6) 的直角三角形
        draw_tri(
            &mut img,
            &mut zbuf,
            Vertex::new(2.0, 2.0, 1.0, 0.0, 0.0),
            Vertex::new(7.0, 2.0, 1.0, 1.0, 0.0),
            Vertex::new(2.0, 7.0, 1.0, 0.0, 1.0),
            solid(opaque_white()),
            1.0,
            false,
        );
        // 右上角在三角形外
        assert_eq!(img.at(9, 9), 0, "三角形外的像素不应被写入");
        assert_eq!(img.at(0, 0), 0);
        // 三角形内部应被填充
        assert_eq!(img.at(3, 3), opaque_white());
        assert_eq!(img.at(2, 2), opaque_white());
    }

    #[test]
    fn depth_test_keeps_nearer_geometry() {
        let mut img = Image::new(8, 8, 0);
        let mut zbuf = vec![f32::MAX; 64];
        let tri = |z: f32| {
            (
                Vertex::new(0.0, 0.0, z, 0.0, 0.0),
                Vertex::new(8.0, 0.0, z, 1.0, 0.0),
                Vertex::new(0.0, 8.0, z, 0.0, 1.0),
            )
        };
        // 先画远处（红色）
        let (a, b, c) = tri(5.0);
        draw_tri(&mut img, &mut zbuf, a, b, c, solid(0xFFFF0000), 1.0, false);
        // 再画近处（绿色），应覆盖
        let (a, b, c) = tri(1.0);
        draw_tri(&mut img, &mut zbuf, a, b, c, solid(0xFF00FF00), 1.0, false);
        assert_eq!(img.at(1, 1), 0xFF00FF00, "更近的几何应覆盖更远的");

        // 反向：先近后远，远的应被深度测试拒绝
        let mut img2 = Image::new(8, 8, 0);
        let mut zbuf2 = vec![f32::MAX; 64];
        let (a, b, c) = tri(1.0);
        draw_tri(&mut img2, &mut zbuf2, a, b, c, solid(0xFF00FF00), 1.0, false);
        let (a, b, c) = tri(5.0);
        draw_tri(&mut img2, &mut zbuf2, a, b, c, solid(0xFFFF0000), 1.0, false);
        assert_eq!(img2.at(1, 1), 0xFF00FF00, "更远的几何不应覆盖更近的");
    }

    #[test]
    fn degenerate_triangle_writes_nothing() {
        let mut img = Image::new(8, 8, 0);
        let mut zbuf = vec![f32::MAX; 64];
        // 三点共线，面积为零
        draw_tri(
            &mut img,
            &mut zbuf,
            Vertex::new(0.0, 0.0, 1.0, 0.0, 0.0),
            Vertex::new(4.0, 4.0, 1.0, 0.0, 0.0),
            Vertex::new(8.0, 8.0, 1.0, 0.0, 0.0),
            solid(opaque_white()),
            1.0,
            false,
        );
        assert!(img.px.iter().all(|v| *v == 0), "退化三角形不应写入任何像素");
    }

    #[test]
    fn fully_transparent_fragment_is_skipped() {
        let mut img = Image::new(8, 8, 0);
        let mut zbuf = vec![f32::MAX; 64];
        draw_tri(
            &mut img,
            &mut zbuf,
            Vertex::new(0.0, 0.0, 1.0, 0.0, 0.0),
            Vertex::new(8.0, 0.0, 1.0, 1.0, 0.0),
            Vertex::new(0.0, 8.0, 1.0, 0.0, 1.0),
            solid(0x00FFFFFF), // alpha 为 0
            1.0,
            false,
        );
        assert!(img.px.iter().all(|v| *v == 0), "完全透明的像素跳过");
        // 也不应写入深度
        assert!(zbuf.iter().all(|v| *v == f32::MAX));
    }

    #[test]
    fn transparent_draw_does_not_write_depth() {
        let mut img = Image::new(8, 8, 0);
        let mut zbuf = vec![f32::MAX; 64];
        draw_tri(
            &mut img,
            &mut zbuf,
            Vertex::new(0.0, 0.0, 1.0, 0.0, 0.0),
            Vertex::new(8.0, 0.0, 1.0, 1.0, 0.0),
            Vertex::new(0.0, 8.0, 1.0, 0.0, 1.0),
            solid(0x80FFFFFF),
            1.0,
            true,
        );
        assert!(
            zbuf.iter().all(|v| *v == f32::MAX),
            "半透明三角形不写深度，否则会挡住其后的几何"
        );
        // 但颜色应被混合写入
        assert_ne!(img.at(1, 1), 0);
    }

    #[test]
    fn shade_scales_rgb_and_keeps_alpha() {
        assert_eq!(shade_color(0xFFFFFFFF, 0.5), 0xFF808080);
        assert_eq!(shade_color(0xFFFFFFFF, 1.0), 0xFFFFFFFF);
        // alpha 不变
        assert_eq!((shade_color(0x80FFFFFF, 0.5) >> 24) & 0xFF, 0x80);
        // 不溢出
        assert_eq!(shade_color(0xFFFFFFFF, 2.0), 0xFFFFFFFF);
        assert_eq!(shade_color(0xFFFFFFFF, 0.0), 0xFF000000);
    }

    #[test]
    fn missing_texture_pattern_is_checkerboard() {
        // 相邻的 2x2 块颜色交替
        assert_ne!(missing_texture_color(0, 0), missing_texture_color(2, 0));
        assert_ne!(missing_texture_color(0, 0), missing_texture_color(0, 2));
        assert_eq!(missing_texture_color(0, 0), missing_texture_color(2, 2));
        // 两种颜色都不透明
        assert_eq!((missing_texture_color(0, 0) >> 24) & 0xFF, 0xFF);
        assert_eq!((missing_texture_color(2, 0) >> 24) & 0xFF, 0xFF);
    }

    #[test]
    fn sampler_returning_none_leaves_pixels_untouched() {
        let mut img = Image::new(8, 8, 0);
        let mut zbuf = vec![f32::MAX; 64];
        draw_tri(
            &mut img,
            &mut zbuf,
            Vertex::new(0.0, 0.0, 1.0, 0.0, 0.0),
            Vertex::new(8.0, 0.0, 1.0, 1.0, 0.0),
            Vertex::new(0.0, 8.0, 1.0, 0.0, 1.0),
            |_, _| None,
            1.0,
            false,
        );
        assert!(img.px.iter().all(|v| *v == 0));
        assert!(zbuf.iter().all(|v| *v == f32::MAX));
    }

    #[test]
    fn triangle_outside_image_is_clipped() {
        let mut img = Image::new(4, 4, 0);
        let mut zbuf = vec![f32::MAX; 16];
        // 完全在图像外，不应越界 panic
        draw_tri(
            &mut img,
            &mut zbuf,
            Vertex::new(100.0, 100.0, 1.0, 0.0, 0.0),
            Vertex::new(110.0, 100.0, 1.0, 1.0, 0.0),
            Vertex::new(100.0, 110.0, 1.0, 0.0, 1.0),
            solid(opaque_white()),
            1.0,
            false,
        );
        assert!(img.px.iter().all(|v| *v == 0));
    }
}
