use super::*;

#[test]
fn reports_have_valid_defaults_and_cover_supported_releases() {
    for &(dv, _) in crate::pack::base::SUPPORTED_RELEASES.iter().filter(|(dv, _)| *dv >= 1952) {
        let registry = get(dv).expect("正式版缺少精确注册信息");
        for (domain, defaults) in registry.values() {
            assert_eq!(domain.len(), defaults.len());
            for (key, value) in defaults {
                assert!(domain[key].contains(value));
            }
        }
    }
    assert!(get(3699).is_none(), "快照不能使用邻近正式版的信息");
}

#[test]
fn registry_distinguishes_wall_blocks_and_items() {
    let registry = get(3700).unwrap();
    assert!(registry["water"].0["level"].contains(&"8".into()));
    assert!(registry["bubble_column"].0["drag"].contains(&"true".into()));
    assert!(registry["wither_skeleton_wall_skull"].0.contains_key("facing"));
    assert!(!registry["wither_skeleton_wall_skull"].0.contains_key("rotation"));
    assert!(registry["birch_wall_sign"].0["waterlogged"].contains(&"true".into()));
    assert!(!registry.contains_key("pale_oak_wall_sign"));
    assert!(!registry.contains_key("water_bucket"));
}
