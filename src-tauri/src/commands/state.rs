//! 界面状态的读写。

use std::path::{Path, PathBuf};
use tauri::Manager;


// ---------------------------------------------------------------- 界面状态记忆

/// 界面状态文件名。内容是一段前端自己解释的 JSON。
const UI_STATE_FILE: &str = "ui-state.json";

/// 读取界面状态文件；文件不存在返回 `None`（首次启动的正常情况，不是错误）。
///
/// 与定位路径分开：路径依赖运行时的 AppHandle，而读写逻辑本身
/// （尤其是「不存在」与「读失败」的区分）是可以独立验证的。
pub(crate) fn read_ui_state_file(path: &Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("读取界面状态 {} 失败：{e}", path.display())),
    }
}

/// 原子写入界面状态文件。
///
/// 先写同目录的临时文件再改名：直接覆盖时若中途失败（磁盘满、进程被杀）
/// 会留下半截 JSON，下次启动解析失败就等于把用户所有界面设置清零。
/// 改成同目录内的改名在主流文件系统上是原子的，因此读到的要么是旧内容、
/// 要么是完整的新内容。
pub(crate) fn write_ui_state_file(path: &Path, json: &str) -> Result<(), String> {
    // 临时文件必须与目标同目录：跨目录改名不保证原子，甚至可能失败
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json.as_bytes())
        .map_err(|e| format!("写入 {} 失败：{e}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .map_err(|e| format!("替换 {} 失败：{e}", path.display()))
}

/// 应用配置目录。
///
/// 放在系统的应用配置目录（`%APPDATA%\<bundle-id>` 等）而不是程序目录：
/// 程序可能装在 Program Files 这类只读位置，而「记住界面」失败不该影响使用。
/// 用 Tauri 核心的 PathResolver 取，不需要额外插件。
pub(crate) fn config_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("无法定位配置目录：{e}"))?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("无法创建配置目录 {}：{e}", dir.display()))?;
    Ok(dir)
}

/// 界面状态的存放路径。
pub(crate) fn ui_state_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(config_dir(app)?.join(UI_STATE_FILE))
}

/// 读取上次保存的界面状态；从未保存过时返回 None（不是错误）。
#[tauri::command]
pub fn load_ui_state(app: tauri::AppHandle) -> Result<Option<String>, String> {
    read_ui_state_file(&ui_state_path(&app)?)
}

/// 保存界面状态。
#[tauri::command]
pub fn save_ui_state(app: tauri::AppHandle, json: String) -> Result<(), String> {
    write_ui_state_file(&ui_state_path(&app)?, &json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::shared::temp_state_dir;

    #[test]
    fn ui_state_round_trips_through_the_file() {
        let dir = temp_state_dir("rt");
        let path = dir.join(UI_STATE_FILE);
        let json = r#"{"version":1,"sidebarWidth":820,"dirs":["C:/D1"]}"#;

        write_ui_state_file(&path, json).expect("写入失败");
        let got = read_ui_state_file(&path).expect("读取失败");
        assert_eq!(got.as_deref(), Some(json), "写进去的应能原样读回来");

        // 覆盖写：第二次必须替换掉第一次，而不是追加或残留
        let json2 = r#"{"version":1,"sidebarWidth":666,"dirs":[]}"#;
        write_ui_state_file(&path, json2).expect("覆盖写入失败");
        assert_eq!(
            read_ui_state_file(&path).expect("读取失败").as_deref(),
            Some(json2),
            "第二次保存应替换旧内容"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn ui_state_missing_file_is_not_an_error() {
        let dir = temp_state_dir("missing");
        let path = dir.join(UI_STATE_FILE);
        // 首次启动：文件还不存在。这是正常情况，必须返回 None 而不是错误，
        // 否则每次首启都会弹出一个「读取界面状态失败」的告警。
        let got = read_ui_state_file(&path).expect("文件不存在不应视为错误");
        assert!(got.is_none(), "未保存过时应返回 None");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn ui_state_write_leaves_no_temp_file_behind() {
        let dir = temp_state_dir("tmp");
        let path = dir.join(UI_STATE_FILE);
        write_ui_state_file(&path, "{}").expect("写入失败");
        // 临时文件必须已被改名带走：残留会在下次写入时产生
        // 「目标已存在」之类的意外，也会让用户目录里留下垃圾
        assert!(!path.with_extension("json.tmp").exists(), "不应残留临时文件");
        let names: Vec<String> = std::fs::read_dir(&dir)
            .expect("列目录失败")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec![UI_STATE_FILE.to_string()], "目录里应只剩状态文件本身");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn ui_state_failed_write_keeps_previous_content() {
        let dir = temp_state_dir("fail");
        let path = dir.join(UI_STATE_FILE);
        let good = r#"{"version":1,"sidebarWidth":700}"#;
        write_ui_state_file(&path, good).expect("首次写入失败");

        // 用一个必然失败的目标模拟「写盘中途失败」：
        // 把临时文件的位置占成一个目录，写入就会失败。
        // 关键是**原有内容必须完好**——否则用户所有界面设置会被清零。
        std::fs::create_dir_all(path.with_extension("json.tmp")).expect("占位失败");
        let r = write_ui_state_file(&path, r#"{"version":1,"sidebarWidth":1}"#);
        assert!(r.is_err(), "临时文件写不进去时应报错，而不是静默成功");
        assert_eq!(
            read_ui_state_file(&path).expect("读取失败").as_deref(),
            Some(good),
            "写入失败后原有内容必须保持不变"
        );

        std::fs::remove_dir_all(&dir).ok();
    }
}
