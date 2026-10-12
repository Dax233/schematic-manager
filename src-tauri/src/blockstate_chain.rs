//! 模型继承链解析与贴图变量替换。
//!
//! 用途：Minecraft 的模型可以 `parent` 继承另一个模型。导入 jar 时要沿
//! 父链把 textures 逐级合并、并取出最近一级的 elements，最后把面里的
//! 贴图变量（`#all` 之类）替换成实际路径。
//!
//! 对照 legacy-cpp/packbuild.cpp 的 BuildChain 与 ResolveTexRef。
//!
//! 本模块不涉及文件 IO：调用方提供「按 id 取模型」的闭包，
//! 因此可完整测试继承与替换逻辑。
//!
//! 合并规则（与原实现一致）：
//!   - textures 逐级合并，子级覆盖父级
//!   - elements 取链上最近一级出现的那个（子级有则整个替换，不逐元素合并）
//!   - force_translucent 只要链上任意一级声明过即为真

use crate::blockstate::ModelDef;
use std::collections::HashMap;

/// 贴图变量的原始值
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TexValue {
    /// 引用内容：可能是 `#other`、也可能是 `namespace:block/stone` 这样的路径
    pub sprite: String,
    /// 该贴图强制半透明（用于玻璃等）
    pub force_translucent: bool,
}

/// 一条继承链解析后的结果
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Chain {
    /// 贴图变量表，保留原始引用形式（可能仍含 `#`）
    pub textures: HashMap<String, TexValue>,
    /// 链上最近一级的 elements
    pub elements: Vec<crate::blockstate::ModelElementDef>,
    pub force_translucent: bool,
}

/// 父链的最大深度。
///
/// 原实现用 16：真实模型的继承链通常不超过 4 层，
/// 给一个上限是为了让环形 parent（A 继承 B、B 继承 A）能终止。
const MAX_DEPTH: usize = 16;

/// 把模型 id 规范成 `命名空间:路径`。
///
/// 不带命名空间时沿用当前上下文（通常来自模型所在目录）。
pub fn resolve_model_id(raw: &str, cur_ns: &str) -> Option<String> {
    if raw.is_empty() {
        return None;
    }
    match raw.split_once(':') {
        Some((ns, path)) => {
            if path.is_empty() {
                None
            } else {
                Some(format!("{ns}:{path}"))
            }
        }
        None => Some(format!("{cur_ns}:{raw}")),
    }
}

/// 模型 id 对应的 jar 内条目路径
pub fn model_entry_path(id: &str) -> Option<String> {
    let (ns, path) = id.split_once(':')?;
    if ns.is_empty() || path.is_empty() {
        return None;
    }
    Some(format!("assets/{ns}/models/{path}.json"))
}

/// 解析继承链。
///
/// `load` 按规范 id 返回模型定义；返回 None 表示该模型不存在
/// （父模型缺失时按「无父级」处理，而不是整体失败——真实资源包里
/// 存在指向不存在父模型的模型，让整个方块因此消失代价过大）。
pub fn build_chain<F>(start_id: &str, cur_ns: &str, load: &mut F) -> Chain
where
    F: FnMut(&str) -> Option<ModelDef>,
{
    let mut visiting: Vec<String> = Vec::new();
    build_inner(start_id, cur_ns, load, &mut visiting, 0)
}

fn build_inner<F>(
    start_id: &str,
    cur_ns: &str,
    load: &mut F,
    visiting: &mut Vec<String>,
    depth: usize,
) -> Chain
where
    F: FnMut(&str) -> Option<ModelDef>,
{
    if depth > MAX_DEPTH {
        return Chain::default();
    }
    let Some(id) = resolve_model_id(start_id, cur_ns) else {
        return Chain::default();
    };
    // 环形继承：已在当前路径上出现过就直接返回，避免无限递归
    if visiting.iter().any(|v| v == &id) {
        return Chain::default();
    }
    let Some(def) = load(&id) else {
        return Chain::default();
    };

    visiting.push(id.clone());
    let mut ch = if def.parent.is_empty() {
        Chain::default()
    } else {
        // 父模型的命名空间沿用当前模型的：`parent` 常写作 "block/cube"，
        // 不带命名空间
        let (ns, _) = id.split_once(':').unwrap_or(("minecraft", ""));
        build_inner(&def.parent, ns, load, visiting, depth + 1)
    };
    visiting.pop();

    // 本级的 textures 覆盖父级
    for (k, v, force) in &def.textures {
        let tv = TexValue {
            sprite: v.clone(),
            force_translucent: *force,
        };
        // 链上任意一级声明过即为真：父级已经置位时不能被本级的 false 抹掉
        if *force {
            ch.force_translucent = true;
        }
        ch.textures.insert(k.clone(), tv);
    }
    // 本级的 elements 存在则整体替换父级的
    if !def.elements.is_empty() {
        ch.elements = def.elements.clone();
    }
    ch
}

