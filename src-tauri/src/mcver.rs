//! Minecraft dataVersion 与游戏版本名的映射。
//!
//! 数据由 legacy-cpp/src/mcver_table.inc 转换而来（原始来源：
//! malilib 的 Schema 枚举 + Minecraft Wiki）。
//! 该表只增不改：新版本追加到末尾，历史条目保持不变。

/// (dataVersion, 版本名)，按 dataVersion 升序。
pub const DATA_VERSIONS: &[(i32, &str)] = &[
    (90, "15w14a"),
    (100, "15w32a"),
    (169, "1.9"),
    (173, "16w13_1.RV-Pre1"),
    (184, "1.9.4"),
    (510, "1.10"),
    (512, "1.10.2"),
    (819, "1.11"),
    (922, "1.11.2"),
    (1139, "1.12"),
    (1241, "1.12.1"),
    (1343, "1.12.2"),
    (1444, "17w43a"),
    (1449, "17w46a"),
    (1451, "17w47a"),
    (1457, "17w50a"),
    (1469, "18w07c"),
    (1481, "18w14b"),
    (1499, "18w22c"),
    (1519, "1.13"),
    (1628, "1.13.1"),
    (1631, "1.13.2"),
    (1901, "18w43a"),
    (1919, "18w50a"),
    (1934, "19w08b"),
    (1943, "19w13_3d_shareware_v1.34"),
    (1945, "19w14b"),
    (1952, "1.14"),
    (1957, "1.14.1"),
    (1963, "1.14.2"),
    (1968, "1.14.3"),
    (1976, "1.14.4"),
    (2200, "19w34a"),
    (2208, "19w40a"),
    (2217, "19w46b"),
    (2225, "1.15"),
    (2227, "1.15.1"),
    (2230, "1.15.2"),
    (2504, "20w06a"),
    (2522, "20w14~"),
    (2525, "20w15a"),
    (2555, "20w22a"),
    (2566, "1.16"),
    (2567, "1.16.1"),
    (2578, "1.16.2"),
    (2580, "1.16.3"),
    (2584, "1.16.4"),
    (2586, "1.16.5"),
    (2681, "20w45a"),
    (2685, "20w49a"),
    (2690, "21w05a"),
    (2699, "21w10a"),
    (2709, "21w15a"),
    (2715, "21w20a"),
    (2724, "1.17"),
    (2730, "1.17.1"),
    (2834, "21w37a"),
    (2839, "21w41a"),
    (2845, "21w44a"),
    (2860, "1.18"),
    (2865, "1.18.1"),
    (2975, "1.18.2"),
    (3066, "22w12a_deep_dark"),
    (3076, "22w13oneBlockAtATime"),
    (3080, "22w11a"),
    (3091, "22w16a"),
    (3096, "22w19a"),
    (3105, "1.19"),
    (3117, "1.19.1"),
    (3120, "1.19.2"),
    (3218, "1.19.3"),
    (3337, "1.19.4"),
    (3442, "23w12a"),
    (3443, "23w13a"),
    (3444, "23w14a_or_b"),
    (3445, "23w15a"),
    (3449, "23w16a"),
    (3453, "23w18a"),
    (3463, "1.20"),
    (3465, "1.20.1"),
    (3567, "23w31a"),
    (3571, "23w35a"),
    (3578, "1.20.2"),
    (3679, "23w40a"),
    (3687, "23w43b"),
    (3691, "23w46a"),
    (3700, "1.20.4"),
    (3801, "23w51a"),
    (3804, "24w03a"),
    (3817, "24w07a"),
    (3819, "24w09a"),
    (3821, "24w10a"),
    (3823, "24w11a"),
    (3824, "24w14potato"),
    (3826, "24w13a"),
    (3827, "24w14a"),
    (3837, "1.20.5"),
    (3940, "24w18a"),
    (3946, "24w21a"),
    (3953, "1.21"),
    (3955, "1.21.1"),
    (4058, "24w33a"),
    (4062, "24w35a"),
    (4065, "24w37a"),
    (4072, "24w40a"),
    (4080, "1.21.2"),
    (4082, "1.21.3"),
    (4174, "24w44a"),
    (4178, "24w46a"),
    (4189, "1.21.4"),
    (4298, "25w02a"),
    (4304, "25w03a"),
    (4319, "25w10a"),
    (4323, "25w14craftmine"),
    (4324, "1.21.5-rc2"),
    (4325, "1.21.5"),
    (4423, "25w16a"),
    (4429, "25w21a"),
    (4435, "1.21.6"),
    (4438, "1.21.7"),
    (4440, "1.21.8"),
    (4534, "25w31a"),
    (4546, "25w36b"),
    (4554, "1.21.9"),
    (4556, "1.21.10"),
    (4657, "25w41a"),
    (4662, "25w46a"),
    (4671, "1.21.11"),
    (4764, "26.1-snapshot-1"),
    (4774, "26.1-snapshot-6"),
    (4786, "26.1"),
    (4788, "26.1.1"),
    (4790, "26.1.2"),
    (4883, "26.2-snapshot-1"),
    (4887, "26.2-snapshot-4"),
    (4891, "26.2-snapshot-7"),
    (4897, "26.2-pre-4"),
    (4903, "26.2"),
    (4998, "26.3-snapshot-1"),
    (4999, "26.3-snapshot-2"),
    (5000, "26w14a"),
    (5001, "26.3-snapshot-3"),
    (5003, "26.3-snapshot-4"),
    (5004, "26.3-snapshot-5"),
    (5005, "26.3-snapshot-6"),
    (5009, "26.3-snapshot-7"),
    (5010, "26.3-snapshot-8"),
    (5011, "26.3-snapshot-9"),
    (5015, "26.3-snapshot-10"),
    (5017, "26.3-pre-1"),
    (5018, "26.3-pre-2"),
    (5019, "26.3-pre-3"),
    (5020, "26.3-rc-1"),
    (5021, "26.3-rc-2"),
    (5022, "26.3-rc-3"),
    (5023, "26.3"),
    (5119, "26.4-snapshot-1"),
    (5120, "26.4-snapshot-2"),
    (5122, "26.4-snapshot-3"),
];

