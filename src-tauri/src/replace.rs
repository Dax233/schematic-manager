//! 方块替换。
//!
//! 对照 legacy-cpp/litematic.cpp 的 ReplaceRule / ReplaceBlocks。
//!
//! 替换在**调色板层面**进行：同一状态的方块在区域里共享一个调色板项，
//! 因此只需逐项计算替换结果并重映射索引，而不必逐方块改写。
//! 调色板通常只有几十项，而方块可能上百万个，差别很大。

use crate::litematic::Schematic;

/// 方块知识来源。
///
/// 替换需要知道目标方块支持哪些属性、以及必需属性的默认值，
/// 否则会写出游戏无法识别的状态（例如给石头加上 facing）。
/// 由方块库实现，测试可提供自己的实现。
pub trait BlockInfo {
    /// 目标方块是否支持某属性取某值
    fn supports_prop(&self, block: &str, prop: &str, value: &str) -> bool;
    /// 目标方块的必需属性及默认值
    fn default_props(&self, block: &str) -> Vec<(String, String)>;
}

/// 一条替换规则
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReplaceRule {
    /// 源方块名（必填，不含属性）
    pub from_name: String,
    /// 可选：限定源方块必须同时满足这些属性
    pub from_props: Vec<(String, String)>,
    /// 目标方块名
    pub to_name: String,
    /// 可选：替换后强制设置的属性
    pub to_props: Vec<(String, String)>,
    /// 是否继承源方块中目标也支持的属性
    pub inherit_props: bool,
    /// 目标不支持的属性是否保留。默认 false：
    /// 保留会让游戏读到无法识别的属性，通常应当丢弃。
    pub keep_unknown_props: bool,
}

/// 替换结果统计
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReplaceOutcome {
    /// 被改写的方块实例数
    pub replaced: u64,
    /// 实际发生变化的调色板项数
    pub palette_changed: usize,
}

/// 判断源方块是否命中规则
fn matches(state: &crate::litematic::BlockState, rule: &ReplaceRule) -> bool {
    if state.name != rule.from_name {
        return false;
    }
    rule.from_props
        .iter()
        .all(|(k, v)| state.prop(k) == Some(v.as_str()))
}

/// 计算替换后的新状态。返回 None 表示无需替换。
fn replacement(
    state: &crate::litematic::BlockState,
    rule: &ReplaceRule,
    info: &dyn BlockInfo,
) -> Option<crate::litematic::BlockState> {
    if !matches(state, rule) {
        return None;
    }
    let mut next = crate::litematic::BlockState::new(&rule.to_name);

    // 继承：源属性里目标也支持的才保留
    if rule.inherit_props {
        for (k, v) in &state.props {
            if info.supports_prop(&rule.to_name, k, v) {
                next.set_prop(k, v);
            }
        }
    } else if rule.keep_unknown_props {
        for (k, v) in &state.props {
            next.set_prop(k, v);
        }
    }

    // 补齐目标必需的属性；已有取值则保留（继承来的优先）
    for (k, v) in info.default_props(&rule.to_name) {
        if next.prop(&k).is_none() {
            next.set_prop(&k, &v);
        }
    }

    // 强制属性最后应用，优先级最高
    for (k, v) in &rule.to_props {
        next.set_prop(k, v);
    }

    // 结果与原来完全一致时视作未替换，避免把「没变化」计入统计
    if next == *state {
        None
    } else {
        Some(next)
    }
}

