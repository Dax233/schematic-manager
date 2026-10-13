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

## 故障排查

**双击后没有任何反应？**

程序启动失败时会在两个位置留一份 `启动失败.log`，并弹出提示窗口：

1. 程序所在目录（例如 `C:\Program Files\投影文件管理器\`）
2. 若上处不可写，则退到系统临时目录

日志里含失败原因、出错位置与程序路径。

**提示「WebView2 错误」或「拒绝访问」**

程序界面依赖系统的 Microsoft Edge WebView2 运行时（Windows 11 自带）。
遇到这类错误时，多为其用户数据目录损坏，可删除后重试：

```
%LOCALAPPDATA%\com.heiheids.schematic-manager
```

若仍无法启动，请安装 WebView2 运行时：
<https://developer.microsoft.com/microsoft-edge/webview2/>

**无人值守环境**

环境变量 `SCHEMATIC_MANAGER_NO_ERROR_DIALOG=1` 可让启动失败只写日志、
不弹窗。适用于自动化脚本或计划任务——弹窗是模态的，无人点击时会一直
等待。默认不设置，普通用户不受影响。

## 项目结构

- `src/` —— Vue 前端
- `src-tauri/` —— Rust 后端
  - `src-tauri/resources/blocks.dat.z` —— 内置方块库（zlib 压缩，约 1.6 MB）
  - `src-tauri/tests/data/` —— 测试用的真实 `.litematic` 样本
- `legacy-cpp/` —— 原有的 Windows 专用 C++ 实现，**仅作参考**。
  其中的 `.litematic` 解析、NBT 位打包、ZIP/PNG 解码、多版本方块库烘焙等
  已通过实测验证的语义，是 Rust 重写时的对照依据。
- `tools/typecheck/` —— 无网络环境下的离线类型检查与单元测试脚本
- `tools/render/` —— 用无头浏览器渲染构建产物、核对界面确实画得出来
- `.github/workflows/release.yml` —— 多平台自动构建与发布

## 构建

需要 Node.js（含 pnpm）与 Rust 工具链。

```bash
pnpm install
pnpm tauri dev      # 开发
pnpm tauri build    # 打包
```

前端类型检查、单元测试与构建：

```bash
pnpm typecheck       # vue-tsc
pnpm test            # vitest：API 契约 + 界面挂载与交互流程
pnpm check:ipc       # 前后端命令名与参数名的一致性检查
pnpm check:notes     # Release 说明的格式（文件表与下载链接）
pnpm check:workflow  # 工作流结构：needs / outputs / 矩阵键 / 表达式 / concurrency
pnpm build           # vite build
```

`pnpm check:notes` 与 `pnpm check:workflow` 都不联网、也不需要 token，
不受仓库当前 Release 状态影响，可在本地随时运行。

`check:workflow` 补的是「YAML 能解析 ≠ GitHub 能执行」这段差距：
`needs` 指向不存在的任务、引用未声明的 output、矩阵键没定义、
表达式括号不配对、矩阵里未加引号的逗号（会被当成条目分隔符）——
这些只在推送之后才由 GitHub 报出来，而本工作流的构建阶段要跑六个平台。

后端的单元测试：

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

仓库内只随源码分发一个小体积样本（见 `src-tauri/tests/data/README.md`）。
若要拿自己收藏的真实投影核验各功能的覆盖面，用这几个按需运行的用例：

```bash
SM_REAL_DIR=/path/to/projects \
  cargo test --manifest-path src-tauri/Cargo.toml --lib -- --ignored --nocapture
```

对目录下每个真实投影依次核对：

- 载入、区域结构自检、NBT 往返零信息损失、方块统计
- 用随包分发的方块库构建 3D 场景，并报出未能解析的方块数
- 方块替换：替换 → 写出 → 读回，核对体积与方块总数不变
- 版本转换：转换 → 写出 → 读回，核对 dataVersion 已改写且体积不变
- 查重：副本被判为重复、内容不同的文件不被误判

未设置 `SM_REAL_DIR` 时会直接失败，不会静默跳过。

界面渲染检查（需先 `pnpm build`，且本机装有 Chrome / Edge / Chromium）：

```bash
python tools/render/check-render.py
```

它用无头浏览器加载 dist 产物、自动走一遍真实操作路径
「添加目录 → 扫描 → 选中文件 → 逐个打开五个功能对话框」，
再从渲染结果里核对界面文案。单元测试跑在 jsdom 里，看不到真实浏览器中的
渲染：构建配置写错时 `vite build` 依旧成功、dist 也生成了，打开却是一片
空白。这一步专门守这种情况。

对话框单独验：它们打开时会各自加载数据（可转换版本、资源包清单、备份列表），
一旦抛错，界面表现就是「点了没反应」——最难排查的一类问题。
浏览器路径可用 `CHROME_PATH` 指定。

## 版本号规则

- 预发布：`v<MAJOR>.<MINOR>.<PATCH>+snapshot.<构建次数>`（推送自动生成）
- 正式版：`v<MAJOR>.<MINOR>.<PATCH>`（手动打 tag 触发，随后仓库内各版本号
  自动提升到下一个 PATCH）

「下一个 PATCH」以**所打的 tag** 为基准：`v2.0.0` 之后仓库内版本号变为
`2.0.1`（而不是「仓库原版本 + 1」）。两者在常规流程下一致，但直接发一个
与仓库版本不同的 tag 时，只有以 tag 为基准才不会让版本号倒退。

仓库内的版本号共四处，由 `scripts/bump-version.mjs` 一并改写：
`package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`
与 `src-tauri/Cargo.lock` 里本包那一条。

各平台的产物格式：Windows 为 NSIS 安装包（`-setup.exe`），macOS 为 DMG，
Linux 为 deb。

推送提交会触发多平台构建并以预发布形式上传；手动打正式版 tag 时以该版本号
构建发布，Release 说明按提交信息自动归类生成。
