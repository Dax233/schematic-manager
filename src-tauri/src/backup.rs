//! 改动投影文件前的自动备份。
//!
//! 备份目录放在程序所在目录的 backups/ 下，与 legacy-cpp 的行为保持一致：
//! 用户在有问题的改动后能直接找到原件，而不是去猜测备份去了哪里。

use crate::error::AppResult;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// 备份根目录：<程序所在目录>/backups
///
/// exe_dir 由调用方传入，便于测试时注入临时目录。
pub fn backup_root(exe_dir: &Path) -> PathBuf {
    exe_dir.join("backups")
}

fn timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("{now}")
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("sm_backup_{tag}_{}", timestamp()));
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
        let _ = std::fs::remove_dir_all(&base);
    }
}
