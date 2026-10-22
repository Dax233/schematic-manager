//! 方块染色（生物群系着色）。
//!
//! Minecraft 的许多方块贴图是**灰度**的，颜色由「方块着色器」在渲染时乘上去：
//! 草方块顶面、树叶、藤蔓、红石线、水等都是这样。模型文件里的 `tintindex`
//! 只是着色器的**下标**，同一个下标在不同方块上含义不同——
//! 例如下标 0 在草方块上是草绿、在红石线上却是红色。
//! 因此这里按**方块名**决定颜色，而不是只按 tintindex 查一张固定表。
//!
//! 颜色取自原版（1.20.1）的默认值：生物群系取平原，避免依赖具体坐标。
//! 参考 legacy-cpp/render.cpp 的 ApplyTint（它写好了三个系数但从未被调用，
//! 因为传给 drawTri 的 tint 恒为 -1）。

/// 平原生物群系的草色
const GRASS: [f32; 3] = [0.568, 0.741, 0.349]; // #91BD59
/// 平原生物群系的树叶色
const FOLIAGE: [f32; 3] = [0.466, 0.670, 0.184]; // #77AB2F
/// 云杉树叶的固定色（原版不用生物群系染色）
const SPRUCE: [f32; 3] = [0.380, 0.600, 0.380]; // #619961
/// 白桦树叶的固定色
const BIRCH: [f32; 3] = [0.502, 0.655, 0.333]; // #80A755
/// 睡莲
const LILY_PAD: [f32; 3] = [0.125, 0.502, 0.188]; // #208030
/// 水（平原）
const WATER: [f32; 3] = [0.247, 0.463, 0.894]; // #3F76E4

/// 不染色（原样显示贴图）
pub const NO_TINT: [f32; 3] = [1.0, 1.0, 1.0];

/// 由方块名与属性的 power 值算红石线的颜色。
///
/// 原版按信号强度在深红与亮红之间插值（RedstoneWireBlock#getColor）：
/// 0 档几乎全黑红、15 档纯亮红。照搬原版公式而不是自己挑一个红色，
/// 这样不同信号强度的线路在预览里能看出区别。
fn redstone_color(power: i32) -> [f32; 3] {
    let f = (power.clamp(0, 15) as f32) / 15.0;
    let r = f * 0.6 + if f > 0.0 { 0.4 } else { 0.3 };
    let g = (f * f * 0.7 - 0.5).clamp(0.0, 1.0);
    let b = (f * f * 0.6 - 0.7).clamp(0.0, 1.0);
    [r, g, b]
}

/// 从属性列表取一个整数值，缺失或非法时返回 `None`。
fn prop_int(props: &[(String, String)], key: &str) -> Option<i32> {
    props
        .iter()
        .find(|(k, _)| k == key)
        .and_then(|(_, v)| v.parse().ok())
}

/// 判断方块名是否以给定结尾，同时兼容命名空间前缀。
fn name_is(name: &str, want: &str) -> bool {
    name == want || name.ends_with(&format!(":{want}"))
}

/// 名字里是否含某个片段（用于成组的方块，如各种树叶）
fn name_has(name: &str, frag: &str) -> bool {
    name.contains(frag)
}

/// 求一个面应乘的染色。
///
/// `tintindex` 为负表示不染色（模型里绝大多数面如此）。
/// 返回三个 0..1 的通道系数。
pub fn tint_color(name: &str, props: &[(String, String)], tintindex: i16) -> [f32; 3] {
    if tintindex < 0 {
        return NO_TINT;
    }

    // 红石线：不管下标是多少都按信号强度上红色（原版就是这样）
    if name_is(name, "redstone_wire") {
        return redstone_color(prop_int(props, "power").unwrap_or(0));
    }

    // 水与含水方块
    if name_is(name, "water") || name_is(name, "flowing_water") {
        return WATER;
    }

    match tintindex {
        // 树叶：原版里云杉与白桦用固定色，其余走生物群系
        1 => leaves_color(name),
        // 草类
        0 => {
            if name_has(name, "leaves") || name_has(name, "vine") {
                // 藤蔓与原版一致走树叶色
                leaves_color(name)
            } else if name_is(name, "lily_pad") {
                LILY_PAD
            } else if name_is(name, "water") {
                WATER
            } else {
                GRASS
            }
        }
        // 水
        2 => WATER,
        // 其余下标原版没有默认着色器，保持原样
        _ => NO_TINT,
    }
}

