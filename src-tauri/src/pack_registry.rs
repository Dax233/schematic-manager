//! 已导入资源包的清单。
//!
//! 用途：用户导入的 jar 会转成资源包文件放在 `<程序目录>/packs/`，
//! 清单记录每个包的来源、类型、适用数据版本与启用状态，供界面列出，
//! 并在启动时决定载入哪些包。
//!
//! 对照 legacy-cpp 的 packmgr.cpp：清单为制表符分隔的文本，
//! 首行是带 `#` 的表头注释。沿用同一格式是为了让用户已有的 packs 目录
//! 无需转换即可继续使用。
//!
//! 文本解析与写出不涉及文件系统，因此可完整测试；读写文件由调用方负责。

use crate::error::{AppError, AppResult};

/// 资源包类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackKind {
    /// 内置基础库（随程序分发，清单里不出现）
    Builtin = 0,
    /// Minecraft 版本包：由客户端 jar 导入，dataVersion 取自 jar 内
    Version = 1,
    /// 模组包：叠加到指定版本，dataVersion 由用户选定
    Mod = 2,
}

impl PackKind {
    pub fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(PackKind::Builtin),
            1 => Some(PackKind::Version),
            2 => Some(PackKind::Mod),
            _ => None,
        }
    }
    pub fn as_i32(self) -> i32 {
        self as i32
    }
    /// 界面显示用的中文名
    pub fn label(self) -> &'static str {
        match self {
            PackKind::Builtin => "内置基础库",
            PackKind::Version => "MC 版本包",
            PackKind::Mod => "模组包",
        }
    }
}

/// 清单里的一项
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackEntry {
    /// 资源包文件名（相对 packs 目录，不含路径分隔符）
    pub file: String,
    /// 显示名
    pub name: String,
    /// 来源 jar 的路径，供界面显示与「重新导入」使用
    pub source: String,
    pub kind: PackKind,
    /// 适用的数据版本
    pub data_version: i32,
    pub enabled: bool,
}

/// 清单的表头。带 `#` 前缀，便于用户直接看懂文件内容。
const HEADER: &str = "# enabled\tfile\tname\tsource\tkind(1=MC,2=mod)\tdataVersion";

/// 制表符转义：字段内出现制表符、换行或反斜杠时必须转义，
/// 否则一行会被解析成更多字段，整份清单错位。
fn escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\t' => o.push_str("\\t"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\\' => o.push_str("\\\\"),
            _ => o.push(c),
        }
    }
    o
}

/// 反转义。
///
/// 遇到无法识别的转义序列时保留反斜杠与原字符——静默丢弃会让
/// 路径变得与原始文件不同，进而在「重新导入」时找不到源文件。
fn unescape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            o.push(c);
            continue;
        }
        match it.next() {
            Some('t') => o.push('\t'),
            Some('n') => o.push('\n'),
            Some('r') => o.push('\r'),
            Some('\\') => o.push('\\'),
            Some(other) => {
                o.push('\\');
                o.push(other);
            }
            None => o.push('\\'),
        }
    }
    o
}

/// 解析清单文本。
///
/// 单行解析失败时跳过该行而不是整体失败：清单由程序写出，
/// 但用户可能手工编辑；一行有误不应让其余已导入的包全部不可用。
/// 跳过的行数由调用方通过返回值判断（见 `load_with_errors`）。
pub fn parse(text: &str) -> Vec<PackEntry> {
    parse_with_errors(text).0
}

