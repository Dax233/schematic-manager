//! 命令层与 DTO 的离线类型检查。
//!
//! 这两个文件此前从未被编译验证过（依赖 tauri 与 serde 的过程宏），
//! 而它们恰恰最容易出错——跨模块接口调用、DTO 字段与转换。
//! 实测手工核对两轮仍漏掉了接口名写错与导入缺失。
//!
//! 做法：剥掉过程宏属性（gen_cmd_check.py），用独立 crate 形式的
//! tauri / serde 桩替代依赖，再把全部业务模块挂进来编译。
//! 被检查到的是函数体本身，也就是风险所在。
//!
//! 覆盖不到的部分（明确记录，不假装覆盖）：
//!   - #[tauri::command] 宏展开（参数名映射、返回值序列化）
//!   - serde 的字段重命名与序列化行为
//!   - lib.rs 的 generate_handler! 列表，由 CI 的 cargo 构建覆盖
#![allow(dead_code)]
#![allow(unused_imports)]

// 与真实 crate 同名同路径地挂载业务模块，保证接口一致
#[path = "error_stub.rs"] mod error;
#[path = "../../src-tauri/src/backup.rs"] mod backup;
#[path = "../../src-tauri/src/block_model.rs"] mod block_model;
#[path = "../../src-tauri/src/block_search.rs"] mod block_search;
#[path = "../../src-tauri/src/blockstate.rs"] mod blockstate;
#[path = "../../src-tauri/src/blockstate_chain.rs"] mod blockstate_chain;
#[path = "../../src-tauri/src/blockdb.rs"] mod blockdb;
#[path = "../../src-tauri/src/blockdb_write.rs"] mod blockdb_write;
#[path = "../../src-tauri/src/camera.rs"] mod camera;
#[path = "../../src-tauri/src/convert.rs"] mod convert;
#[path = "../../src-tauri/src/duplicate.rs"] mod duplicate;
#[path = "../../src-tauri/src/geometry.rs"] mod geometry;
#[path = "../../src-tauri/src/jar_file.rs"] mod jar_file;
#[path = "../../src-tauri/src/jar_index.rs"] mod jar_index;
#[path = "../../src-tauri/src/litematic.rs"] mod litematic;
#[path = "../../src-tauri/src/mcver.rs"] mod mcver;
#[path = "../../src-tauri/src/mesh.rs"] mod mesh;
#[path = "../../src-tauri/src/model_pack.rs"] mod model_pack;
#[path = "../../src-tauri/src/nbt.rs"] mod nbt;
#[path = "../../src-tauri/src/png.rs"] mod png;
#[path = "../../src-tauri/src/pool.rs"] mod pool;
#[path = "../../src-tauri/src/pack_build.rs"] mod pack_build;
#[path = "../../src-tauri/src/pack_registry.rs"] mod pack_registry;
#[path = "../../src-tauri/src/replace.rs"] mod replace;
#[path = "../../src-tauri/src/scene.rs"] mod scene;
#[path = "../../src-tauri/src/transform.rs"] mod transform;
#[path = "../../src-tauri/src/visibility.rs"] mod visibility;
#[path = "../../src-tauri/src/zip.rs"] mod zip;

// 待检查的两个文件（生成副本，宏属性已剥离）
#[path = "generated/dto.rs"] mod dto;
#[path = "generated/commands.rs"] mod commands;

fn main() {}
