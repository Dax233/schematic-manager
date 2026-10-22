//! 应用数据目录的搬迁。
//!
//! bundle identifier 决定应用数据目录（`app_data_dir`、`app_config_dir`）的
//! 位置。identifier 从 `com.heiheids.schematic-manager` 换成
//! `io.github.heiheids.schematic-manager` 之后，已装用户原有的数据全留在旧
//! 目录里：方块库体积可达数百 MB，要靠联网下载或扫本机客户端才能补齐，界面
//! 状态也会重置——对用户来说就是「更新完东西全没了」。所以要在启动时把旧
//! 目录里的内容搬进新位置。
//!
//! 搬迁必须容忍目标目录已经存在：Tauri 在调用用户的 `.setup()` **之前**就按
//! 配置把窗口建好了（`app.rs` 的 `fn setup` 先创建窗口，之后才调用 setup
//! 闭包），而窗口在 Linux/Windows 上会强制把 `app_local_data_dir()` 当作
//! webview 数据目录并 `create_dir_all` 出来（`manager/webview.rs`）；Linux 上
//! `dirs::data_local_dir()` 与 `dirs::data_dir()` 是同一个目录，那正好就是
//! 应用数据目录。因此这里逐项搬入，而不给整个目录改名。
//!
//! 移除条件：旧 identifier 不再有任何在用的发布时，删掉本模块以及 lib.rs 里
//! 的调用（当前最后一个使用旧 identifier 的发布是 v0.1.1-dev.15）。
//!
//! 两个实例同时首次启动时，后启动的那个可能在搬迁中途失败（对手抢先搬走了同
//! 一项），于是它启动失败。数据不会受损，重开即可——项目没有单实例插件，这种
//! 并发是可能的，但代价只是重试一次，不值得为它引入锁。

use crate::error::{AppError, AppResult};
use std::path::Path;

/// 旧 identifier，也就是旧数据目录的目录名。
const LEGACY_IDENTIFIER: &str = "com.heiheids.schematic-manager";

/// 把 `old_root` 里的条目逐个搬进 `new_root`。
///
/// 目标里已有同名条目时保留目标那一份、旧的那份留在原地：哪一份才是用户想要
/// 的无法自动判断，替他选就可能丢掉方块库。旧目录搬空后删掉，还留着同名条目
/// 时保留原地，下次启动会再试一次。
fn take_over(old_root: &Path, new_root: &Path) -> AppResult<()> {
    if !old_root.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(new_root)?;

    for entry in std::fs::read_dir(old_root)? {
        let entry = entry?;
        let dest = new_root.join(entry.file_name());
        if dest.exists() {
            continue;
        }
        std::fs::rename(entry.path(), &dest)
            .map_err(|e| migration_failed(&entry.path(), &dest, &e))?;
    }

    if std::fs::read_dir(old_root)?.next().is_none() {
        std::fs::remove_dir(old_root).map_err(|e| migration_failed(old_root, new_root, &e))?;
    }
    Ok(())
}

/// 搬迁失败的错误，带上新旧路径：用户要照着提示自己把数据挪回去。
fn migration_failed(from: &Path, to: &Path, cause: &std::io::Error) -> AppError {
    AppError::Io(std::io::Error::new(
        cause.kind(),
        format!("把 {} 搬到 {} 失败：{cause}", from.display(), to.display()),
    ))
}

/// 迁移旧 identifier 留下的数据目录。
///
/// 应用数据目录与配置目录分别处理：Windows 上两者是同一个目录，第一次搬完
/// 之后第二次无事可做（旧目录已经不在了）。
pub fn migrate_legacy_data(app: &tauri::AppHandle) -> AppResult<()> {
    use tauri::Manager;

    for new_root in [app.path().app_data_dir(), app.path().app_config_dir()] {
        let new_root =
            new_root.map_err(|e| AppError::Parse(format!("无法定位应用目录：{e}")))?;
        let parent = new_root.parent().ok_or_else(|| {
            AppError::Parse(format!("应用目录没有上级目录：{}", new_root.display()))
        })?;
        take_over(&parent.join(LEGACY_IDENTIFIER), &new_root)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// 测试用临时目录。放在工程内的 target 下，与其余模块的测试保持一致，
    /// 避免依赖系统临时目录的可写性。
    fn temp_dir(tag: &str) -> PathBuf {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join(format!("sm_data_dir_{tag}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 旧、新两个目录名，以及它们共同的上级目录。
    fn layout(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
        let base = temp_dir(tag);
        let old = base.join(LEGACY_IDENTIFIER);
        let new = base.join("io.github.heiheids.schematic-manager");
        (base, old, new)
    }

    #[test]
    fn take_over_moves_contents_into_a_target_that_already_exists() {
        // Linux 上目标目录会被 webview 先建出来（空壳），搬迁仍须进行
        let (_base, old, new) = layout("existing");
        let blocks = old.join("base").join("blocks");
        std::fs::create_dir_all(&blocks).unwrap();
        std::fs::write(blocks.join("1.20.dat.z"), b"lib").unwrap();
        std::fs::create_dir_all(&new).unwrap();

        take_over(&old, &new).unwrap();

        assert_eq!(
            std::fs::read(new.join("base").join("blocks").join("1.20.dat.z")).unwrap(),
            b"lib",
            "目标目录已存在（哪怕是空的）时也要把内容搬过去"
        );
        assert!(!old.exists(), "搬空之后旧目录应被删掉");
    }

    #[test]
    fn take_over_resumes_a_partial_move() {
        let (_base, old, new) = layout("resume");
        // 上一次迁移搬走了一项就退出了：那一项已经不在旧目录里了
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("state.json"), b"ui").unwrap();
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(new.join("blocks.dat.z"), b"lib").unwrap();

        take_over(&old, &new).unwrap();

        assert_eq!(std::fs::read(new.join("state.json")).unwrap(), b"ui");
        assert_eq!(
            std::fs::read(new.join("blocks.dat.z")).unwrap(),
            b"lib",
            "已经搬过去的那一份不该被覆盖"
        );
        assert!(!old.exists(), "剩下的都搬走后旧目录应被删掉");
    }

    #[test]
    fn take_over_keeps_both_copies_when_a_name_exists_on_both_sides() {
        let (_base, old, new) = layout("conflict");
        std::fs::create_dir_all(old.join("base")).unwrap();
        std::fs::write(old.join("base").join("old.txt"), b"old").unwrap();
        std::fs::create_dir_all(new.join("base")).unwrap();
        std::fs::write(new.join("base").join("new.txt"), b"new").unwrap();

        take_over(&old, &new).unwrap();

        assert_eq!(std::fs::read(new.join("base").join("new.txt")).unwrap(), b"new");
        assert_eq!(
            std::fs::read(old.join("base").join("old.txt")).unwrap(),
            b"old",
            "同名条目不能覆盖目标里已有的那份，也不能把它删掉"
        );
    }

    #[test]
    fn take_over_without_the_old_dir_changes_nothing() {
        let (_base, old, new) = layout("none");

        take_over(&old, &new).unwrap();

        assert!(!old.exists());
        assert!(!new.exists(), "没有旧目录时不该凭空建出新目录");
    }
}