/// 对整个投影执行替换。
///
/// `limit` 大于 0 时最多替换那么多个方块实例。
pub fn replace_blocks(
    sc: &mut Schematic,
    rule: &ReplaceRule,
    info: &dyn BlockInfo,
    limit: u64,
) -> ReplaceOutcome {
    let mut outcome = ReplaceOutcome::default();

    for r in sc.regions.iter_mut() {
        // 逐调色板项算出替换后的状态；None 表示不变
        let mut mapped: Vec<Option<usize>> = vec![None; r.palette.len()];
        // 按索引遍历：循环体内会向 palette 追加项，不能同时持有它的借用
        let original_len = r.palette.len();
        for i in 0..original_len {
            let st = r.palette[i].clone();
            let Some(next) = replacement(&st, rule, info) else { continue };
            // 已存在同状态则复用，避免调色板无谓膨胀
            let idx = match r.palette.iter().position(|p| *p == next) {
                Some(k) => k,
                None => {
                    r.palette.push(next);
                    r.palette.len() - 1
                }
            };
            mapped[i] = Some(idx);
        }

        if mapped.iter().all(|m| m.is_none()) {
            continue;
        }

        for slot in r.indices.iter_mut() {
            let cur = *slot as usize;
            let Some(Some(target)) = mapped.get(cur).copied() else { continue };
            if limit > 0 && outcome.replaced >= limit {
                continue;
            }
            if target != cur {
                *slot = target as i32;
                outcome.replaced += 1;
            }
        }
        outcome.palette_changed += mapped.iter().filter(|m| m.is_some()).count();
    }

    if outcome.replaced > 0 {
        sc.blocks_modified = true;
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::litematic::{BlockState, Region};

    /// 一个只认识楼梯类方块的方块库替身
    struct Stairs {
        props: Vec<(String, String)>,
    }

    impl BlockInfo for Stairs {
        fn supports_prop(&self, block: &str, prop: &str, value: &str) -> bool {
            if !block.starts_with("minecraft:") || !block.contains("stairs") {
                return false;
            }
            match prop {
                "facing" => ["north", "south", "east", "west"].contains(&value),
                "half" => ["top", "bottom"].contains(&value),
                "shape" => ["straight", "inner_left", "outer_left"].contains(&value),
                _ => false,
            }
        }
        fn default_props(&self, block: &str) -> Vec<(String, String)> {
            if block.contains("stairs") {
                vec![("facing".into(), "north".into()), ("half".into(), "bottom".into())]
            } else {
                self.props.clone()
            }
        }
    }

    fn info() -> Stairs {
        Stairs { props: Vec::new() }
    }

    fn schematic_with(palette: Vec<BlockState>, indices: Vec<i32>) -> Schematic {
        Schematic {
            root: crate::nbt::Value::Compound(vec![]),
            regions: vec![Region {
                name: "r".into(),
                pos: [0, 0, 0],
                size: [indices.len() as i32, 1, 1],
                volume: indices.len(),
                bits: 2,
                palette,
                indices,
                ..Default::default()
            }],
            path: String::new(),
            loaded: true,
            meta_modified: false,
            blocks_modified: false,
        }
    }

    fn rule(from: &str, to: &str) -> ReplaceRule {
        ReplaceRule {
            from_name: from.into(),
            to_name: to.into(),
            inherit_props: true,
            ..Default::default()
        }
    }

    #[test]
    fn replaces_palette_and_remaps_indices() {
        let mut sc = schematic_with(
            vec![BlockState::new("minecraft:stone"), BlockState::new("minecraft:dirt")],
            vec![0, 1, 0, 1, 0],
        );
        let out = replace_blocks(&mut sc, &rule("minecraft:stone", "minecraft:gold_block"), &info(), 0);

        assert_eq!(out.replaced, 3, "三个石头实例");
        assert!(sc.blocks_modified, "发生替换后应置改动标志");
        let r = &sc.regions[0];
        for (i, idx) in r.indices.iter().enumerate() {
            let want = if i % 2 == 0 { "minecraft:gold_block" } else { "minecraft:dirt" };
            assert_eq!(r.palette[*idx as usize].name, want, "第 {i} 个");
        }
    }

    #[test]
    fn non_matching_blocks_are_untouched() {
        let mut sc = schematic_with(vec![BlockState::new("minecraft:dirt")], vec![0, 0]);
        let out = replace_blocks(&mut sc, &rule("minecraft:stone", "minecraft:gold_block"), &info(), 0);
        assert_eq!(out.replaced, 0);
        assert!(!sc.blocks_modified);
        assert_eq!(sc.regions[0].palette[0].name, "minecraft:dirt");
    }

    #[test]
    fn from_props_narrow_the_match() {
        let mut east = BlockState::new("minecraft:oak_stairs");
        east.set_prop("facing", "east");
        let mut west = BlockState::new("minecraft:oak_stairs");
        west.set_prop("facing", "west");
        let mut sc = schematic_with(vec![east, west], vec![0, 1, 0]);

        let mut r = rule("minecraft:oak_stairs", "minecraft:spruce_stairs");
        r.from_props = vec![("facing".into(), "east".into())];
        let out = replace_blocks(&mut sc, &r, &info(), 0);

        assert_eq!(out.replaced, 2, "只有朝东的两处命中");
        let reg = &sc.regions[0];
        assert_eq!(reg.palette[reg.indices[1] as usize].name, "minecraft:oak_stairs", "朝西的不变");
    }

    #[test]
    fn supported_props_are_inherited() {
        let mut src = BlockState::new("minecraft:oak_stairs");
        src.set_prop("facing", "east");
        src.set_prop("half", "top");
        let mut sc = schematic_with(vec![src], vec![0]);

        replace_blocks(&mut sc, &rule("minecraft:oak_stairs", "minecraft:spruce_stairs"), &info(), 0);
        let st = &sc.regions[0].palette[sc.regions[0].indices[0] as usize];
        assert_eq!(st.name, "minecraft:spruce_stairs");
        assert_eq!(st.prop("facing"), Some("east"), "目标支持的属性应继承");
        assert_eq!(st.prop("half"), Some("top"));
    }

    #[test]
    fn unsupported_props_are_dropped_and_defaults_filled() {
        // 源是楼梯（有 facing/half），目标是石头（都不支持）
        let mut src = BlockState::new("minecraft:oak_stairs");
        src.set_prop("facing", "east");
        src.set_prop("half", "top");
        let mut sc = schematic_with(vec![src], vec![0]);

        replace_blocks(&mut sc, &rule("minecraft:oak_stairs", "minecraft:stone"), &info(), 0);
        let st = &sc.regions[0].palette[sc.regions[0].indices[0] as usize];
        assert_eq!(st.name, "minecraft:stone");
        assert!(st.prop("facing").is_none(), "目标不支持的属性应丢弃");
        assert!(st.prop("half").is_none());
    }

    #[test]
    fn to_props_override_inherited_values() {
        let mut src = BlockState::new("minecraft:oak_stairs");
        src.set_prop("facing", "east");
        let mut sc = schematic_with(vec![src], vec![0]);

        let mut r = rule("minecraft:oak_stairs", "minecraft:spruce_stairs");
        r.to_props = vec![("facing".into(), "west".into())];
        replace_blocks(&mut sc, &r, &info(), 0);

        let st = &sc.regions[0].palette[sc.regions[0].indices[0] as usize];
        assert_eq!(st.prop("facing"), Some("west"), "强制属性应覆盖继承值");
    }

    #[test]
    fn limit_caps_replaced_blocks() {
        let mut sc = schematic_with(vec![BlockState::new("minecraft:stone")], vec![0, 0, 0, 0, 0]);
        let out = replace_blocks(&mut sc, &rule("minecraft:stone", "minecraft:gold_block"), &info(), 2);
        assert_eq!(out.replaced, 2, "最多替换 2 个");
    }

    #[test]
    fn equivalent_result_is_not_counted() {
        // 替换成同一状态不应产生改动
        let mut sc = schematic_with(vec![BlockState::new("minecraft:stone")], vec![0, 0]);
        let out = replace_blocks(&mut sc, &rule("minecraft:stone", "minecraft:stone"), &info(), 0);
        assert_eq!(out.replaced, 0);
        assert!(!sc.blocks_modified, "无实际变化时不应置改动标志");
    }

    #[test]
    fn reuse_existing_palette_entry() {
        // 目标状态已在调色板中时，应复用而不是追加新项
        let mut sc = schematic_with(
            vec![BlockState::new("minecraft:stone"), BlockState::new("minecraft:gold_block")],
            vec![0, 0],
        );
        replace_blocks(&mut sc, &rule("minecraft:stone", "minecraft:gold_block"), &info(), 0);
        assert_eq!(sc.regions[0].palette.len(), 2, "调色板不应膨胀");
        assert_eq!(sc.regions[0].indices, vec![1, 1]);
    }
}
