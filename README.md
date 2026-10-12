# 投影文件管理器 / Schematic Manager
<img width="1560" height="980" alt="2026-10-12_010941" src="https://github.com/user-attachments/assets/d7855b1d-78b4-4957-9537-6c146bfee77f" />

跨平台的 Minecraft 投影（`.litematic`）文件管理与预览工具。

> 本程序仅限于免费分享，禁止任何销售行为。
> 作者：黑山大叔 — https://space.bilibili.com/19428259

## 功能

| 功能 | 说明 |
|---|---|
| 投影列表 | 选定目录后递归扫描其中的 `.litematic`，显示尺寸、方块数、制作版本等概要 |
| 3D 预览 | 用真实方块模型与贴图渲染投影，支持多视角切换与导出透明背景 PNG |
| 方块统计 | 按方块与方块状态统计数量；另可查看实体与方块实体 |
| 方块替换 | 按规则把一种方块替换为另一种；替换前自动备份，写回后可用备份还原 |
| 版本转换 | 把投影转换到另一个游戏版本：先分析差异，再为缺失的方块指定替代品，结果另存为新文件 |
| 重复文件筛查 | 在目录内按**文件内容**找出重复的投影 |
| 备份管理 | 列出每次改动前的备份，可还原或删除；另附操作日志 |
| 资源包管理 | 导入 Minecraft 客户端或模组 jar，为内置方块库补充或叠加贴图与模型 |
| 元数据编辑 | 修改投影的名称、作者与描述 |

投影文件本身在改动前都会先备份，备份位于程序目录下的 `backups/`。

## 关于贴图与方块库

程序内置 1.13.2 ~ 26.3 共 49 个正式版的贴图与方块模型，覆盖常见投影，
**通常无需手动导入**。

以下情形才需要导入：

- 投影用到了模组方块（模组贴图不在原版资源里）
- 需要比内置更新版本的贴图

导入时选**客户端** jar（`assets/`、`models/`、`textures/` 所在的那个文件，
一般位于 `.minecraft/versions/<版本>/<版本>.jar`）。**服务端 jar 没有贴图资源**，
导入后不会有任何效果。

导入的包放在程序目录下的 `packs/`，可用「资源包管理」启用、停用或删除；
改动即时生效，无需重启程序。

## 技术栈

| 层 | 选型 |
|---|---|
| 桌面框架 | Tauri 2 |
| 前端 | Vite + Vue 3 + TypeScript |
| 样式 | Sass |
| 组件库 | Arco Design Vue |
| 后端 | Rust（除压缩外，`.litematic`、NBT、ZIP、PNG 的解析全部自行实现） |

## 平台支持

| 操作系统 | x86_64 | arm64 |
|---|---|---|
| Windows | ✅ | ✅ |
| Linux | ✅ | ✅ |
| macOS | ✅ | ✅ |

## 项目结构

- `src/` —— Vue 前端
- `src-tauri/` —— Rust 后端
  - `src-tauri/resources/blocks.dat.z` —— 内置方块库（zlib 压缩，约 1.6 MB）
  - `src-tauri/tests/data/` —— 测试用的真实 `.litematic` 样本
- `legacy-cpp/` —— 原有的 Windows 专用 C++ 实现，**仅作参考**。
  其中的 `.litematic` 解析、NBT 位打包、ZIP/PNG 解码、多版本方块库烘焙等
  已通过实测验证的语义，是 Rust 重写时的对照依据。
- `tools/typecheck/` —— 无网络环境下的离线类型检查与单元测试脚本
- `.github/workflows/release.yml` —— 多平台自动构建与发布

## 构建

需要 Node.js（含 pnpm）与 Rust 工具链。

```bash
pnpm install
pnpm tauri dev      # 开发
pnpm tauri build    # 打包
```

前端类型检查与构建：

```bash
pnpm typecheck      # vue-tsc
pnpm build          # vite build
```

## 版本号规则

- 预发布：`v<MAJOR>.<MINOR>.<PATCH>+snapshot.<构建次数>`（推送自动生成）
- 正式版：`v<MAJOR>.<MINOR>.<PATCH>`（手动打 tag 触发，随后仓库内各版本号
  自动提升到下一个 PATCH）

推送提交会触发多平台构建并以预发布形式上传；手动打正式版 tag 时以该版本号
构建发布，Release 说明按提交信息自动归类生成。
