mod backup;
mod block_model;
mod block_search;
mod blockstate;
mod blockstate_chain;
mod blockdb;
mod blockdb_write;
mod camera;
mod commands;
mod convert;
mod dto;
mod duplicate;
mod error;
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
        ])
        .run(tauri::generate_context!());

    if let Err(e) = result {
        startup::report(&e.to_string());
    }
}
