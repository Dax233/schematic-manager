//! 把可见四边形整理成前端可直接使用的网格数据。
//!
//! 渲染交给前端的 Three.js，因此本模块只负责几何整理：
//! 按贴图分批、展开顶点、生成三角形索引。
//! 按贴图分批是为了让前端「一批一个材质」，避免自己再维护图集。

use crate::visibility::Quad;

/// 一个批次对应一张贴图，前端据此创建一个材质。
#[derive(Debug, Clone, PartialEq)]
pub struct TextureBatch {
    /// 贴图池下标
    pub texture: u32,
    /// 顶点位置，每 3 个 f32 为一个顶点
    pub positions: Vec<f32>,
    /// 顶点 UV，每 2 个 f32 为一个顶点
    pub uvs: Vec<f32>,
    /// 每顶点的明暗系数（由面朝向决定，用于表现立体感）
    pub shades: Vec<f32>,
    /// 三角形索引，每 3 个构成一个三角形
    pub indices: Vec<u32>,
}

impl TextureBatch {
    pub fn vertex_count(&self) -> usize {
        self.positions.len() / 3
    }
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }
}

/// 整个投影的网格数据
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MeshData {
    pub batches: Vec<TextureBatch>,
    /// 顶点总数，供前端预估内存
    pub total_vertices: usize,
    pub total_triangles: usize,
}

