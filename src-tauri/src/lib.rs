mod backup;
mod block_model;
mod block_search;
mod blockstate;
mod blockstate_chain;
mod blockdb;
mod blockdb_write;
mod commands;
mod convert;
mod dto;
mod duplicate;
mod error;
mod file_ops;
mod geometry;
mod jar_file;
mod jar_index;
mod litematic;
mod mcver;
mod mesh;
mod model_pack;
mod nbt;
mod png;
mod pool;
mod pack_build;
mod pack_registry;
mod replace;
// 针对任意真实投影集合的端到端核对，按需运行（标了 #[ignore]）
#[cfg(test)]
mod real_project;
mod scene;
mod startup;
mod transform;
mod visibility;
mod zip;

/// 应用入口。以库形式导出，便于将来接入移动端入口。
///
/// 启动失败必须有可见反馈：发布版以 `windows_subsystem = "windows"`
/// 编译、没有控制台，错误信息无处可见，用户双击后只会看到
/// 「什么都没发生」。实测 WebView2 用户数据目录损坏时正是这种表现。
///
/// 因此同时装两道防线：
///   - panic 钩子：Tauri 建窗口失败时是 panic，不是返回 Err
///   - `run()` 返回 Err 时的处理：覆盖不 panic 的失败路径
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    startup::install_panic_hook();

    let result = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .invoke_handler(tauri::generate_handler![
            commands::peek_schematic,
            commands::scan_litematics,
            commands::analyze_schematic,
            commands::mc_version_name,
            commands::build_scene,
            commands::list_backups,
            commands::restore_backup,
            commands::delete_backup,
            commands::read_operation_log,
            commands::search_blocks,
            commands::list_block_versions,
            commands::preview_replace,
            commands::apply_replace,
            commands::scan_duplicates,
            commands::convert_targets,
            commands::analyze_conversion,
            commands::apply_conversion,
            commands::save_metadata,
            commands::list_packs,
            commands::import_jar,
            commands::set_pack_enabled,
            commands::delete_pack,
            commands::delete_files,
            commands::copy_files_to_clipboard,
            commands::copy_files_to_dir,
            commands::reveal_in_file_manager,
        ])
        .setup(|app| {
            disable_native_context_menu(app);
            Ok(())
        })
        .run(tauri::generate_context!());

    if let Err(e) = result {
        startup::report(&e.to_string());
    }
}

/// 关掉 WebView2 自带的右键菜单。
///
/// 不做这件事时，用户在表格上点右键会看到浏览器那一套
/// 「返回 / 刷新 / 另存为 / 打印 / 更多工具」——这是 WebView2 的默认行为，
/// 出现在桌面应用里既突兀又无用，还会盖住我们自己的菜单。
/// 用户实测反馈的就是它。
///
/// Tauri 2 没有把 wry 的 `with_default_context_menus` 透出来，
/// 因此用 `with_webview` 拿到控制器后自己调
/// `ICoreWebView2Settings::SetAreDefaultContextMenusEnabled(false)`——
/// wry 内部关闭该菜单走的也是这一步
/// （wry-0.57.0/src/webview2/mod.rs 的 set_webview_settings）。
///
/// 失败只记日志、不中断启动：右键菜单关不掉不影响核心功能，
/// 为它让程序起不来是本末倒置。
#[cfg(windows)]
fn disable_native_context_menu(app: &tauri::App) {
    // get_webview_window 由 Manager trait 提供
    use tauri::Manager;

    let Some(win) = app.get_webview_window("main") else {
        eprintln!("未找到主窗口，跳过禁用 WebView2 右键菜单");
        return;
    };

    let result = win.with_webview(|pw| {
        // SAFETY: 闭包由 Tauri 在 webview 线程上执行，此时控制器有效。
        // 每个 COM 调用都检查返回值，任一失败就放弃并说明原因，
        // 不做「静默当作成功」。
        let controller = pw.controller();
        let core = match unsafe { controller.CoreWebView2() } {
            Ok(v) => v,
            Err(e) => {
                eprintln!("取 CoreWebView2 失败，系统右键菜单可能仍会出现：{e}");
                return;
            }
        };
        let settings = match unsafe { core.Settings() } {
            Ok(v) => v,
            Err(e) => {
                eprintln!("取 WebView2 设置失败，系统右键菜单可能仍会出现：{e}");
                return;
            }
        };
        match unsafe { settings.SetAreDefaultContextMenusEnabled(false) } {
            Ok(()) => {}
            Err(e) => eprintln!("禁用 WebView2 右键菜单失败：{e}"),
        }
    });

    if let Err(e) = result {
        eprintln!("注册 WebView2 设置回调失败：{e}");
    }
}

/// 非 Windows 平台无需处理（其余平台没有 WebView2 的原生右键菜单）。
#[cfg(not(windows))]
fn disable_native_context_menu(_app: &tauri::App) {}
