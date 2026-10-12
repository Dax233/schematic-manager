//! 仅用于本机类型检查，不参与实际构建。
//! 通过 #[path] 引用 src-tauri/src 下的真实文件；error 与 flate2 用桩替代。
#[path = "error_stub.rs"] mod error;
#[path = "../../src-tauri/src/nbt.rs"] mod nbt;
#[path = "../../src-tauri/src/mcver.rs"] mod mcver;
#[path = "../../src-tauri/src/litematic.rs"] mod litematic;
#[path = "../../src-tauri/src/blockdb.rs"] mod blockdb;
#[path = "../../src-tauri/src/block_model.rs"] mod block_model;
#[path = "../../src-tauri/src/geometry.rs"] mod geometry;
#[path = "../../src-tauri/src/camera.rs"] mod camera;
#[path = "../../src-tauri/src/visibility.rs"] mod visibility;
#[path = "../../src-tauri/src/backup.rs"] mod backup;