/// 解析清单，同时返回被跳过的行。
pub fn parse_with_errors(text: &str) -> (Vec<PackEntry>, Vec<String>) {
    let mut out = Vec::new();
    let mut skipped = Vec::new();

    for (lineno, raw) in text.lines().enumerate() {
        let line = raw.trim_end_matches(['\r', '\n']);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 6 {
            skipped.push(format!("第 {} 行字段不足（{} 个）", lineno + 1, f.len()));
            continue;
        }
        let enabled = match f[0].trim() {
            "1" => true,
            "0" => false,
            other => {
                skipped.push(format!("第 {} 行的启用标志「{other}」无效", lineno + 1));
                continue;
            }
        };
        let kind = match f[4].trim().parse::<i32>().ok().and_then(PackKind::from_i32) {
            Some(k) => k,
            None => {
                skipped.push(format!("第 {} 行的类型「{}」无效", lineno + 1, f[4]));
                continue;
            }
        };
        let data_version = match f[5].trim().parse::<i32>() {
            Ok(v) => v,
            Err(_) => {
                skipped.push(format!(
                    "第 {} 行的数据版本「{}」不是整数",
                    lineno + 1,
                    f[5]
                ));
                continue;
            }
        };
        let file = unescape(f[1]);
        if file.is_empty() {
            skipped.push(format!("第 {} 行没有文件名", lineno + 1));
            continue;
        }
        out.push(PackEntry {
            file,
            name: unescape(f[2]),
            source: unescape(f[3]),
            kind,
            data_version,
            enabled,
        });
    }
    (out, skipped)
}

/// 序列化清单。输出确定：同一组条目总是得到相同文本。
pub fn serialize(entries: &[PackEntry]) -> String {
    let mut o = String::new();
    o.push_str(HEADER);
    o.push('\n');
    for e in entries {
        o.push_str(if e.enabled { "1\t" } else { "0\t" });
        o.push_str(&escape(&e.file));
        o.push('\t');
        o.push_str(&escape(&e.name));
        o.push('\t');
        o.push_str(&escape(&e.source));
        o.push('\t');
        o.push_str(&e.kind.as_i32().to_string());
        o.push('\t');
        o.push_str(&e.data_version.to_string());
        o.push('\n');
    }
    o
}

/// 新增或替换一项（以文件名为准）。
///
/// 同一来源重复导入时替换而不是追加：否则会出现多条指向同一文件的记录，
/// 界面列出重复项、载入时重复合并同一份数据。
pub fn upsert(entries: &mut Vec<PackEntry>, item: PackEntry) {
    if let Some(slot) = entries.iter_mut().find(|e| e.file == item.file) {
        // 保留原有的启用状态：重新导入不应把用户关掉的包又打开
        let was_enabled = slot.enabled;
        *slot = item;
        slot.enabled = was_enabled;
    } else {
        entries.push(item);
    }
}

/// 移除一项，返回是否确有移除
pub fn remove(entries: &mut Vec<PackEntry>, file: &str) -> bool {
    let before = entries.len();
    entries.retain(|e| e.file != file);
    entries.len() != before
}

/// 切换启用状态，返回是否找到该项
pub fn set_enabled(entries: &mut [PackEntry], file: &str, enabled: bool) -> bool {
    if let Some(e) = entries.iter_mut().find(|e| e.file == file) {
        e.enabled = enabled;
        true
    } else {
        false
    }
}

