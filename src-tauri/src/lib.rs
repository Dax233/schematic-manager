mod error;

/// 应用入口。以库形式导出，便于将来接入移动端入口。
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .invoke_handler(tauri::generate_handler![greet, app_version])
        .run(tauri::generate_context!())
        .expect("启动 Tauri 应用失败");
}

/// 连通性自检用命令
#[tauri::command]
fn greet(name: &str) -> String {
    format!("你好，{name}！")
}

/// 返回后端版本号，供前端「关于」展示
#[tauri::command]
fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greet_works() {
        assert_eq!(greet("世界"), "你好，世界！");
    }

    #[test]
    fn version_is_not_empty() {
        assert!(!app_version().is_empty());
    }
}