/// 由 dataVersion 推断游戏版本名。
///
/// 取「不大于给定 dataVersion 的最大条目」——同一个正式版的生命周期内
/// 常有若干快照，投影里记录的 dataVersion 未必与正式版完全相等。
/// 低于表中最小值时返回空串，由调用方决定如何展示。
pub fn version_name_from_data_version(dv: i32) -> String {
    if dv <= 0 {
        return String::new();
    }
    let mut found: Option<&str> = None;
    for (v, name) in DATA_VERSIONS {
        if *v <= dv {
            found = Some(name);
        } else {
            break;
        }
    }
    found.unwrap_or("").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_sorted_and_unique() {
        for w in DATA_VERSIONS.windows(2) {
            assert!(w[0].0 < w[1].0, "dataVersion 未严格递增: {:?} {:?}", w[0], w[1]);
        }
    }

    #[test]
    fn lookup_exact_below_and_between() {
        let (first_dv, first_name) = DATA_VERSIONS[0];
        assert_eq!(version_name_from_data_version(first_dv), first_name);
        // 低于整张表
        assert_eq!(version_name_from_data_version(0), "");
        assert_eq!(version_name_from_data_version(first_dv - 1), "");

        // 真正落在相邻两条目之间：应取较小的那个
        if DATA_VERSIONS.len() >= 2 {
            let (lo_dv, lo_name) = DATA_VERSIONS[0];
            let (hi_dv, _) = DATA_VERSIONS[1];
            if hi_dv > lo_dv + 1 {
                assert_eq!(
                    version_name_from_data_version(lo_dv + 1),
                    lo_name,
                    "两条目之间应取不大于给定值的最大条目"
                );
            }
        }
    }

    #[test]
    fn unknown_high_version_returns_last() {
        let last = DATA_VERSIONS.last().unwrap();
        assert_eq!(version_name_from_data_version(last.0 + 100000), last.1);
    }

    /// 用外部已知的版本号钉住查表结果。
    ///
    /// 上面三条测试的期望值都取自 `DATA_VERSIONS` 自身，只能验证「查表
    /// 逻辑与表结构自洽」。若表里某个版本名写错（例如把 3465 写成
    /// "1.20.2"），它们全部照样通过——而错误会一路传到界面显示与
    /// 版本转换的目标选择上。
    ///
    /// 这里写死若干 dataVersion 与版本名的对应关系。它们来自 Minecraft
    /// 官方的 dataVersion 编号（每个正式版发布时确定，此后不再变动），
    /// 不是从这张表读出来的，因此能真正发现表里的笔误。
    #[test]
    fn well_known_data_versions_map_to_expected_names() {
        let anchors = [
            (3465, "1.20.1"),
            (3120, "1.19.2"),
            (2975, "1.18.2"),
            (2586, "1.16.5"),
            (2230, "1.15.2"),
            (1976, "1.14.4"),
            (1343, "1.12.2"),
        ];
        for (dv, want) in anchors {
            assert_eq!(
                version_name_from_data_version(dv),
                want,
                "dataVersion {dv} 应为 {want}，表里写成了别的版本名"
            );
        }
    }

    /// 表里每个版本名都不得为空、不得带首尾空白、应为可打印 ASCII。
    ///
    /// 注意：这张表**有意**同时收录正式版与快照。实测 159 条里有 101 条是
    /// 快照命名（`15w14a`、`19w13_3d_shareware_v1.34`、`20w14~` 等），
    /// 形式五花八门，因此不能要求「必须是 `<数字>.<数字>[.<数字>]`」——
    /// 那样会把合法的快照名误判为错误。
    ///
    /// 这里只断言所有条目都成立的性质，用来抓「多打了空格」「写成空串」
    /// 「混入不可见字符」这类笔误。这些错误会被上面基于表自身的测试放过。
    #[test]
    fn every_version_name_is_clean() {
        for (dv, name) in DATA_VERSIONS {
            assert!(!name.is_empty(), "dataVersion {dv} 的版本名为空");
            assert_eq!(
                name.trim(),
                *name,
                "dataVersion {dv} 的版本名 {name:?} 首尾有空白"
            );
            assert!(
                name.chars().all(|c| c.is_ascii_graphic() || c == ' '),
                "dataVersion {dv} 的版本名 {name:?} 含不可见或非 ASCII 字符"
            );
            assert!(
                !name.contains("  "),
                "dataVersion {dv} 的版本名 {name:?} 含连续空格"
            );
        }
    }

    /// 正式版命名的条目必须是 `<数字>.<数字>[.<数字>]` 形式。
    ///
    /// 判据是「以数字开头且不含空格」——快照名以两位年份开头（如 `15w14a`），
    /// 也以数字开头，因此这里进一步要求整串只由数字与点组成；
    /// 快照名含字母或下划线，自然被排除在外。
    #[test]
    fn release_style_names_are_wellformed() {
        let mut checked = 0;
        for (dv, name) in DATA_VERSIONS {
            let release_like = name.chars().all(|c| c.is_ascii_digit() || c == '.');
            if !release_like {
                continue; // 快照命名，跳过
            }
            checked += 1;
            let parts: Vec<&str> = name.split('.').collect();
            assert!(
                (2..=3).contains(&parts.len()),
                "dataVersion {dv} 的版本名 {name:?} 段数应为 2 或 3"
            );
            for p in &parts {
                assert!(
                    !p.is_empty(),
                    "dataVersion {dv} 的版本名 {name:?} 含空段"
                );
            }
        }
        assert!(
            checked >= 20,
            "只校验到 {checked} 条正式版命名，说明判据可能写错了"
        );
    }

    /// 表必须覆盖到较新的版本，避免忘记补充新版本后静默退化。
    ///
    /// 判据用「不小于已知的较新版本」，而不是断言某个固定最大值——
    /// 后者会在每次补新版本时被迫改测试。
    #[test]
    fn table_covers_recent_versions() {
        let (max_dv, _) = *DATA_VERSIONS.last().unwrap();
        assert!(
            max_dv >= 3465,
            "表的最新 dataVersion 为 {max_dv}，应至少覆盖 1.20.1（3465）"
        );
    }
}
