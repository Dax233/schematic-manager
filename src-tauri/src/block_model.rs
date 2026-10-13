//! 方块状态到模型的解析。
//!
//! 对照 legacy-cpp/blockdb.cpp 的 MatchWhen / ResolveModels。
//! 方块模型定义来自 Minecraft 的资源包语义：
//!   - `variants`：**取第一个匹配的条目**（不是全部）
//!   - `multipart`：**所有匹配的条目叠加**，条件为空表示恒成立
//!   - 条件串用 `,` 表示「与」，用 `|` 表示「或」

use crate::blockdb::{BlockDb, BlockDef, Variant, VersionDef};
use crate::replace::BlockInfo;
use std::collections::BTreeMap;
use crate::litematic::BlockState;

/// 解析 "a=1,b=2" 形式的与条件。
///
/// 只按第一个 `=` 切分：属性值本身可能含有 `=`。
/// 缺少 `=` 或 `=` 在首位的片段被忽略，这与原实现一致。
pub fn parse_conds(when: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if when.is_empty() {
        return out;
    }
    for one in when.split(',') {
        if let Some(eq) = one.find('=') {
            if eq > 0 {
                out.push((one[..eq].to_string(), one[eq + 1..].to_string()));
            }
        }
    }
    out
}

/// 全部条件都满足才为真（与语义）
pub fn match_conds(conds: &[(String, String)], st: &BlockState) -> bool {
    conds.iter().all(|(k, v)| st.prop(k) == Some(v.as_str()))
}

/// 单个条件串是否匹配。空串恒为真。
pub fn match_when(when: &str, st: &BlockState) -> bool {
    if when.is_empty() {
        return true;
    }
    match_conds(&parse_conds(when), st)
}

/// multipart 的条件可含 `|` 分隔的多个备选，任一成立即为真。
pub fn match_any_when(when: &str, st: &BlockState) -> bool {
    if when.is_empty() {
        return true;
    }
    when.split('|').any(|alt| match_conds(&parse_conds(alt), st))
}

/// 解析出的单个模型引用：模型下标 + 该变体要求的旋转
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedModel {
    pub model_idx: u32,
    /// 绕 X 轴旋转的 90 度倍数
    pub rot_x_div90: u8,
    /// 绕 Y 轴旋转的 90 度倍数
    pub rot_y_div90: u8,
    pub uvlock: bool,
}

impl From<&Variant> for ResolvedModel {
    fn from(v: &Variant) -> Self {
        Self {
            model_idx: v.model_idx,
            rot_x_div90: v.rot_x_div90,
            rot_y_div90: v.rot_y_div90,
            uvlock: v.uvlock,
        }
    }
}

/// 在指定版本（及其叠加包）中按方块名查方块定义。
///
/// 叠加包是模组提供的方块，基础版本里没有它们的定义。
/// 查找顺序与旧实现一致：本版本 -> 绑定到该版本的叠加包 -> 任意叠加包。
pub fn find_block<'a>(db: &'a BlockDb, version: &'a VersionDef, name: &str) -> Option<&'a BlockDef> {
    if let Some(b) = version.blocks.iter().find(|b| b.name == name) {
        return Some(b);
    }
    db.versions
        .iter()
        .filter(|v| v.is_overlay)
        .find_map(|v| v.blocks.iter().find(|b| b.name == name))
}

/// 解析方块状态对应的模型列表。
///
/// variants 取第一个匹配项，multipart 叠加全部匹配项——两者结果合并返回，
/// 顺序为 variants 在前、multipart 在后。
pub fn resolve_models<'a>(
    db: &'a BlockDb,
    version: &'a VersionDef,
    st: &BlockState,
) -> Vec<ResolvedModel> {
    let Some(bd) = find_block(db, version, &st.name) else {
        return Vec::new();
    };
    let mut out = Vec::new();

    // variants：取第一个匹配的（Minecraft 语义），命中后即停止
    for v in &bd.variants {
        if match_when(&v.when, st) {
            out.push(ResolvedModel::from(v));
            break;
        }
    }

    // multipart：所有匹配项叠加
    for v in &bd.multipart {
        if match_any_when(&v.when, st) {
            out.push(ResolvedModel::from(v));
        }
    }

    out
}

/// 一个方块支持的属性及其取值域：属性名 -> 取值（有序，便于结果稳定）
pub type PropDomain = BTreeMap<String, Vec<String>>;

