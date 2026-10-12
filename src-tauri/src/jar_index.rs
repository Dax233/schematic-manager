//! jar 内的资源索引。
//!
//! 用途：从 Minecraft 客户端/模组 jar 的条目名里找出各命名空间下的
//! blockstates、models 与 textures，供后续解析与构建资源包使用。
//!
//! 对照 legacy-cpp/packbuild.cpp 的 BuildFromJar 前半段（收集命名空间与
//! 建立模型 id 到条目路径的映射）。
//!
//! 纯字符串处理，不涉及文件 IO，因此可完整测试。

use std::collections::{BTreeMap, BTreeSet};

/// assets 下的固定子目录名
const DIR_BLOCKSTATES: &str = "blockstates";
const DIR_MODELS: &str = "models";
const DIR_TEXTS: &str = "textures";

/// 从条目名里取出 `assets/<ns>/` 之后的命名空间。
///
/// 只认 `assets/` 开头且确有命名空间段的名字，其余（META-INF、根目录文件、
/// 以及 `assets/` 下直接放文件的情形）返回 None。
fn namespace_of(entry: &str) -> Option<&str> {
    let rest = entry.strip_prefix("assets/")?;
    let (ns, _) = rest.split_once('/')?;
    if ns.is_empty() {
        return None;
    }
    Some(ns)
}

/// 从 `assets/<ns>/<dir>/<path>.json` 里取出 `<path>`；扩展名必须匹配
fn path_under(entry: &str, ns: &str, dir: &str, ext: &str) -> Option<String> {
    let prefix = format!("assets/{ns}/{dir}/");
    let rest = entry.strip_prefix(&prefix)?;
    let path = rest.strip_suffix(ext)?;
    if path.is_empty() {
        return None;
    }
    Some(path.to_string())
}

/// 一个命名空间下索引到的资源
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NamespaceAssets {
    /// blockstates 的条目名（保持输入顺序）
    pub blockstates: Vec<String>,
    /// 模型 id -> jar 内条目路径。id 形如 `minecraft:block/stone`
    pub models: BTreeMap<String, String>,
    /// 贴图条目名（`assets/<ns>/textures/**` 下的 .png）
    pub textures: Vec<String>,
}

impl NamespaceAssets {
    pub fn is_empty(&self) -> bool {
        self.blockstates.is_empty() && self.models.is_empty() && self.textures.is_empty()
    }
}

/// jar 内资源的整体索引
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct JarIndex {
    /// 命名空间 -> 该命名空间下的资源
    pub namespaces: BTreeMap<String, NamespaceAssets>,
}

impl JarIndex {
    /// 含有 blockstates 的命名空间，按名称排序（保证构建结果可复现）
    pub fn blockstate_namespaces(&self) -> Vec<&str> {
        self.namespaces
            .iter()
            .filter(|(_, v)| !v.blockstates.is_empty())
            .map(|(k, _)| k.as_str())
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.namespaces.values().all(|v| v.is_empty())
    }
}

/// 由条目名列表建立索引。
///
/// `entries` 的顺序会影响各列表内的顺序，因此调用方应先排序——
/// jar 的中央目录本身是有序的，但 zip 规范不保证，排序后结果才可复现。
pub fn build(entries: &[String]) -> JarIndex {
    let mut idx = JarIndex::default();

    for e in entries {
        let Some(ns) = namespace_of(e) else { continue };
        let slot = idx.namespaces.entry(ns.to_string()).or_default();

        if let Some(p) = path_under(e, ns, DIR_BLOCKSTATES, ".json") {
            slot.blockstates.push(e.clone());
            let _ = p;
            continue;
        }
        if let Some(p) = path_under(e, ns, DIR_MODELS, ".json") {
            // 模型 id 用命名空间限定；另存一份不带命名空间的短名，
            // 因为 parent 常写作 "block/cube" 而不带 minecraft 前缀
            slot.models.insert(format!("{ns}:{p}"), e.clone());
            if ns == "minecraft" {
                slot.models.insert(p, e.clone());
            }
            continue;
        }
        // 贴图：只收 .png，且必须位于 textures 下
        if e.starts_with(&format!("assets/{ns}/{DIR_TEXTS}/"))
            && e.to_ascii_lowercase().ends_with(".png")
        {
            slot.textures.push(e.clone());
        }
    }

    // 各列表排序，使结果与输入顺序无关
    for v in idx.namespaces.values_mut() {
        v.blockstates.sort();
        v.textures.sort();
    }
    idx
}