/// 已导入包涉及的数据版本（去重、升序）。用于判断「该版本的贴图是否已具备」。
pub fn imported_data_versions(entries: &[PackEntry]) -> Vec<i32> {
    let mut v: Vec<i32> = entries.iter().map(|e| e.data_version).collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// 由来源 jar 路径与名称推出资源包文件名。
///
/// 用名称而非直接取源文件名：同名 jar 来自不同目录时，
/// 直接取文件名会互相覆盖。名称里可能含有文件系统不允许的字符，
/// 这里替换成下划线。
pub fn pack_file_name(name: &str, existing: &[PackEntry]) -> AppResult<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(AppError::Parse("资源包名不能为空".into()));
    }
    let mut base: String = trimmed
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    // 结尾的点与空格在 Windows 上会被忽略，导致实际文件名与清单不符
    while base.ends_with('.') || base.ends_with(' ') {
        base.pop();
    }
    if base.is_empty() {
        return Err(AppError::Parse("资源包名经清理后为空".into()));
    }
    let candidate = format!("{base}.pack");
    if !existing.iter().any(|e| e.file == candidate) {
        return Ok(candidate);
    }
    // 重名时加序号，避免覆盖已有包
    for i in 2..1000 {
        let c = format!("{base}_{i}.pack");
        if !existing.iter().any(|e| e.file == c) {
            return Ok(c);
        }
    }
    Err(AppError::Parse(format!("资源包名「{base}」重复过多")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(file: &str, name: &str, kind: PackKind, dv: i32) -> PackEntry {
        PackEntry {
            file: file.into(),
            name: name.into(),
            source: format!("/jars/{name}.jar"),
            kind,
            data_version: dv,
            enabled: true,
        }
    }

    #[test]
    fn header_is_skipped_and_fields_read() {
        let text = "# enabled\tfile\tname\tsource\tkind(1=MC,2=mod)\tdataVersion\n\
                    1\tfoo.pack\tFoo\t/jars/foo.jar\t1\t3465\n";
        let v = parse(text);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].file, "foo.pack");
        assert_eq!(v[0].name, "Foo");
        assert_eq!(v[0].source, "/jars/foo.jar");
        assert_eq!(v[0].kind, PackKind::Version);
        assert_eq!(v[0].data_version, 3465);
        assert!(v[0].enabled);
    }

    #[test]
    fn round_trip_preserves_all_fields() {
        let items = vec![
            entry("a.pack", "A 包", PackKind::Version, 3465),
            PackEntry {
                enabled: false,
                ..entry("b.pack", "B", PackKind::Mod, 9999)
            },
        ];
        let text = serialize(&items);
        let back = parse(&text);
        assert_eq!(back, items, "往返必须完全一致");
    }

    #[test]
    fn round_trip_is_deterministic() {
        let items = vec![entry("a.pack", "A", PackKind::Version, 1)];
        assert_eq!(serialize(&items), serialize(&items));
    }

    #[test]
    fn tabs_and_newlines_in_fields_are_escaped() {
        // 名称里含制表符与换行：不转义会让一行被解析成更多字段
        let items = vec![entry("a.pack", "bad\tname\nhere", PackKind::Version, 1)];
        let text = serialize(&items);
        assert_eq!(text.lines().count(), 2, "转义后仍应只有表头与一行数据");
        let back = parse(&text);
        assert_eq!(back[0].name, "bad\tname\nhere", "转义应可逆");
    }

    #[test]
    fn backslashes_are_escaped_and_reversible() {
        let items = vec![entry("a.pack", "C:\\dir\\x", PackKind::Version, 1)];
        let back = parse(&serialize(&items));
        assert_eq!(back[0].name, "C:\\dir\\x");
    }

    #[test]
    fn unknown_escape_keeps_backslash() {
        // 无法识别的转义应保留原样，而不是静默吞掉反斜杠
        let text = "1\ta.pack\tna\\qme\t/jars/a.jar\t1\t1\n";
        let v = parse(text);
        assert_eq!(v[0].name, "na\\qme");
    }

    #[test]
    fn malformed_line_is_skipped_but_others_survive() {
        let text = "# header\n\
                    not-enough-fields\n\
                    1\tgood.pack\tGood\t/j\t1\t100\n\
                    2\tbad.pack\tBad\t/j\t1\t100\n\
                    1\tbad2.pack\tB2\t/j\t9\t100\n\
                    1\tbad3.pack\tB3\t/j\t1\tnotanumber\n";
        let (v, skipped) = parse_with_errors(text);
        assert_eq!(v.len(), 1, "只有合法的那行应被接受");
        assert_eq!(v[0].file, "good.pack");
        assert_eq!(skipped.len(), 4, "其余四行都应被记为跳过：{skipped:?}");
    }

    #[test]
    fn skipped_lines_mention_line_number_and_reason() {
        let text = "# h\n2\ta.pack\tA\t/j\t1\t1\n";
        let (_, skipped) = parse_with_errors(text);
        assert_eq!(skipped.len(), 1);
        assert!(skipped[0].contains("第 2 行"), "应指出行号：{}", skipped[0]);
        assert!(skipped[0].contains("启用标志"), "应说明原因：{}", skipped[0]);
    }

    #[test]
    fn empty_text_yields_nothing() {
        assert!(parse("").is_empty());
        assert!(parse("# only a comment\n").is_empty());
        assert!(parse("\n\n").is_empty());
    }

    #[test]
    fn upsert_replaces_same_file_and_keeps_enabled_state() {
        let mut v = vec![entry("a.pack", "Old", PackKind::Version, 100)];
        set_enabled(&mut v, "a.pack", false);
        upsert(&mut v, entry("a.pack", "New", PackKind::Version, 200));
        assert_eq!(v.len(), 1, "同一文件应替换而不是追加");
        assert_eq!(v[0].name, "New");
        assert_eq!(v[0].data_version, 200);
        assert!(!v[0].enabled, "重新导入不应把用户关掉的包又打开");
    }

    #[test]
    fn upsert_appends_new_file() {
        let mut v = vec![entry("a.pack", "A", PackKind::Version, 1)];
        upsert(&mut v, entry("b.pack", "B", PackKind::Version, 1));
        assert_eq!(v.len(), 2);
        assert_eq!(v[1].file, "b.pack");
    }

    #[test]
    fn remove_reports_whether_anything_was_removed() {
        let mut v = vec![entry("a.pack", "A", PackKind::Version, 1)];
        assert!(remove(&mut v, "a.pack"));
        assert!(v.is_empty());
        assert!(!remove(&mut v, "a.pack"), "再删一次应返回 false");
        assert!(!remove(&mut v, "never.pack"));
    }

    #[test]
    fn set_enabled_reports_missing() {
        let mut v = vec![entry("a.pack", "A", PackKind::Version, 1)];
        assert!(set_enabled(&mut v, "a.pack", false));
        assert!(!v[0].enabled);
        assert!(!set_enabled(&mut v, "nope.pack", true));
    }

    #[test]
    fn imported_versions_are_sorted_and_deduped() {
        let v = vec![
            entry("a.pack", "A", PackKind::Version, 3465),
            entry("b.pack", "B", PackKind::Version, 100),
            entry("c.pack", "C", PackKind::Version, 3465),
        ];
        assert_eq!(imported_data_versions(&v), vec![100, 3465]);
        assert!(imported_data_versions(&[]).is_empty());
    }

    #[test]
    fn pack_file_name_uses_name_and_extension() {
        let n = pack_file_name("1.20.1", &[]).unwrap();
        assert_eq!(n, "1.20.1.pack");
    }

    #[test]
    fn pack_file_name_replaces_forbidden_characters() {
        // 名称里可能出现路径分隔符等文件系统不允许的字符
        let n = pack_file_name("a/b:c*d", &[]).unwrap();
        assert_eq!(n, "a_b_c_d.pack");
        assert!(!n.contains('/') && !n.contains(':'));
    }

    #[test]
    fn pack_file_name_strips_trailing_dots_and_spaces() {
        // Windows 会忽略结尾的点与空格，留下会让清单里的文件名与实际不符
        let n = pack_file_name("name. ", &[]).unwrap();
        assert_eq!(n, "name.pack");
    }

    #[test]
    fn pack_file_name_rejects_empty() {
        assert!(pack_file_name("", &[]).is_err());
        assert!(pack_file_name("   ", &[]).is_err());
    }

    #[test]
    fn pack_file_name_avoids_collision() {
        let existing = vec![entry("1.20.1.pack", "1.20.1", PackKind::Version, 1)];
        let n = pack_file_name("1.20.1", &existing).unwrap();
        assert_ne!(n, "1.20.1.pack", "重名时应加序号，避免覆盖已有包");
        assert_eq!(n, "1.20.1_2.pack");
    }

    #[test]
    fn pack_file_name_collision_counter_advances() {
        let existing = vec![
            entry("x.pack", "x", PackKind::Version, 1),
            entry("x_2.pack", "x", PackKind::Version, 1),
        ];
        assert_eq!(pack_file_name("x", &existing).unwrap(), "x_3.pack");
    }

    #[test]
    fn kind_round_trips_through_i32() {
        for k in [PackKind::Builtin, PackKind::Version, PackKind::Mod] {
            assert_eq!(PackKind::from_i32(k.as_i32()), Some(k));
        }
        assert_eq!(PackKind::from_i32(7), None);
        assert_eq!(PackKind::Version.label(), "MC 版本包");
    }

    #[test]
    fn enabled_flag_round_trips() {
        let items = vec![
            PackEntry { enabled: true, ..entry("a.pack", "A", PackKind::Version, 1) },
            PackEntry { enabled: false, ..entry("b.pack", "B", PackKind::Mod, 2) },
        ];
        let back = parse(&serialize(&items));
        assert!(back[0].enabled);
        assert!(!back[1].enabled);
    }
}
