//! 仅用于本机类型检查，不参与实际构建。
//!
//! 注意：本 crate 只包含纯逻辑模块，不含 commands.rs / dto.rs，
//! 因此编译器会把「被 commands 使用」的项误报为 dead_code。
//! 这里关掉该 lint —— 本 crate 的职责是类型检查，不是死代码分析。
#![allow(dead_code)]
//! 通过 #[path] 引用 src-tauri/src 下的真实文件；error 与 flate2 用桩替代。
#[path = "error_stub.rs"] mod error;
#[path = "../../src-tauri/src/duplicate.rs"] mod duplicate;
#[path = "../../src-tauri/src/mesh.rs"] mod mesh;
#[path = "../../src-tauri/src/nbt.rs"] mod nbt;
#[path = "../../src-tauri/src/png.rs"] mod png;
#[path = "../../src-tauri/src/mcver.rs"] mod mcver;
#[path = "../../src-tauri/src/litematic.rs"] mod litematic;
#[path = "../../src-tauri/src/blockdb.rs"] mod blockdb;
#[path = "../../src-tauri/src/blockdb_write.rs"] mod blockdb_write;
#[path = "../../src-tauri/src/block_model.rs"] mod block_model;
#[path = "../../src-tauri/src/block_search.rs"] mod block_search;
#[path = "../../src-tauri/src/blockstate.rs"] mod blockstate;
#[path = "../../src-tauri/src/blockstate_chain.rs"] mod blockstate_chain;
#[path = "../../src-tauri/src/geometry.rs"] mod geometry;
#[path = "../../src-tauri/src/camera.rs"] mod camera;
#[path = "../../src-tauri/src/convert.rs"] mod convert;
#[path = "../../src-tauri/src/replace.rs"] mod replace;
#[path = "../../src-tauri/src/scene.rs"] mod scene;
#[path = "../../src-tauri/src/transform.rs"] mod transform;
#[path = "../../src-tauri/src/visibility.rs"] mod visibility;
#[path = "../../src-tauri/src/zip.rs"] mod zip;
#[path = "../../src-tauri/src/backup.rs"] mod backup;