/// 把贴图条目名转成资源路径（去掉 `assets/<ns>/textures/` 前缀与扩展名）。
///
/// blockstate 与 model 里引用贴图用的就是这个形式，例如
/// `assets/minecraft/textures/block/stone.png` -> `block/stone`。
pub fn texture_path(entry: &str) -> Option<String> {
    let ns = namespace_of(entry)?;
    let prefix = format!("assets/{ns}/textures/");
    let rest = entry.strip_prefix(&prefix)?;
    let stem = rest.strip_suffix(".png").or_else(|| rest.strip_suffix(".PNG"))?;
    if stem.is_empty() {
        return None;
    }
    Some(stem.to_string())
}

/// 把模型/贴图引用规范成 `<ns>:<path>` 形式。
///
/// 引用可能已是全名（`minecraft:block/stone`），也可能只是路径
/// （`block/stone`），后者要补上当前命名空间。
pub fn qualify(reference: &str, cur_ns: &str) -> Option<String> {
    if reference.is_empty() {
        return None;
    }
    match reference.split_once(':') {
        Some((ns, path)) => {
            if ns.is_empty() || path.is_empty() {
                None
            } else {
                Some(format!("{ns}:{path}"))
            }
        }
        None => Some(format!("{cur_ns}:{reference}")),
    }
}

