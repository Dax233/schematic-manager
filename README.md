# 投影文件管理器 / Schematic Manager
<img width="1560" height="980" alt="2026-10-12_010941" src="https://github.com/user-attachments/assets/d7855b1d-78b4-4957-9537-6c146bfee77f" />

跨平台的 Minecraft 投影（`.litematic`）文件管理与预览工具。

> 本程序仅限于免费分享，禁止任何销售行为。
> 作者：黑山大叔 — https://space.bilibili.com/19428259

## 技术栈

| 层 | 选型 |
|---|---|
| 桌面框架 | Tauri 2 |
| 前端 | Vite + Vue 3 + TypeScript |
| 样式 | Sass |
| 组件库 | Arco Design Vue |
| 后端 | Rust |

## 平台支持

| 操作系统 | x86_64 | arm64 |
|---|---|---|
| Windows | ✅ | ✅ |
| Linux | ✅ | ✅ |
| macOS | ✅ | ✅ |

## 项目状态

正在从原 Windows 专用 C++ 实现迁移为跨平台应用。

- `legacy-cpp/` —— 原有 C++ 实现，**仅作参考**。
  其中的 `.litematic` 解析、NBT 位打包、ZIP/PNG 解码、多版本方块库烘焙等
  已通过实测验证的语义，是 Rust 重写时的对照依据。
- `src/` —— Vue 前端
- `src-tauri/` —— Rust 后端

## 构建

```bash
pnpm install
pnpm tauri dev      # 开发
pnpm tauri build    # 打包
```

## 版本号规则

- 预发布：`v<MAJOR>.<MINOR>.<PATCH>+snapshot.<构建次数>`（推送自动生成）
- 正式版：`v<MAJOR>.<MINOR>.<PATCH>`（手动打 tag 触发）
