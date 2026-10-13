//! 改动投影文件前的自动备份。
//!
//! 备份目录放在程序所在目录的 backups/ 下，与 legacy-cpp 的行为保持一致：
//! 用户在有问题的改动后能直接找到原件，而不是去猜测备份去了哪里。

use crate::error::{AppError, AppResult};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// 备份根目录：<程序所在目录>/backups
///
/// exe_dir 由调用方传入，便于测试时注入临时目录。
pub fn backup_root(exe_dir: &Path) -> PathBuf {
    exe_dir.join("backups")
}

/// 备份文件名的唯一后缀。
///
/// 用纳秒而非毫秒：同一毫秒内的两次备份若得到相同文件名，
/// std::fs::copy 会直接覆盖，静默丢掉一份备份。
///
/// 必须是**纯数字**：文件名以 "__" 分隔字段，后缀里若含下划线，
/// 解析时会把它当成字段分隔符，导致所有真实备份都列不出来。
fn timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    now.as_nanos().to_string()
}

/// 备份结果
#[derive(Debug, Clone)]
pub struct BackupOutcome {
    pub backup_path: PathBuf,
}

/// 把 src 复制到 backups/<文件名主干>/<主干>__<时间戳>__<操作>.litematic
///
/// 每个源文件一个子目录，避免同名文件互相覆盖；
/// 文件名里带上操作名与时间戳，便于在资源管理器里直接辨认。
pub fn create(exe_dir: &Path, src: &Path, op: &str) -> AppResult<BackupOutcome> {
    let stem = src
        .file_stem()
        .map(|v| v.to_string_lossy().into_owned())
        .ok_or_else(|| crate::error::AppError::Parse("源文件没有文件名主干".into()))?;

    // 分组目录必须把「原始所在目录」也区分开：不同目录下同名文件
    // （A/demo.litematic 与 B/demo.litematic）若共用一个分组，
    // 还原时只比对文件名主干会把两个不同文件判为同源，进而覆盖错对象。
    let dir = group_dir(exe_dir, src)?;
    std::fs::create_dir_all(&dir)?;

    // 记录原始完整路径。legacy-cpp 用一个 index.tsv 记 origPath，
    // 这里改为每组一个旁文件——省掉索引与实际文件不一致的失配面。
    std::fs::write(dir.join(ORIG_PATH_FILE), orig_path_of(src))?;

    let file_name = format!("{stem}__{}__{op}.litematic", timestamp());
    let dst = dir.join(file_name);
    std::fs::copy(src, &dst)?;

    Ok(BackupOutcome { backup_path: dst })
}

/// 记录原始路径的旁文件名
const ORIG_PATH_FILE: &str = ".orig_path";

/// FNV-1a 64 位哈希，用于把父目录折叠成分组名后缀
fn hash_path(s: &str) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h = OFFSET;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(PRIME);
    }
    h
}

/// 原始路径的规范化文本，用于跨平台比较。
///
/// 统一小写并去掉尾部分隔符：Windows 路径大小写不敏感，
/// `C:\A` 与 `c:\a` 必须视为同一处。
fn orig_path_of(src: &Path) -> String {
    let abs = std::fs::canonicalize(src).unwrap_or_else(|_| {
        src.parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default()
            .join(src.file_name().unwrap_or_default())
    });
    let mut t = abs.to_string_lossy().replace('/', "\\").to_lowercase();
    while t.len() > 3 && t.ends_with('\\') {
        t.pop();
    }
    t
}