/// 把贴图引用迭代替换成最终路径。
///
/// `#all` 这类变量可能指向另一个变量，因此要迭代；出现环时返回 None，
/// 而不是给出一个中间态或死循环。
pub fn resolve_tex_ref(textures: &HashMap<String, TexValue>, raw: &str) -> Option<String> {
    let mut cur = raw.to_string();
    let mut seen: Vec<String> = Vec::new();

    for _ in 0..MAX_DEPTH {
        if !cur.starts_with('#') {
            // 已是字面路径
            return if cur.is_empty() { None } else { Some(cur) };
        }
        let key = cur[1..].to_string();
        if seen.iter().any(|s| s == &key) {
            return None; // 成环
        }
        seen.push(key.clone());
        let v = textures.get(&key)?;
        if v.sprite.is_empty() {
            return None;
        }
        cur = v.sprite.clone();
    }
    // 迭代次数用尽仍未收敛，视为异常
    None
}

/// 判断贴图是否为「强制半透明」。
///
/// 沿变量链查找：链上任意一级声明了 force_translucent 即为真。
pub fn is_force_translucent(
    textures: &HashMap<String, TexValue>,
    raw: &str,
) -> bool {
    let mut cur = raw.to_string();
    let mut seen: Vec<String> = Vec::new();
    for _ in 0..MAX_DEPTH {
        if !cur.starts_with('#') {
            return false;
        }
        let key = cur[1..].to_string();
        if seen.iter().any(|s| s == &key) {
            return false;
        }
        seen.push(key.clone());
        let Some(v) = textures.get(&key) else { return false };
        if v.force_translucent {
            return true;
        }
        cur = v.sprite.clone();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blockstate::{ModelElementDef, ModelFace};

    fn elem(y: f32) -> ModelElementDef {
        ModelElementDef {
            from: [0.0, y, 0.0],
            to: [16.0, 16.0, 16.0],
            faces: vec![(
                "north".to_string(),
                ModelFace {
                    texture: "#all".to_string(),
                    cullface: String::new(),
                    tintindex: -1,
                    rotation: 0,
                    uv: None,
                },
            )],
            rotation: None,
        }
    }

    /// 构造一个模型。`force` 为该模型全部贴图变量的强制半透明标记。
    fn model(parent: &str, textures: &[(&str, &str)], elements: Vec<ModelElementDef>) -> ModelDef {
        model_tex(parent, textures, elements, false)
    }

    fn model_tex(
        parent: &str,
        textures: &[(&str, &str)],
        elements: Vec<ModelElementDef>,
        force: bool,
    ) -> ModelDef {
        ModelDef {
            parent: parent.to_string(),
            textures: textures
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string(), force))
                .collect(),
            elements,
            ambientocclusion: true,
        }
    }

    /// 用一个静态表当模型仓库
    fn loader<'a>(
        table: &'a [(&'a str, ModelDef)],
    ) -> impl FnMut(&str) -> Option<ModelDef> + 'a {
        move |id: &str| table.iter().find(|(k, _)| *k == id).map(|(_, v)| v.clone())
    }

    #[test]
    fn resolve_model_id_adds_current_namespace() {
        assert_eq!(resolve_model_id("block/cube", "minecraft").unwrap(), "minecraft:block/cube");
        assert_eq!(resolve_model_id("mod:block/x", "minecraft").unwrap(), "mod:block/x");
        assert!(resolve_model_id("", "minecraft").is_none());
        assert!(resolve_model_id("mod:", "minecraft").is_none(), "路径为空应拒绝");
    }

    #[test]
    fn model_entry_path_matches_jar_layout() {
        assert_eq!(
            model_entry_path("minecraft:block/cube_all").unwrap(),
            "assets/minecraft/models/block/cube_all.json"
        );
        assert!(model_entry_path("nopath").is_none());
    }

    #[test]
    fn child_textures_override_parent() {
        let table = vec![
            ("minecraft:block/cube", model("", &[("all", "block/iron")], vec![elem(0.0)])),
            ("minecraft:block/door", model("block/cube", &[("all", "block/wood")], vec![])),
        ];
        let mut l = loader(&table);
        let ch = build_chain("block/door", "minecraft", &mut l);
        assert_eq!(
            ch.textures.get("all").unwrap().sprite, "block/wood",
            "子级应覆盖父级同名变量"
        );
    }

    /// 链级的 force_translucent 必须由 build_chain 累积出来。
    ///
    /// 此前 blockstate.rs 解析时整段丢掉了这个字段，TexValue 恒为 false，
    /// 于是下游的 is_force_translucent 永远返回 false，特性静默失效。
    /// 这条同时覆盖「解析结果被带入链」这一环。
    #[test]
    fn chain_translucent_is_accumulated_from_model_textures() {
        let table = vec![(
            "minecraft:block/glass",
            model_tex("", &[("all", "block/glass")], vec![elem(0.0)], true),
        )];
        let mut l = loader(&table);
        let ch = build_chain("block/glass", "minecraft", &mut l);
        assert!(
            ch.force_translucent,
            "模型声明了 force_translucent，链上标志应为真"
        );
        assert!(
            ch.textures.get("all").unwrap().force_translucent,
            "对应变量的标记也应为真"
        );
    }

    /// 普通模型不应把链级标志置位。
    #[test]
    fn chain_translucent_stays_false_for_plain_model() {
        let table = vec![(
            "minecraft:block/stone",
            model("", &[("all", "block/stone")], vec![elem(0.0)]),
        )];
        let mut l = loader(&table);
        let ch = build_chain("block/stone", "minecraft", &mut l);
        assert!(!ch.force_translucent, "普通模型不应被判为半透明");
    }

    /// 父级声明过、子级未声明时，链级标志仍应为真。
    ///
    /// 「链上任意一级声明过即为真」是本模块写在文件头的约定，
    /// 子级覆盖贴图变量时不能把它抹掉。
    #[test]
    fn chain_translucent_survives_child_override() {
        let table = vec![
            (
                "minecraft:block/cube",
                model_tex("", &[("all", "block/glass")], vec![elem(0.0)], true),
            ),
            // 子级覆盖了 all，但自身没有 force_translucent
            ("minecraft:block/door", model("block/cube", &[("all", "block/wood")], vec![])),
        ];
        let mut l = loader(&table);
        let ch = build_chain("block/door", "minecraft", &mut l);
        assert!(
            ch.force_translucent,
            "父级声明过，子级覆盖贴图后链级标志仍应为真"
        );
    }

    #[test]
    fn parent_textures_are_inherited_when_not_overridden() {
        let table = vec![
            (
                "minecraft:block/cube",
                model("", &[("all", "block/iron"), ("particle", "block/iron")], vec![elem(0.0)]),
            ),
            ("minecraft:block/door", model("block/cube", &[("all", "block/wood")], vec![])),
        ];
        let mut l = loader(&table);
        let ch = build_chain("block/door", "minecraft", &mut l);
        assert_eq!(ch.textures.get("all").unwrap().sprite, "block/wood");
        assert_eq!(
            ch.textures.get("particle").unwrap().sprite, "block/iron",
            "未被覆盖的父级变量应继承"
        );
    }

    #[test]
    fn elements_come_from_nearest_level_that_has_them() {
        let table = vec![
            ("minecraft:block/base", model("", &[], vec![elem(1.0)])),
            ("minecraft:block/mid", model("block/base", &[], vec![])),
            ("minecraft:block/leaf", model("block/mid", &[], vec![])),
        ];
        let mut l = loader(&table);
        let ch = build_chain("block/leaf", "minecraft", &mut l);
        assert_eq!(ch.elements.len(), 1);
        assert_eq!(ch.elements[0].from[1], 1.0, "应取到 base 的 elements");
    }

    #[test]
    fn child_elements_replace_parent_entirely() {
        let table = vec![
            ("minecraft:block/base", model("", &[], vec![elem(1.0), elem(2.0)])),
            ("minecraft:block/leaf", model("block/base", &[], vec![elem(9.0)])),
        ];
        let mut l = loader(&table);
        let ch = build_chain("block/leaf", "minecraft", &mut l);
        assert_eq!(ch.elements.len(), 1, "子级有 elements 时应整体替换，不逐元素合并");
        assert_eq!(ch.elements[0].from[1], 9.0);
    }

    #[test]
    fn missing_parent_is_tolerated() {
        // 父模型不存在时按无父级处理，本级的定义仍然有效
        let table = vec![(
            "minecraft:block/orphan",
            model("block/does_not_exist", &[("all", "block/x")], vec![elem(3.0)]),
        )];
        let mut l = loader(&table);
        let ch = build_chain("block/orphan", "minecraft", &mut l);
        assert_eq!(ch.textures.get("all").unwrap().sprite, "block/x");
        assert_eq!(ch.elements.len(), 1);
    }

    #[test]
    fn missing_start_model_yields_empty_chain() {
        let table: Vec<(&str, ModelDef)> = Vec::new();
        let mut l = loader(&table);
        let ch = build_chain("block/nope", "minecraft", &mut l);
        assert!(ch.textures.is_empty());
        assert!(ch.elements.is_empty());
    }

    #[test]
    fn cyclic_parent_terminates() {
        // A 继承 B、B 继承 A：不能无限递归
        let table = vec![
            ("minecraft:a", model("b", &[("x", "block/1")], vec![])),
            ("minecraft:b", model("a", &[("y", "block/2")], vec![])),
        ];
        let mut l = loader(&table);
        let ch = build_chain("a", "minecraft", &mut l);
        // 只要求能终止并返回，不规定具体取到哪一级的数据
        assert!(ch.textures.contains_key("x") || ch.textures.contains_key("y"));
    }

    #[test]
    fn deep_chain_terminates_at_limit() {
        // 构造超过上限的链，必须能终止
        let mut table: Vec<(String, ModelDef)> = Vec::new();
        let n = MAX_DEPTH + 5;
        for i in 0..n {
            let parent = if i + 1 < n { format!("block/m{}", i + 1) } else { String::new() };
            table.push((
                format!("minecraft:block/m{i}"),
                model(&parent, &[("all", "block/x")], vec![]),
            ));
        }
        let pairs: Vec<(&str, ModelDef)> = table.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
        let mut l = loader(&pairs);
        let _ = build_chain("block/m0", "minecraft", &mut l); // 不 panic 即可
    }

    #[test]
    fn resolves_variable_to_literal_path() {
        let mut t = HashMap::new();
        t.insert(
            "all".to_string(),
            TexValue { sprite: "minecraft:block/stone".into(), force_translucent: false },
        );
        assert_eq!(
            resolve_tex_ref(&t, "#all").unwrap(),
            "minecraft:block/stone"
        );
    }

    #[test]
    fn resolves_chained_variables() {
        let mut t = HashMap::new();
        t.insert("a".into(), TexValue { sprite: "#b".into(), force_translucent: false });
        t.insert("b".into(), TexValue { sprite: "#c".into(), force_translucent: false });
        t.insert("c".into(), TexValue { sprite: "block/final".into(), force_translucent: false });
        assert_eq!(resolve_tex_ref(&t, "#a").unwrap(), "block/final");
    }

    #[test]
    fn literal_path_is_returned_as_is() {
        let t = HashMap::new();
        assert_eq!(resolve_tex_ref(&t, "block/stone").unwrap(), "block/stone");
    }

    #[test]
    fn variable_cycle_is_rejected() {
        let mut t = HashMap::new();
        t.insert("a".into(), TexValue { sprite: "#b".into(), force_translucent: false });
        t.insert("b".into(), TexValue { sprite: "#a".into(), force_translucent: false });
        assert!(resolve_tex_ref(&t, "#a").is_none(), "成环应返回 None");
    }

    #[test]
    fn unknown_variable_is_rejected() {
        let t = HashMap::new();
        assert!(resolve_tex_ref(&t, "#missing").is_none());
    }

    #[test]
    fn empty_sprite_is_rejected() {
        let mut t = HashMap::new();
        t.insert("a".into(), TexValue { sprite: String::new(), force_translucent: false });
        assert!(resolve_tex_ref(&t, "#a").is_none(), "指向空值应拒绝");
    }

    #[test]
    fn self_reference_is_rejected() {
        let mut t = HashMap::new();
        t.insert("a".into(), TexValue { sprite: "#a".into(), force_translucent: false });
        assert!(resolve_tex_ref(&t, "#a").is_none(), "自引用应被识别为成环");
    }

    #[test]
    fn force_translucent_is_found_along_chain() {
        let mut t = HashMap::new();
        t.insert("a".into(), TexValue { sprite: "#b".into(), force_translucent: false });
        t.insert("b".into(), TexValue { sprite: "block/glass".into(), force_translucent: true });
        assert!(is_force_translucent(&t, "#a"), "链上任意一级声明即应为真");
    }

    #[test]
    fn force_translucent_false_for_plain_texture() {
        let mut t = HashMap::new();
        t.insert("a".into(), TexValue { sprite: "block/stone".into(), force_translucent: false });
        assert!(!is_force_translucent(&t, "#a"));
        // 字面路径无从判断，按否处理
        assert!(!is_force_translucent(&t, "block/stone"));
    }

    #[test]
    fn force_translucent_cycle_terminates() {
        let mut t = HashMap::new();
        t.insert("a".into(), TexValue { sprite: "#b".into(), force_translucent: false });
        t.insert("b".into(), TexValue { sprite: "#a".into(), force_translucent: false });
        assert!(!is_force_translucent(&t, "#a"), "成环时不应死循环");
    }
}