/// 收集全部命名空间（含只有贴图没有 blockstates 的）
pub fn namespaces(entries: &[String]) -> BTreeSet<String> {
    entries.iter().filter_map(|e| namespace_of(e)).map(|s| s.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn indexes_blockstates_models_and_textures() {
        let entries = s(&[
            "assets/minecraft/blockstates/stone.json",
            "assets/minecraft/models/block/stone.json",
            "assets/minecraft/textures/block/stone.png",
        ]);
        let idx = build(&entries);
        let ns = idx.namespaces.get("minecraft").unwrap();
        assert_eq!(ns.blockstates, s(&["assets/minecraft/blockstates/stone.json"]));
        assert_eq!(
            ns.models.get("minecraft:block/stone").unwrap(),
            "assets/minecraft/models/block/stone.json"
        );
        assert_eq!(ns.textures, s(&["assets/minecraft/textures/block/stone.png"]));
    }

    #[test]
    fn model_id_map_includes_unqualified_short_name_for_minecraft() {
        // parent 常写作 "block/cube"，不带命名空间，因此短名必须可查
        let entries = s(&["assets/minecraft/models/block/cube.json"]);
        let idx = build(&entries);
        let ns = idx.namespaces.get("minecraft").unwrap();
        assert_eq!(
            ns.models.get("block/cube").unwrap(),
            "assets/minecraft/models/block/cube.json"
        );
        assert!(ns.models.contains_key("minecraft:block/cube"));
    }

    #[test]
    fn non_minecraft_namespace_has_no_short_name() {
        // 短名只对 minecraft 有意义，其他命名空间补前缀会歧义
        let entries = s(&["assets/mymod/models/block/x.json"]);
        let idx = build(&entries);
        let ns = idx.namespaces.get("mymod").unwrap();
        assert!(ns.models.contains_key("mymod:block/x"));
        assert!(!ns.models.contains_key("block/x"));
    }

    #[test]
    fn ignores_unrelated_entries() {
        let entries = s(&[
            "META-INF/MANIFEST.MF",
            "pack.mcmeta",
            "assets/",
            "assets/minecraft/",
            "assets/minecraft/sounds/a.ogg",
            "assets/minecraft/lang/en_us.json",
            "assets/minecraft/textures/block/stone.png.bak",
        ]);
        let idx = build(&entries);
        // 只有 textures 目录下的 .png 才算，上面都不满足
        assert!(idx.is_empty(), "实际 {:?}", idx.namespaces);
    }

    #[test]
    fn textures_outside_textures_dir_are_ignored() {
        let entries = s(&[
            "assets/minecraft/blockstates/x.png",
            "assets/minecraft/models/a.png",
        ]);
        let idx = build(&entries);
        assert!(idx.is_empty());
    }

    #[test]
    fn only_json_under_blockstates_and_models_counts() {
        let entries = s(&[
            "assets/minecraft/blockstates/x.txt",
            "assets/minecraft/models/y.json.txt",
        ]);
        let idx = build(&entries);
        assert!(idx.is_empty(), "扩展名不符不应计入");
    }

    #[test]
    fn multiple_namespaces_are_separated() {
        let entries = s(&[
            "assets/minecraft/blockstates/stone.json",
            "assets/mymod/blockstates/thing.json",
            "assets/mymod/models/block/thing.json",
        ]);
        let idx = build(&entries);
        assert_eq!(idx.blockstate_namespaces(), vec!["minecraft", "mymod"]);
        assert_eq!(idx.namespaces.len(), 2);
        // 各命名空间互不干扰
        assert!(idx.namespaces["mymod"].models.contains_key("mymod:block/thing"));
        assert!(!idx.namespaces["mymod"].models.contains_key("minecraft:block/stone"));
    }

    #[test]
    fn blockstate_namespaces_only_lists_those_with_blockstates() {
        // 只有贴图的命名空间不应被当作可构建的方块来源
        let entries = s(&[
            "assets/onlytextures/textures/block/x.png",
            "assets/minecraft/blockstates/a.json",
        ]);
        let idx = build(&entries);
        assert_eq!(idx.blockstate_namespaces(), vec!["minecraft"]);
        assert!(idx.namespaces.contains_key("onlytextures"), "但其资源仍被索引");
    }

    #[test]
    fn output_is_independent_of_input_order() {
        let a = s(&[
            "assets/minecraft/blockstates/z.json",
            "assets/minecraft/blockstates/a.json",
            "assets/minecraft/textures/block/z.png",
            "assets/minecraft/textures/block/a.png",
        ]);
        let mut b = a.clone();
        b.reverse();
        let ia = build(&a);
        let ib = build(&b);
        assert_eq!(ia, ib, "排序后结果应与输入顺序无关");
        assert_eq!(
            ia.namespaces["minecraft"].blockstates[0],
            "assets/minecraft/blockstates/a.json"
        );
    }

    #[test]
    fn texture_path_strips_prefix_and_extension() {
        assert_eq!(
            texture_path("assets/minecraft/textures/block/stone.png").unwrap(),
            "block/stone"
        );
        // 嵌套目录也要保留
        assert_eq!(
            texture_path("assets/mymod/textures/block/deep/x.png").unwrap(),
            "block/deep/x"
        );
        assert!(texture_path("assets/minecraft/models/block/x.json").is_none());
        assert!(texture_path("assets/minecraft/textures/a.gif").is_none());
    }

    #[test]
    fn uppercase_png_extension_is_accepted() {
        // 有些资源包用大写扩展名
        let entries = s(&["assets/minecraft/textures/block/x.PNG"]);
        let idx = build(&entries);
        assert_eq!(idx.namespaces["minecraft"].textures.len(), 1);
        assert_eq!(
            texture_path("assets/minecraft/textures/block/x.PNG").unwrap(),
            "block/x"
        );
    }

    #[test]
    fn qualify_adds_current_namespace() {
        assert_eq!(qualify("block/stone", "minecraft").unwrap(), "minecraft:block/stone");
        assert_eq!(qualify("mymod:block/x", "minecraft").unwrap(), "mymod:block/x");
        assert!(qualify("", "minecraft").is_none());
        assert!(qualify("mymod:", "minecraft").is_none(), "空路径应拒绝");
    }

    #[test]
    fn namespaces_collects_all() {
        let entries = s(&[
            "assets/a/blockstates/x.json",
            "assets/b/textures/block/y.png",
            "META-INF/x",
        ]);
        let ns = namespaces(&entries);
        assert_eq!(ns.len(), 2);
        assert!(ns.contains("a") && ns.contains("b"));
    }

    #[test]
    fn empty_input_yields_empty_index() {
        let idx = build(&[]);
        assert!(idx.is_empty());
        assert!(idx.blockstate_namespaces().is_empty());
        assert!(build(&s(&["nothing/here"])).is_empty());
    }
}