fn group_dir(exe_dir: &Path, src: &Path) -> AppResult<PathBuf> {
    let stem = src
        .file_stem()
        .map(|v| v.to_string_lossy().into_owned())
        .ok_or_else(|| crate::error::AppError::Parse("源文件没有文件名主干".into()))?;
    let parent = src
        .parent()
        .map(|p| p.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let key = hash_path(&parent);
    Ok(backup_root(exe_dir).join(format!("{stem}_{key:016x}")))
}


/// 一条备份记录
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupEntry {
    pub path: PathBuf,
    /// 原始文件的完整路径。还原时必须比对它，只看文件名主干会把
    /// 不同目录下的同名文件误判为同一文件。
    pub orig_path: String,
    /// 原始文件的文件名主干
    pub stem: String,
    /// 触发备份的操作名
    pub op: String,
    /// 备份时间（纳秒时间戳，由 timestamp() 产生）
    pub created_ns: u64,
    pub size: u64,
}

/// 解析备份文件名：<主干>__<时间戳>__<操作>.litematic
///
/// 从右侧切分：主干本身可能含有 "__"，从左侧切会把名字切坏。
fn parse_backup_name(name: &str) -> Option<(String, u64, String)> {
    let base = name.strip_suffix(".litematic")?;
    let parts: Vec<&str> = base.rsplitn(3, "__").collect();
    if parts.len() != 3 {
        return None;
    }
    let op = parts[0].to_string();
    let created_ns = parts[1].parse::<u64>().ok()?;
    let stem = parts[2].to_string();
    if stem.is_empty() || op.is_empty() {
        return None;
    }
    Some((stem, created_ns, op))
}

/// 列出全部备份，按时间倒序（最新的在前）。
///
/// 采用直接扫描目录而非维护索引文件：索引与实际文件容易不一致，
/// 一旦不一致就会出现「备份存在但列表为空」这类问题。
pub fn list(root: &Path) -> Vec<BackupEntry> {
    let mut out = Vec::new();
    let Ok(groups) = std::fs::read_dir(root) else { return out };
    for g in groups.flatten() {
        let Ok(ft) = g.file_type() else { continue };
        if !ft.is_dir() {
            continue;
        }
        let Ok(files) = std::fs::read_dir(g.path()) else { continue };
        for f in files.flatten() {
            let path = f.path();
            let Some(name) = path.file_name().map(|v| v.to_string_lossy().into_owned()) else {
                continue;
            };
            let Some((stem, created_ns, op)) = parse_backup_name(&name) else { continue };
            let size = f.metadata().map(|m| m.len()).unwrap_or(0);
            // 原始路径来自分组目录下的旁文件；缺失时留空，
            // 由还原前的校验拒绝，而不是默认放行
            let orig_path = path
                .parent()
                .and_then(|d| std::fs::read_to_string(d.join(ORIG_PATH_FILE)).ok())
                .map(|v| v.trim().to_string())
                .unwrap_or_default();
            out.push(BackupEntry { path, orig_path, stem, op, created_ns, size });
        }
    }
    // 时间戳相同（同纳秒级）时按路径兜底排序，保证结果稳定
    out.sort_by(|a, b| b.created_ns.cmp(&a.created_ns).then_with(|| a.path.cmp(&b.path)));
    out
}

/// 把备份还原到指定位置。
///
/// 还原前会先检查备份文件确实存在：否则会静默产生一个空目标文件。
pub fn restore(backup: &Path, target: &Path) -> AppResult<()> {
    if !backup.is_file() {
        return Err(AppError::Parse(format!("备份文件不存在：{}", backup.display())));
    }
    std::fs::copy(backup, target)?;
    Ok(())
}

/// 删除一条备份，并在分组内已无备份时清理该分组目录。
///
/// 顺序很重要：先删备份文件，**再**判断分组是否为空，最后才删记录原始路径的
/// 旁文件与目录。同一次备份的多个副本共用一个分组和一份 .orig_path，若在删除
/// 备份前就无条件删掉 .orig_path，同组其余备份会因 matches_target 读不到记录
/// 而永远无法还原。
pub fn delete(backup: &Path) -> AppResult<()> {
    if !backup.exists() {
        return Err(AppError::Parse(format!("备份文件不存在：{}", backup.display())));
    }
    std::fs::remove_file(backup)?;
    let Some(dir) = backup.parent() else { return Ok(()) };

    // 分组里还有别的备份时到此为止：.orig_path 是整组共用的，不能删。
    // 判空时必须把 .orig_path 本身排除掉，否则它会让目录看起来永远非空。
    let remaining = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .flatten()
            .any(|e| e.file_name() != std::ffi::OsStr::new(ORIG_PATH_FILE)),
        // 读不到目录时不敢当作「空」：宁可留下空分组，也不要误删同组记录
        Err(_) => true,
    };
    if remaining {
        return Ok(());
    }

    // 只剩记录原始路径的旁文件：连同目录一起清理，避免 backups 下堆积空目录。
    let _ = std::fs::remove_file(dir.join(ORIG_PATH_FILE));
    let _ = std::fs::remove_dir(dir);
    Ok(())
}