/// 把四边形列表按贴图分批展开成三角形网格。
///
/// 每个四边形拆成两个三角形。UV 直接沿用模型空间的 0..16 取值，
/// 由前端除以 16 归一化——这样 Rust 侧不必关心贴图分辨率。
pub fn build(quads: &[Quad]) -> MeshData {
    let mut batches: Vec<TextureBatch> = Vec::new();
    // 贴图下标 -> 批次下标。四边形可达上百万而批次可达上千，
    // 逐个四边形线性扫批次会退化成平方复杂度。
    let mut by_texture: std::collections::HashMap<u32, usize> =
        std::collections::HashMap::new();

    for q in quads {
        // 贴图为哨兵值时跳过：没有可用的贴图，画出来也只是占位色块
        if q.texture == crate::blockdb::NO_TEXTURE {
            continue;
        }
        let idx = match by_texture.get(&q.texture) {
            Some(i) => *i,
            None => {
                batches.push(TextureBatch {
                    texture: q.texture,
                    positions: Vec::new(),
                    uvs: Vec::new(),
                    shades: Vec::new(),
                    indices: Vec::new(),
                });
                let i = batches.len() - 1;
                by_texture.insert(q.texture, i);
                i
            }
        };
        let batch = &mut batches[idx];

        let base = batch.vertex_count() as u32;
        for k in 0..4 {
            let p = &q.corners[k];
            batch.positions.extend_from_slice(p);
            batch.uvs.extend_from_slice(&q.uv[k]);
            batch.shades.push(q.shade);
        }
        // 四边形按 0-2-1、0-3-2 拆成两个三角形。
        //
        // 顺序是**反**的（不是直觉上的 0-1-2、0-2-3），这一点不能改：
        // 角点顺序由 Minecraft 模型规范固定（见 geometry::build_face_corners，
        // 它决定 UV 的方向），而这些角点按 0-1-2 组成的三角形，其几何法线
        // 指向方块**内部**。前端材质用 THREE.FrontSide，只有逆时针（法线朝
        // 观察者）的面才画，于是六个面从外面看全是背面被剔除，画面变成
        // 「看得见对面墙的内侧」——也就是看穿方块。
        //
        // legacy-cpp 的软件光栅器不做背面剔除（render.cpp 的 drawTri 只判
        // 面积是否为 0），因此同样的角点顺序在那里没问题，移植到 three.js
        // 才暴露出来。
        //
        // 交换后两个顶点即可翻转绕序，且不改动顶点本身，UV 与明暗的对应
        // 关系保持不变。
        batch.indices.extend_from_slice(&[base, base + 2, base + 1]);
        batch.indices.extend_from_slice(&[base, base + 3, base + 2]);
    }

    // 结果稳定：按贴图下标排序，便于测试与缓存比对
    batches.sort_by_key(|b| b.texture);

    let total_vertices = batches.iter().map(|b| b.vertex_count()).sum();
    let total_triangles = batches.iter().map(|b| b.triangle_count()).sum();
    MeshData { batches, total_vertices, total_triangles }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad(texture: u32, shade: f32) -> Quad {
        Quad {
            corners: [
                [0.0, 0.0, 0.0],
                [16.0, 0.0, 0.0],
                [16.0, 16.0, 0.0],
                [0.0, 16.0, 0.0],
            ],
            uv: [[0.0, 0.0], [16.0, 0.0], [16.0, 16.0], [0.0, 16.0]],
            texture,
            shade,
            block_pos: [0, 0, 0],
        }
    }

    #[test]
    fn one_quad_becomes_four_vertices_and_two_triangles() {
        let m = build(&[quad(3, 1.0)]);
        assert_eq!(m.batches.len(), 1);
        let b = &m.batches[0];
        assert_eq!(b.vertex_count(), 4);
        assert_eq!(b.triangle_count(), 2);
        assert_eq!(b.positions.len(), 12);
        assert_eq!(b.uvs.len(), 8);
        assert_eq!(b.shades.len(), 4);
        assert_eq!(b.indices, vec![0, 2, 1, 0, 3, 2]);
        assert_eq!(m.total_vertices, 4);
        assert_eq!(m.total_triangles, 2);
    }

    /// 每个面的三角形绕序必须使其几何法线**朝外**。
    ///
    /// 这条直接对应用户看到的「透视方块」：前端材质是 THREE.FrontSide，
    /// 只画逆时针（法线朝观察者）的面。若绕序反了，六个面从外面看全是
    /// 背面而被剔除，画面变成看得见对面墙的内侧。
    ///
    /// 因此这里不只看索引字面量，而是真正算出法线并与「该面朝外方向」
    /// 比较——索引写对但角点顺序被改动时，也能发现。
    #[test]
    fn face_winding_makes_normals_point_outward() {
        use crate::blockdb::Face;
        use crate::geometry::build_face_corners;

        // 一个 16³ 的元素（模型空间 0..16），六个面各生成一个四边形
        let from = [0.0f32, 0.0, 0.0];
        let to = [16.0f32, 16.0, 16.0];
        let uv = [0.0f32, 0.0, 16.0, 16.0];
        let outward: [(Face, [f32; 3]); 6] = [
            (Face::Down, [0.0, -1.0, 0.0]),
            (Face::Up, [0.0, 1.0, 0.0]),
            (Face::North, [0.0, 0.0, -1.0]),
            (Face::South, [0.0, 0.0, 1.0]),
            (Face::West, [-1.0, 0.0, 0.0]),
            (Face::East, [1.0, 0.0, 0.0]),
        ];

        let quads: Vec<Quad> = outward
            .iter()
            .map(|(f, _)| {
                let c = build_face_corners(*f, from, to, uv);
                Quad {
                    corners: c.pos,
                    uv: c.uv,
                    texture: 1,
                    shade: 1.0,
                    block_pos: [0, 0, 0],
                }
            })
            .collect();

        let m = build(&quads);
        assert_eq!(m.batches.len(), 1);
        let b = &m.batches[0];
        let v = |i: u32| -> [f32; 3] {
            let i = i as usize * 3;
            [b.positions[i], b.positions[i + 1], b.positions[i + 2]]
        };
        let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
        let cross = |a: [f32; 3], b: [f32; 3]| {
            [
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
            ]
        };
        let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];

        // 每个四边形两个三角形，绕序必须一致
        for (qi, (face, out)) in outward.iter().enumerate() {
            let t0 = &b.indices[qi * 6..qi * 6 + 3];
            let t1 = &b.indices[qi * 6 + 3..qi * 6 + 6];
            for t in [t0, t1] {
                let n = cross(sub(v(t[1]), v(t[0])), sub(v(t[2]), v(t[0])));
                assert!(
                    dot(n, *out) > 0.0,
                    "{face:?} 的三角形 {t:?} 法线朝内（应为 {out:?}），\
                     前端会用 FrontSide 把它剔除，表现为看穿方块。\
                     法线 = {n:?}"
                );
            }
        }
    }

    #[test]
    fn same_texture_shares_one_batch() {
        let m = build(&[quad(7, 0.8), quad(7, 0.8), quad(7, 0.8)]);
        assert_eq!(m.batches.len(), 1, "相同贴图应合并到同一批次");
        assert_eq!(m.batches[0].vertex_count(), 12);
        assert_eq!(m.batches[0].triangle_count(), 6);
        // 第二、三个四边形的索引应各自偏移
        assert_eq!(&m.batches[0].indices[6..12], &[4, 6, 5, 4, 7, 6]);
    }

    #[test]
    fn different_textures_split_into_batches() {
        let m = build(&[quad(5, 1.0), quad(2, 1.0), quad(9, 1.0)]);
        assert_eq!(m.batches.len(), 3);
        // 批次按下标排序，结果稳定
        let ids: Vec<u32> = m.batches.iter().map(|b| b.texture).collect();
        assert_eq!(ids, vec![2, 5, 9]);
    }

    #[test]
    fn missing_texture_quads_are_skipped() {
        let m = build(&[quad(crate::blockdb::NO_TEXTURE, 1.0), quad(4, 1.0)]);
        assert_eq!(m.batches.len(), 1, "哨兵贴图不应产生批次");
        assert_eq!(m.batches[0].texture, 4);
    }

    #[test]
    fn shade_is_per_vertex_and_matches_quad() {
        let m = build(&[quad(1, 0.55), quad(1, 0.85)]);
        let s = &m.batches[0].shades;
        assert_eq!(s.len(), 8);
        assert!(s[..4].iter().all(|v| (*v - 0.55).abs() < 1e-6));
        assert!(s[4..].iter().all(|v| (*v - 0.85).abs() < 1e-6));
    }

    #[test]
    fn empty_input_yields_empty_mesh() {
        let m = build(&[]);
        assert!(m.batches.is_empty());
        assert_eq!(m.total_vertices, 0);
        assert_eq!(m.total_triangles, 0);
    }

    #[test]
    fn indices_stay_within_vertex_range() {
        let quads: Vec<Quad> = (0..50).map(|i| quad((i % 3) as u32, 0.7)).collect();
        let m = build(&quads);
        for b in &m.batches {
            let n = b.vertex_count() as u32;
            for idx in &b.indices {
                assert!(*idx < n, "索引 {idx} 超出顶点数 {n}");
            }
        }
    }
}