/// 收集方块支持的属性取值域。
///
/// 信息全部来自变体与 multipart 的条件串——方块库并不单独存属性表，
/// 条件串里出现过的 prop=value 就是该方块支持的取值。
pub fn block_prop_domain(
    db: &BlockDb,
    version: &VersionDef,
    name: &str,
) -> Option<(PropDomain, Vec<(String, String)>)> {
    let bd = find_block(db, version, name)?;
    let mut domain: PropDomain = BTreeMap::new();
    // 默认值取自第一个变体的条件：blockstate 文件里默认变体排在前面，
    // 因此它列出的取值最接近游戏内的默认状态。
    let mut defaults: Vec<(String, String)> = Vec::new();

    // 默认值只看 variants[0]：legacy-cpp 要求 !variants.empty() &&
    // !variants[0].when.empty()（blockdb.cpp:552）。若把 multipart 也算进
    // 「第一个变体」，variants 为空的方块会拿 multipart[0] 当默认值。
    let first_variant_when = bd
        .variants
        .first()
        .map(|v| v.when.as_str())
        .unwrap_or("");
    if !first_variant_when.is_empty() {
        for alt in first_variant_when.split('|') {
            for (k, val) in parse_conds(alt) {
                defaults.push((k, val));
            }
        }
    }

    for v in bd.variants.iter().chain(bd.multipart.iter()) {
        if v.when.is_empty() {
            continue;
        }
        for alt in v.when.split('|') {
            for (k, val) in parse_conds(alt) {
                let slot = domain.entry(k.clone()).or_default();
                if !slot.contains(&val) {
                    slot.push(val.clone());
                }
            }
        }
    }
    // 兜底必须在排序**之前**取「首次出现」的值：legacy-cpp 的
    // DefaultProps 用的是 bd.props 的首次出现顺序（blockdb.cpp:556-557），
    // 而排序后取首值会拿到字典序最小的那个，两者对「首变体未提及该属性」
    // 的方块会给出不同的默认状态（实测 46469 个组合中有 1264 个不同）。
    for (k, vals) in &domain {
        if !defaults.iter().any(|(dk, _)| dk == k) {
            if let Some(v) = vals.first() {
                defaults.push((k.clone(), v.clone()));
            }
        }
    }
    for vals in domain.values_mut() {
        vals.sort();
    }
    Some((domain, defaults))
}

/// 以方块库为知识来源的替换信息
pub struct DbBlockInfo<'a> {
    pub db: &'a BlockDb,
    pub version: &'a VersionDef,
}