/// 备份与目标文件是否同源（文件名主干一致）。
///
/// 还原是覆盖写，若允许跨文件还原，用户很容易把 A 的备份盖到 B 上，
/// 而且覆盖后原内容只剩备份可寻。因此还原前必须校验。
pub fn matches_target(backup: &Path, target: &Path) -> bool {
    let Some(name) = backup.file_name().map(|v| v.to_string_lossy().into_owned()) else {
        return false;
    };
    if parse_backup_name(&name).is_none() {
        return false;
    }
    // 比对完整原始路径，而不是只看文件名主干：
    // A/demo.litematic 与 B/demo.litematic 主干相同却是两个文件，
    // 只比主干会把 A 的备份还原到 B 上。
    let Some(dir) = backup.parent() else { return false };
    let Ok(recorded) = std::fs::read_to_string(dir.join(ORIG_PATH_FILE)) else {
        return false; // 没有记录就不放行
    };
    let recorded = recorded.trim();
    if recorded.is_empty() {
        return false;
    }
    recorded == orig_path_of(target)
}

/// 路径是否位于指定根目录之内。
///
/// 删除与还原的参数来自前端，必须限定在备份目录内，否则命令层就成了
/// 一个可以删除任意文件的入口。
///
/// 比较前先规范化：否则 `backups/../other` 之类的路径会绕过检查。
pub fn is_inside(root: &Path, path: &Path) -> bool {
    let norm = |p: &Path| -> PathBuf {
        let mut out = PathBuf::new();
        for seg in p.components() {
            match seg {
                std::path::Component::ParentDir => {
                    out.pop();
                }
                std::path::Component::CurDir => {}
                other => out.push(other.as_os_str()),
            }
        }
        out
    };
    norm(path).starts_with(norm(root))
}

/// 操作日志文件位置
pub fn log_path(root: &Path) -> PathBuf {
    root.join("operations.log")
}

/// 追加一行操作日志。
///
/// 日志失败不应影响主流程，因此调用方可以选择忽略返回值；
/// 但这里如实返回错误，由调用方决定。
pub fn append_log(root: &Path, line: &str) -> AppResult<()> {
    std::fs::create_dir_all(root)?;
    let path = log_path(root);
    let stamp = timestamp();
    let content = format!("{stamp}\t{line}\n");
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    f.write_all(content.as_bytes())?;
    Ok(())
}