/// 树叶的染色。
///
/// 特例：**樱花树叶不染色**。它的贴图本身就是粉色（实测最大通道
/// [252, 218, 239]、86% 的像素明显饱和），再乘树叶绿会得到橄榄色——
/// 与实际观感明显不符。原版里樱花树确实是粉色，因此这里保持贴图原色。
///
/// 注：不能靠「贴图是否已有颜色」来通用判断该不该染色——
/// 实测甘蔗贴图 94% 饱和、草方块侧面 97% 饱和，但它们都该按原版染色
/// （前者草色、后者 tintindex 本就是 -1）。模型里的 tintindex 才是权威依据，
/// 樱花是唯一的例外，所以按方块名单独列出。
fn leaves_color(name: &str) -> [f32; 3] {
    if name_has(name, "cherry_leaves") {
        NO_TINT
    } else if name_has(name, "spruce_leaves") {
        SPRUCE
    } else if name_has(name, "birch_leaves") {
        BIRCH
    } else {
        FOLIAGE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn props(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn negative_tintindex_means_no_tint() {
        assert_eq!(tint_color("minecraft:stone", &[], -1), NO_TINT);
        assert_eq!(tint_color("minecraft:oak_planks", &[], -1), NO_TINT);
    }

    #[test]
    fn leaves_get_foliage_green() {
        // 原版橡树树叶在平原是 #77AB2F
        assert_eq!(tint_color("minecraft:oak_leaves", &[], 0), FOLIAGE);
        assert_eq!(tint_color("minecraft:jungle_leaves", &[], 0), FOLIAGE);
        // 云杉与白桦是固定色，不走生物群系
        assert_eq!(tint_color("minecraft:spruce_leaves", &[], 0), SPRUCE);
        assert_eq!(tint_color("minecraft:birch_leaves", &[], 0), BIRCH);
    }

    #[test]
    fn vines_are_foliage_colored() {
        assert_eq!(tint_color("minecraft:vine", &[], 0), FOLIAGE);
    }

    #[test]
    fn cherry_leaves_keep_their_own_pink() {
        // 樱花树叶的贴图本身就是粉色。若按树叶绿染色，粉 × 绿 会得到橄榄色，
        // 与实际观感明显不符，因此这一种必须保持原色。
        assert_eq!(
            tint_color("minecraft:cherry_leaves", &[], 0),
            NO_TINT,
            "樱花树叶不应被染成树叶绿"
        );
        // 同组的其它树叶仍然要染色，别把整类都关掉
        assert_eq!(tint_color("minecraft:oak_leaves", &[], 0), FOLIAGE);
        assert_eq!(tint_color("minecraft:jungle_leaves", &[], 0), FOLIAGE);
        assert_eq!(tint_color("minecraft:mangrove_leaves", &[], 0), FOLIAGE);
    }

    #[test]
    fn grass_is_grass_green_not_foliage() {
        // 下标 0 在草方块上是草绿、在树叶上是叶绿——必须按方块名区分，
        // 否则草方块会长成树叶色
        assert_eq!(tint_color("minecraft:grass_block", &[], 0), GRASS);
        assert_eq!(tint_color("minecraft:short_grass", &[], 0), GRASS);
        assert_eq!(tint_color("minecraft:sugar_cane", &[], 0), GRASS);
        assert_ne!(
            tint_color("minecraft:grass_block", &[], 0),
            tint_color("minecraft:oak_leaves", &[], 0),
            "草与树叶不能同色"
        );
    }

    #[test]
    fn redstone_wire_is_red_and_varies_with_power() {
        let off = tint_color("minecraft:redstone_wire", &props(&[("power", "0")]), 0);
        let on = tint_color("minecraft:redstone_wire", &props(&[("power", "15")]), 0);
        // 必须是红色系：红通道明显高于绿蓝
        for (label, c) in [("power=0", off), ("power=15", on)] {
            assert!(c[0] > c[1] && c[0] > c[2], "{label} 应为红色系，实际 {c:?}");
        }
        // 信号越强越亮
        assert!(on[0] > off[0], "满信号的红色应比无信号更亮");
        // 无信号也不是纯黑（原版 0 档仍能看见线）
        assert!(off[0] > 0.0);
    }

    #[test]
    fn redstone_power_missing_or_bad_is_treated_as_zero() {
        let a = tint_color("minecraft:redstone_wire", &[], 0);
        let b = tint_color("minecraft:redstone_wire", &props(&[("power", "0")]), 0);
        let c = tint_color(
            "minecraft:redstone_wire",
            &props(&[("power", "不是数字")]),
            0,
        );
        assert_eq!(a, b, "缺 power 应按 0 处理");
        assert_eq!(c, b, "非法 power 应按 0 处理");
    }

    #[test]
    fn water_is_blue() {
        let c = tint_color("minecraft:water", &[], 0);
        assert_eq!(c, WATER);
        // 蓝通道最高
        assert!(c[2] > c[0] && c[2] > c[1], "水应为蓝色系，实际 {c:?}");
    }

    #[test]
    fn name_matching_accepts_prefixed_and_bare_names() {
        // 数据库里带 minecraft: 前缀，但不应假设一定带
        assert_eq!(tint_color("oak_leaves", &[], 0), FOLIAGE);
        assert_eq!(tint_color("minecraft:oak_leaves", &[], 0), FOLIAGE);
    }

    #[test]
    fn unknown_tint_index_keeps_original_color() {
        // 原版没有下标 7 对应的默认着色器，不应擅自上色
        assert_eq!(tint_color("minecraft:stone", &[], 7), NO_TINT);
    }
}