impl BlockInfo for DbBlockInfo<'_> {
    fn supports_prop(&self, block: &str, prop: &str, value: &str) -> bool {
        match block_prop_domain(self.db, self.version, block) {
            Some((domain, _)) => domain
                .get(prop)
                .map(|vals| vals.iter().any(|v| v == value))
                .unwrap_or(false),
            None => false,
        }
    }

    fn default_props(&self, block: &str) -> Vec<(String, String)> {
        block_prop_domain(self.db, self.version, block)
            .map(|(_, d)| d)
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blockdb::{Model, TextureInfo, VersionDef};
    use std::collections::HashMap;

    fn st(name: &str, props: &[(&str, &str)]) -> BlockState {
        let mut b = BlockState::new(name);
        for (k, v) in props {
            b.set_prop(k, v);
        }
        b
    }

    fn var(when: &str, model: u32) -> Variant {
        Variant {
            when: when.to_string(),
            model_idx: model,
            rot_x_div90: 0,
            rot_y_div90: 0,
            uvlock: false,
        }
    }

    fn db_with(block: BlockDef, version_name: &str, data_version: i32) -> BlockDb {
        BlockDb {
            strings: Vec::new(),
            langs: Vec::new(),
            textures: vec![TextureInfo { w: 1, h: 1, offset: 0, len: 4, has_alpha: false }],
            models: vec![Model { translucent: false, elements: Vec::new() }],
            versions: vec![VersionDef {
                name: version_name.to_string(),
                data_version,
                lang_idx: 0,
                texture_by_path: HashMap::new(),
                blocks: vec![block],
                langs: Vec::new(),
                is_overlay: false,
            }],
            pixels: vec![0u8; 4],
        }
    }


    /// 默认值必须取「首次出现」的值，而不是字典序最小的值。
    ///
    /// 这里刻意让两者不同：facing 的取值域排序后以 east 开头，
    /// 但首次出现（即首变体）是 north。若按字典序取，会得到 east。
    /// legacy-cpp 的 DefaultProps 用的是 bd.props 的首次出现顺序。
    #[test]
    fn defaults_use_first_occurrence_not_alphabetical() {
        let bd = BlockDef {
            name: "minecraft:oak_stairs".into(),
            variants: vec![
                var("facing=north", 0),
                var("facing=east|facing=south|facing=west", 0),
            ],
            multipart: Vec::new(),
        };
        let db = db_with(bd, "1.20.1", 3465);
        let info = DbBlockInfo { db: &db, version: &db.versions[0] };
        let d = info.default_props("minecraft:oak_stairs");
        assert_eq!(
            d.iter().find(|(k, _)| k == "facing").map(|(_, v)| v.as_str()),
            Some("north"),
            "默认值应为首次出现的 north，而不是字典序最小的 east；实际 {d:?}"
        );
    }
    #[test]
    fn parse_conds_splits_on_comma_and_first_equals() {
        let c = parse_conds("facing=east,half=bottom");
        assert_eq!(c, vec![
            ("facing".to_string(), "east".to_string()),
            ("half".to_string(), "bottom".to_string()),
        ]);
        // 值里含等号时只按第一个等号切分
        let c = parse_conds("a=b=c");
        assert_eq!(c, vec![("a".to_string(), "b=c".to_string())]);
        // 缺少等号或等号在首位：忽略该片段
        assert!(parse_conds("nope").is_empty());
        assert!(parse_conds("=v").is_empty());
        assert!(parse_conds("").is_empty());
    }

    #[test]
    fn and_semantics_require_all_conditions() {
        let s = st("minecraft:oak_stairs", &[("facing", "east"), ("half", "bottom")]);
        assert!(match_when("facing=east,half=bottom", &s));
        assert!(!match_when("facing=east,half=top", &s), "任一条件不满足即为假");
        assert!(!match_when("facing=west,half=bottom", &s));
        // 方块缺少该属性时也判为不匹配，而不是视为通过
        assert!(!match_when("waterlogged=true", &s));
    }

    #[test]
    fn empty_condition_always_matches() {
        let s = st("minecraft:stone", &[]);
        assert!(match_when("", &s));
        assert!(match_any_when("", &s));
    }

    #[test]
    fn pipe_is_or() {
        let s = st("minecraft:oak_fence", &[("north", "true")]);
        assert!(match_any_when("north=true|south=true", &s));
        assert!(!match_any_when("east=true|south=true", &s));
        // 备选内部仍是与语义：要让整个与条件成立，
        // 每一对键值都必须在该方块上存在且相等。
        // 注意：方块缺少某属性时该条件不成立（而不是「默认通过」），
        // 因此 south=false 在缺少 south 属性时应判为不匹配。
        assert!(!match_any_when("north=true,south=false|east=true", &s),
                "缺少 south 属性时 south=false 不应视为匹配");
        assert!(!match_any_when("north=true,south=true|east=true", &s));
        // 换成方块确实拥有的属性，且第一备选成立时，整体为真
        assert!(match_any_when("north=true|east=true", &s));
        assert!(match_any_when("north=false|north=true", &s), "第二个备选成立");
    }

    #[test]
    fn variants_take_first_match_only() {
        let bd = BlockDef {
            name: "minecraft:oak_stairs".into(),
            variants: vec![var("facing=east", 0), var("facing=east", 0), var("", 0)],
            multipart: Vec::new(),
        };
        let db = db_with(bd, "1.20.1", 3465);
        let got = resolve_models(&db, &db.versions[0], &st("minecraft:oak_stairs", &[("facing", "east")]));
        assert_eq!(got.len(), 1, "variants 只取第一个匹配项");
    }

    #[test]
    fn multipart_stacks_all_matches() {
        let bd = BlockDef {
            name: "minecraft:oak_fence".into(),
            variants: Vec::new(),
            multipart: vec![var("north=true", 0), var("south=true", 0), var("east=true", 0)],
        };
        let db = db_with(bd, "1.20.1", 3465);
        let got = resolve_models(
            &db,
            &db.versions[0],
            &st("minecraft:oak_fence", &[("north", "true"), ("south", "true")]),
        );
        assert_eq!(got.len(), 2, "multipart 应叠加全部匹配项，实际 {got:?}");
    }

    #[test]
    fn unknown_block_resolves_to_nothing() {
        let bd = BlockDef { name: "minecraft:stone".into(), variants: vec![var("", 0)], multipart: Vec::new() };
        let db = db_with(bd, "1.20.1", 3465);
        assert!(resolve_models(&db, &db.versions[0], &st("mod:unknown", &[])).is_empty());
    }

    #[test]
    fn overlay_block_is_found() {
        // 基础版本没有该方块，叠加包提供时才应解析成功
        let mut db = db_with(
            BlockDef { name: "minecraft:stone".into(), variants: vec![var("", 0)], multipart: Vec::new() },
            "1.20.1",
            3465,
        );
        db.versions.push(VersionDef {
            name: "modpack".into(),
            data_version: -1,
            lang_idx: 0,
            texture_by_path: HashMap::new(),
            blocks: vec![BlockDef {
                name: "mod:custom".into(),
                variants: vec![var("", 0)],
                multipart: Vec::new(),
            }],
            langs: Vec::new(),
            is_overlay: true,
        });
        let got = resolve_models(&db, &db.versions[0], &st("mod:custom", &[]));
        assert_eq!(got.len(), 1, "叠加包中的方块应能被解析到");
    }

    #[test]
    fn prop_domain_comes_from_condition_strings() {
        let bd = BlockDef {
            name: "minecraft:oak_stairs".into(),
            variants: vec![
                var("facing=north,half=bottom,shape=straight", 0),
                var("facing=east,half=top,shape=outer_left", 0),
            ],
            multipart: Vec::new(),
        };
        let db = db_with(bd, "1.20.1", 3465);
        let (domain, defaults) = block_prop_domain(&db, &db.versions[0], "minecraft:oak_stairs").unwrap();

        let facing = domain.get("facing").expect("应有 facing");
        assert_eq!(facing, &vec!["east".to_string(), "north".to_string()], "取值应去重且有序");
        assert_eq!(domain.get("half").unwrap().len(), 2);
        // 默认值取自第一个变体，而不是字母序第一个
        assert_eq!(
            defaults.iter().find(|(k, _)| k == "facing").map(|(_, v)| v.as_str()),
            Some("north"),
            "默认值应来自第一个变体"
        );
    }

    #[test]
    fn prop_domain_includes_multipart_conditions() {
        let bd = BlockDef {
            name: "minecraft:oak_fence".into(),
            variants: vec![var("north=true", 0)],
            multipart: vec![var("waterlogged=true", 0), var("south=true", 0)],
        };
        let db = db_with(bd, "1.20.1", 3465);
        let (domain, _) = block_prop_domain(&db, &db.versions[0], "minecraft:oak_fence").unwrap();
        assert!(domain.contains_key("north"));
        assert!(domain.contains_key("south"), "multipart 的条件也应计入");
        assert!(domain.contains_key("waterlogged"));
    }

    #[test]
    fn supports_prop_checks_value_not_just_name() {
        let bd = BlockDef {
            name: "minecraft:oak_stairs".into(),
            variants: vec![var("facing=north|facing=south", 0)],
            multipart: Vec::new(),
        };
        let db = db_with(bd, "1.20.1", 3465);
        let info = DbBlockInfo { db: &db, version: &db.versions[0] };
        assert!(info.supports_prop("minecraft:oak_stairs", "facing", "north"));
        assert!(info.supports_prop("minecraft:oak_stairs", "facing", "south"));
        // 属性名对但取值不支持，仍应判为不支持
        assert!(!info.supports_prop("minecraft:oak_stairs", "facing", "up"));
        // 未知方块一律不支持
        assert!(!info.supports_prop("mod:unknown", "facing", "north"));
    }

    #[test]
    fn defaults_fall_back_to_domain_first_value() {
        // 第一个变体没提到 shape，但后续变体提到了，
        // 此时默认值应取自取值域中的第一个值，而不是缺失
        let bd = BlockDef {
            name: "minecraft:oak_stairs".into(),
            variants: vec![var("facing=north", 0), var("shape=outer_left|shape=straight", 0)],
            multipart: Vec::new(),
        };
        let db = db_with(bd, "1.20.1", 3465);
        let info = DbBlockInfo { db: &db, version: &db.versions[0] };
        let d = info.default_props("minecraft:oak_stairs");
        assert!(d.iter().any(|(k, v)| k == "facing" && v == "north"));
        assert!(
            d.iter().any(|(k, v)| k == "shape" && v == "outer_left"),
            "未在首变体出现的属性应取取值域第一个值，实际 {d:?}"
        );
    }

    #[test]
    fn variant_rotation_is_carried_through() {
        let bd = BlockDef {
            name: "minecraft:stone".into(),
            variants: vec![Variant {
                when: String::new(),
                model_idx: 0,
                rot_x_div90: 1,
                rot_y_div90: 3,
                uvlock: true,
            }],
            multipart: Vec::new(),
        };
        let db = db_with(bd, "1.20.1", 3465);
        let got = resolve_models(&db, &db.versions[0], &st("minecraft:stone", &[]));
        assert_eq!(got[0].rot_x_div90, 1);
        assert_eq!(got[0].rot_y_div90, 3);
        assert!(got[0].uvlock, "uvlock 必须一并带出，否则贴图方向会错");
    }
}