/// 读取最近若干行日志，返回顺序为由新到旧。
pub fn read_log(root: &Path, max_lines: usize) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(log_path(root)) else { return Vec::new() };
    let mut lines: Vec<String> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.to_string())
        .collect();
    lines.reverse();
    lines.truncate(max_lines);
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试用临时目录。
    /// 不用 std::env::temp_dir()：该路径在部分受限环境下不可写，
    /// 会让测试因环境而失败。放在工程内的 target 下更可控。
    fn temp_dir(tag: &str) -> PathBuf {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("testtmp")
            .join(format!("sm_backup_{tag}_{}", timestamp()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn create_copies_file_into_stem_subdir() {
        let base = temp_dir("basic");
        let src = base.join("demo.litematic");
        std::fs::write(&src, b"hello").unwrap();

        let out = create(&base, &src, "metadata").unwrap();
        assert!(out.backup_path.exists(), "备份文件应存在");
        assert_eq!(std::fs::read(&out.backup_path).unwrap(), b"hello");

        // 备份应位于 backups/<主干>/ 之下
        let rel = out.backup_path.strip_prefix(backup_root(&base)).unwrap();
        assert_eq!(rel.components().count(), 2, "应有一层以文件主干命名的子目录");
        let name = out.backup_path.file_name().unwrap().to_string_lossy();
        assert!(name.starts_with("demo__"));
        assert!(name.ends_with("__metadata.litematic"));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn create_for_missing_source_errors() {
        let base = temp_dir("missing");
        let r = create(&base, &base.join("nope.litematic"), "metadata");
        assert!(r.is_err(), "源文件不存在时必须报错，不能静默产生空备份");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn repeated_backups_do_not_overwrite() {
        let base = temp_dir("repeat");
        let src = base.join("a.litematic");
        std::fs::write(&src, b"1").unwrap();
        let first = create(&base, &src, "metadata").unwrap();
        // 内容改成不同值后再次备份，两份都应保留
        std::fs::write(&src, b"2").unwrap();
        let second = create(&base, &src, "metadata").unwrap();

        assert!(first.backup_path.exists());
        assert!(second.backup_path.exists());
        // 只断言「两个文件都存在」测不出覆盖：路径相同时这两条依然通过，
        // 因此必须显式断言路径不同。
        assert_ne!(
            first.backup_path, second.backup_path,
            "两次备份必须落在不同文件，否则后者会静默覆盖前者"
        );
        let _ = std::fs::remove_dir_all(&base);
    }
    #[test]
    fn parse_backup_name_handles_underscores_in_stem() {
        // 主干本身含 "__" 时，必须从右侧切分
        let r = parse_backup_name("我的__投影__1700000000000__metadata.litematic");
        assert_eq!(r, Some(("我的__投影".to_string(), 1700000000000, "metadata".to_string())));
        assert!(parse_backup_name("没有分隔符.litematic").is_none());
        assert!(parse_backup_name("a__notanumber__op.litematic").is_none());
        assert!(parse_backup_name("a__1__op.txt").is_none(), "扩展名不符应拒绝");
    }

    #[test]
    fn list_finds_backups_and_sorts_newest_first() {
        let base = temp_dir("list");
        let root = backup_root(&base);
        let src = base.join("demo.litematic");
        std::fs::write(&src, b"x").unwrap();
        let first = create(&base, &src, "metadata").unwrap();

        // 手工造一条时间戳更大的备份，验证排序。
        // 注意真实备份用的是纳秒时间戳（19 位），这里取 u64::MAX 才确保更大；
        // 早先写成毫秒量级的 13 位数字，反而比真实备份小。
        let dir = root.join("demo");
        std::fs::create_dir_all(&dir).unwrap();
        let newer = dir.join(format!("demo__{}__replace.litematic", u64::MAX));
        std::fs::write(&newer, b"y").unwrap();

        let items = list(&root);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].path, newer, "最新的应排在最前");
        assert_eq!(items[0].op, "replace");
        assert_eq!(items[1].path, first.backup_path);
        assert_eq!(items[0].size, 1);

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn restore_copies_content_back() {
        let base = temp_dir("restore");
        let root = backup_root(&base);
        let src = base.join("a.litematic");
        std::fs::write(&src, b"original").unwrap();
        let b = create(&base, &src, "metadata").unwrap();
        std::fs::write(&src, b"modified").unwrap();

        restore(&b.backup_path, &src).unwrap();
        assert_eq!(std::fs::read(&src).unwrap(), b"original");

        // 备份不存在时必须报错，而不是产生空文件
        let missing = root.join("a").join("nope.litematic");
        let e = restore(&missing, &src).unwrap_err().to_string();
        assert!(e.contains("不存在"), "应说明备份不存在：{e}");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn delete_removes_backup_and_empty_group() {
        let base = temp_dir("delete");
        let root = backup_root(&base);
        let src = base.join("b.litematic");
        std::fs::write(&src, b"x").unwrap();
        let b = create(&base, &src, "metadata").unwrap();
        assert!(b.backup_path.exists());

        delete(&b.backup_path).unwrap();
        assert!(!b.backup_path.exists());
        assert!(!root.join("b").exists(), "空的分组目录应被清理");

        assert!(delete(&b.backup_path).is_err(), "删除不存在的备份应报错");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// 同一文件的多份备份共用一个分组与一份 .orig_path。
    /// 删掉其中一份不能影响其余备份的还原能力——否则用户删了旧备份之后，
    /// 想还原较新的那份会被「不同源」错误地拒绝。
    #[test]
    fn deleting_one_backup_keeps_others_restorable() {
        let base = temp_dir("delete_keeps_siblings");
        let src = base.join("demo.litematic");
        std::fs::write(&src, b"v1").unwrap();
        let older = create(&base, &src, "metadata").unwrap();

        std::fs::write(&src, b"v2").unwrap();
        let newer = create(&base, &src, "replace").unwrap();

        // 两份应在同一分组（同一文件的两次备份）
        assert_eq!(older.backup_path.parent(), newer.backup_path.parent());

        delete(&older.backup_path).unwrap();

        // 剩下那份必须仍可还原
        assert!(
            matches_target(&newer.backup_path, &src),
            "同组还有备份时，不能把 .orig_path 一起删掉，否则其余备份永远无法还原"
        );
        restore(&newer.backup_path, &src).unwrap();
        assert_eq!(std::fs::read(&src).unwrap(), b"v2");

        // 删掉最后一份时才应清理分组目录
        delete(&newer.backup_path).unwrap();
        assert!(!newer.backup_path.parent().unwrap().exists(), "最后一份删掉后分组目录应被清理");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn matches_target_requires_same_original_path() {
        let base = temp_dir("match");
        let root = backup_root(&base);

        // 两个不同目录下的同名文件
        let dir_a = base.join("A");
        let dir_b = base.join("B");
        std::fs::create_dir_all(&dir_a).unwrap();
        std::fs::create_dir_all(&dir_b).unwrap();
        let fa = dir_a.join("demo.litematic");
        let fb = dir_b.join("demo.litematic");
        std::fs::write(&fa, b"a").unwrap();
        std::fs::write(&fb, b"b").unwrap();

        let ba = create(&base, &fa, "metadata").unwrap();

        // 同源：完整路径一致
        assert!(matches_target(&ba.backup_path, &fa));
        // 跨目录同名文件绝不能被判为同源，否则 A 的备份会盖到 B 上
        assert!(
            !matches_target(&ba.backup_path, &fb),
            "A/demo.litematic 的备份不能还原到 B/demo.litematic"
        );
        // 文件名不合规时判为不匹配，而不是默认通过
        assert!(!matches_target(&dir_a.join("demo.litematic"), &fa));
        // 记录缺失时也不放行
        let orphan = root.join("x").join("x__1__metadata.litematic");
        std::fs::create_dir_all(orphan.parent().unwrap()).unwrap();
        std::fs::write(&orphan, b"x").unwrap();
        assert!(!matches_target(&orphan, &fa));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn same_name_in_different_dirs_gets_separate_groups() {
        let base = temp_dir("groups");
        let dir_a = base.join("A");
        let dir_b = base.join("B");
        std::fs::create_dir_all(&dir_a).unwrap();
        std::fs::create_dir_all(&dir_b).unwrap();
        let fa = dir_a.join("demo.litematic");
        let fb = dir_b.join("demo.litematic");
        std::fs::write(&fa, b"a").unwrap();
        std::fs::write(&fb, b"b").unwrap();

        let ba = create(&base, &fa, "metadata").unwrap();
        let bb = create(&base, &fb, "metadata").unwrap();

        assert_ne!(
            ba.backup_path.parent(),
            bb.backup_path.parent(),
            "同名但不同目录的文件不能混进同一分组"
        );
        // 各自的备份只能还原到自己的原文件
        assert!(matches_target(&ba.backup_path, &fa));
        assert!(matches_target(&bb.backup_path, &fb));
        assert!(!matches_target(&ba.backup_path, &fb));

        let _ = std::fs::remove_dir_all(&base);
    }
    #[test]
    fn log_appends_and_reads_newest_first() {
        let base = temp_dir("log");
        let root = backup_root(&base);
        append_log(&root, "第一条").unwrap();
        append_log(&root, "第二条").unwrap();

        let lines = read_log(&root, 10);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with("第二条"), "应由新到旧");
        assert!(lines[1].ends_with("第一条"));

        // 超出上限时只取最近的若干条
        let one = read_log(&root, 1);
        assert_eq!(one.len(), 1);
        assert!(one[0].ends_with("第二条"));

        // 目录不存在时返回空而不是 panic
        assert!(read_log(&base.join("nope"), 10).is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn is_inside_rejects_paths_outside_root() {
        let root = PathBuf::from("/data/backups");
        assert!(is_inside(&root, &root.join("demo").join("a.litematic")));
        assert!(is_inside(&root, &root));
        // 同级或完全无关的路径必须拒绝
        assert!(!is_inside(&root, &PathBuf::from("/data/other/a.litematic")));
        assert!(!is_inside(&root, &PathBuf::from("/etc/passwd")));
        // 用 .. 逃逸的路径不能绕过检查
        assert!(!is_inside(&root, &root.join("..").join("other").join("a.litematic")));
    }

}
