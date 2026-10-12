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

    let dir = backup_root(exe_dir).join(&stem);
    std::fs::create_dir_all(&dir)?;

    let file_name = format!("{stem}__{}__{op}.litematic", timestamp());
    let dst = dir.join(file_name);
    std::fs::copy(src, &dst)?;

    Ok(BackupOutcome { backup_path: dst })
}


/// 一条备份记录
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupEntry {
    pub path: PathBuf,
    /// 原始文件的文件名主干
    pub stem: String,
    /// 触发备份的操作名
    pub op: String,
    /// 备份时间（毫秒时间戳）
    pub created_ms: u64,
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
    let created_ms = parts[1].parse::<u64>().ok()?;
    let stem = parts[2].to_string();
    if stem.is_empty() || op.is_empty() {
        return None;
    }
    Some((stem, created_ms, op))
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
            let Some((stem, created_ms, op)) = parse_backup_name(&name) else { continue };
            let size = f.metadata().map(|m| m.len()).unwrap_or(0);
            out.push(BackupEntry { path, stem, op, created_ms, size });
        }
    }
    // 时间戳相同（同纳秒级）时按路径兜底排序，保证结果稳定
    out.sort_by(|a, b| b.created_ms.cmp(&a.created_ms).then_with(|| a.path.cmp(&b.path)));
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

/// 删除一条备份，并清理因此变空的分组目录。
pub fn delete(backup: &Path) -> AppResult<()> {
    if !backup.exists() {
        return Err(AppError::Parse(format!("备份文件不存在：{}", backup.display())));
    }
    std::fs::remove_file(backup)?;
    // 分组目录空了就一并删掉，避免 backups 下堆积空目录
    if let Some(dir) = backup.parent() {
        if let Ok(mut it) = std::fs::read_dir(dir) {
            if it.next().is_none() {
                let _ = std::fs::remove_dir(dir);
            }
        }
    }
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
    let Some((stem, _, _)) = parse_backup_name(&name) else { return false };
    target
        .file_stem()
        .map(|v| v.to_string_lossy() == stem)
        .unwrap_or(false)
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

    #[test]
    fn matches_target_compares_stems() {
        let dir = PathBuf::from("/x");
        let backup = dir.join("demo__123__metadata.litematic");
        assert!(matches_target(&backup, &dir.join("demo.litematic")));
        // 不同文件必须判为不匹配
        assert!(!matches_target(&backup, &dir.join("other.litematic")));
        // 文件名不合规时判为不匹配，而不是默认通过
        assert!(!matches_target(&dir.join("demo.litematic"), &dir.join("demo.litematic")));
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
}
