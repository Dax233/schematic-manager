# 离线类型检查

本目录是一套**离线类型检查工具**，用于在没有网络、无法使用 `cargo`（拉不到
crates.io 依赖）的环境下，检查纯逻辑模块是否存在真实编译错误。

## 为什么需要它

项目的 Rust 侧依赖 `tauri`、`serde`、`flate2`、`thiserror` 等 crate。若本机
无法访问 crates.io，`cargo check` 直接失败，导致**没有任何编译反馈通道**，
只能靠人工审读——而人工审读漏掉的错误会一直累积到 CI 才爆发。

类型检查本身不需要真实依赖实现，只需要**接口形状**。因此这里用最小桩替代
`flate2` 与 `thiserror`，再用 `rustc` 直接检查源码。

## 用法

```powershell
pwsh tools/typecheck/run.ps1
```

退出码为错误数量，可直接用于脚本判断。

## 覆盖范围

通过 `#[path]` 引用 `src-tauri/src` 下的**真实文件**，不复制代码：

```
error（用桩）  nbt  mcver  litematic  blockdb  block_model
geometry  camera  visibility  backup
```

含以上模块的全部单元测试（以 `--test` 编译）。

**不覆盖**：`commands.rs`、`dto.rs`、`lib.rs`、`main.rs` —— 它们依赖
`tauri` 与 `serde`（其中 `serde` 的衍生宏无法用桩替代）。

## 局限（务必了解）

- **只做类型检查**：不链接、不运行，因此**测试不会被真正执行**。它只能证明
  「代码能编译」，不能证明「行为正确」。行为仍需 CI 执行 `cargo test`。
- 桩只提供被用到的 API 表面。若代码新用了 `flate2` 或 `thiserror` 的其它接口，
  需要同步补桩，否则会报"方法不存在"的假错误。
- 不检查 `commands.rs` 等 Tauri 相关文件。
- 真正的验证仍是 CI 上的完整构建与测试。
