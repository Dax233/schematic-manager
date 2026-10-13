// 校验 Release 说明的生成结果：跑一遍 release-notes.mjs，检查产物。
//
// 存在的理由：说明的格式与「对比范围」的取值规则是发布流程的对外契约，
// 而它在工作流里排在**全部六个平台构建之后**才执行——一旦写错，
// 要等整轮多平台构建跑完才会发现，重来一次代价很高。
// 这个脚本把这一步提前，由前端检查任务顺手验证（几秒）。
//
// 为什么自建一个假的 GitHub API 而不是直接问真的：
//   1. release-notes.mjs 的行为取决于仓库里已有哪些 Release。真仓库里
//      只有正式版、或只有预发布，都无法构造；用夹具才能把「上一个正式版」
//      「上一个预发布」「两者取更近者」「一个都没有」这四种情形都覆盖到。
//   2. 匿名调用 GitHub API 有额度限制，CI 里容易撞限流而误报失败。
//   3. 本地与 CI 得到的结果完全一致，不受仓库状态变化影响。
//
// 用法：node scripts/check-release-notes.mjs

import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const repo = "heiheids/schematic-manager";
const SCRIPT = join(here, "release-notes.mjs");

const dir = mkdtempSync(join(tmpdir(), "notes-check-"));
const outFile = join(dir, "notes.md");

let failed = 0;
function check(label, ok, extra = "") {
  console.log(`${ok ? "[通过]" : "[失败]"} ${label}${ok ? "" : " —— " + extra}`);
  if (!ok) failed++;
}

// ---------------------------------------------------------------- 假的 GitHub API

/** 当前夹具：Release 列表（按创建时间倒序，与 GitHub 返回顺序一致） */
let releases = [];

/** 请求过的路径，用来确认脚本真的按预期查了对应端点 */
const requested = [];

const server = createServer((req, res) => {
  const url = new URL(req.url, "http://127.0.0.1");
  const path = url.pathname;
  requested.push(path);
  const send = (code, body) => {
    res.writeHead(code, { "content-type": "application/json" });
    res.end(JSON.stringify(body));
  };

  // 分页取 Release 列表
  if (path.endsWith("/releases")) {
    const page = url.searchParams.get("page") || "1";
    send(200, page === "1" ? releases : []);
    return;
  }
  // 提交对比
  const cmp = path.match(/\/compare\/(.+?)\.\.\.(.+)$/);
  if (cmp) {
    send(200, {
      commits: [
        { commit: { message: "feat(ui): 新增示例功能" } },
        { commit: { message: "fix(png): 修复示例缺陷" } },
      ],
    });
    return;
  }
  send(404, { message: "Not Found" });
});

await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const apiBase = `http://127.0.0.1:${server.address().port}`;

/**
 * 跑一次 release-notes.mjs。
 *
 * 用异步 spawn 而不是 spawnSync：本进程同时还在跑那个假 API 的 HTTP 服务，
 * spawnSync 会阻塞事件循环，子进程的请求永远等不到响应。
 */
function runNotes({ tag, ver, prerelease, assetsJson }) {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, [SCRIPT], {
      cwd: root,
      env: {
        ...process.env,
        GITHUB_REPOSITORY: repo,
        GITHUB_API_BASE: apiBase,
        TAG: tag,
        VERSION: ver,
        PRERELEASE: String(prerelease),
        ASSETS_JSON: JSON.stringify(assetsJson),
        OUTPUT: outFile,
        // 清掉 token：假 API 不需要，也避免误用真凭据
        GITHUB_TOKEN: "",
        GH_TOKEN: "",
      },
    });
    let stderr = "";
    child.stderr.on("data", (d) => (stderr += d.toString()));
    child.on("close", (status) => resolve({ status, stderr }));
  });
}

/** 构造一个 Release 夹具 */
function rel(tagName, prerelease, createdAt) {
  return { tag_name: tagName, prerelease, created_at: createdAt };
}

// ---------------------------------------------------------------- 资产夹具

/** 各平台实际产物：Windows 出 setup.exe，macOS 出 dmg，Linux 出 deb。
 *
 * 这里**没有** tar.gz。早先的 Release 里确实有一份
 * `schematic-manager-<ver>-linux-<arch>.tar.gz`，但那不是便携包：
 * 它是 deb 的内部成员 data.tar.gz 被 collect-artifacts.mjs 误收后重命名的
 * 产物（两个内部成员还重名互相覆盖）。修正收集规则后 Linux 只剩 deb。
 */
const assets = [
  "schematic-manager-0.1.1-windows-x86_64-setup.exe",
  "schematic-manager-0.1.1-windows-arm64-setup.exe",
  "schematic-manager-0.1.1-linux-x86_64.deb",
  "schematic-manager-0.1.1-linux-arm64.deb",
  "schematic-manager-0.1.1-macos-x86_64.dmg",
  "schematic-manager-0.1.1-macos-arm64.dmg",
];

// ---------------------------------------------------------------- 用例

/** 跑一次并返回说明正文 */
async function notesFor(opts) {
  const r = await runNotes({ assetsJson: assets, ...opts });
  if (r.status !== 0) return { text: "", r };
  return { text: readFileSync(outFile, "utf8"), r };
}

/** 从说明正文里取出「对比范围」那一行 */
function rangeOf(text) {
  const m = text.match(/对比范围：`([^`]+)`/) || text.match(/^(初始版本)$/m);
  return m ? m[1] : null;
}

// 一、正式版：应对比「上一个正式版」，并跳过中间那些预发布
releases = [
  rel("v0.1.1+snapshot.7", true, "2026-01-03T00:00:00Z"),
  rel("v0.1.0", false, "2026-01-01T00:00:00Z"),
];
{
  const { text, r } = await notesFor({ tag: "v0.1.1", ver: "0.1.1", prerelease: false });
  check("正式版：脚本正常退出", r.status === 0, `退出码 ${r.status}：${r.stderr.slice(-200)}`);
  check(
    "正式版：对比范围是上一个**正式版**（跳过更近的预发布）",
    rangeOf(text) === "v0.1.0...v0.1.1",
    `实际 ${rangeOf(text)}`,
  );
  check(
    "正式版：结尾含表头 Platform / 平台 | Architecture / 架构 | Format / 格式 | File / 文件",
    text.includes("| Platform / 平台 | Architecture / 架构 | Format / 格式 | File / 文件 |"),
  );
  // 表格列顺序是「平台 | 架构」，不要写反
  for (const [plat, arch] of [
    ["Windows", "x86_64"], ["Windows", "arm64"],
    ["Linux", "x86_64"], ["Linux", "arm64"],
    ["macOS", "x86_64"], ["macOS", "arm64"],
  ]) {
    check(`正式版：表格含 ${plat} / ${arch} 一行`, new RegExp(`\\| ${plat} \\| ${arch} \\|`).test(text));
  }
  // 下载链接必须是 https://github.com/<user>/<repo>/releases/download/<tag>/<file>
  const badUrl =
    text.match(/\((https?:\/\/[^)]*)\)/g)?.filter((u) =>
      !u.includes(`https://github.com/${repo}/releases/download/v0.1.1/`),
    ) ?? [];
  check("正式版：全部下载链接指向 releases/download/<tag>/<file>", badUrl.length === 0, badUrl.join(", "));
  check("正式版：每个资产都出现在表格中", assets.every((a) => text.includes(a)));
  check(
    "正式版：安装包格式列显示为 EXE (安装包)（不把 setup 当成格式）",
    /\| Windows \| x86_64 \| EXE \(安装包\) \|/.test(text),
  );
  check("正式版：deb 格式列显示为 deb", /\| Linux \| x86_64 \| deb \|/.test(text));
  // 提交按 conventional-commit 类型分组
  check("正式版：含「新增」分组", text.includes("### ✨ 新增"));
  check("正式版：含「修复」分组", text.includes("### 🐛 修复"));
}

// 二、预发布：应对比「上一正式版与上一预发布中更近的那个」
releases = [
  rel("v0.1.1+snapshot.7", true, "2026-01-03T00:00:00Z"),
  rel("v0.1.0", false, "2026-01-01T00:00:00Z"),
];
{
  const { text, r } = await notesFor({ tag: "v0.1.1+snapshot.8", ver: "0.1.1", prerelease: true });
  check("预发布：脚本正常退出", r.status === 0, `退出码 ${r.status}：${r.stderr.slice(-200)}`);
  check(
    "预发布：对比范围取更近的预发布",
    rangeOf(text) === "v0.1.1+snapshot.7...v0.1.1+snapshot.8",
    `实际 ${rangeOf(text)}`,
  );
  check("预发布：带预发布提示语", text.includes("快照构建"));
}

// 三、预发布：上一版是正式版时，应对比该正式版
releases = [
  rel("v0.1.1+snapshot.7", true, "2026-01-01T00:00:00Z"),
  rel("v0.1.0", false, "2026-01-03T00:00:00Z"),
];
{
  const { text } = await notesFor({ tag: "v0.1.1+snapshot.8", ver: "0.1.1", prerelease: true });
  check(
    "预发布：上一版是正式版时对比该正式版（两者取更近者）",
    rangeOf(text) === "v0.1.0...v0.1.1+snapshot.8",
    `实际 ${rangeOf(text)}`,
  );
}

// 四、一个上一版都没有：应标为初始版本
releases = [];
{
  const { text, r } = await notesFor({ tag: "v0.1.0", ver: "0.1.0", prerelease: false });
  check("首个版本：脚本正常退出", r.status === 0, `退出码 ${r.status}：${r.stderr.slice(-200)}`);
  check("首个版本：标注为初始版本", rangeOf(text) === "初始版本", `实际 ${rangeOf(text)}`);
}

// 五、畸形资产不应让脚本崩溃（表格少几行不该阻塞整个发布）
{
  const r = await runNotes({
    tag: "v0.1.1",
    ver: "0.1.1",
    prerelease: false,
    assetsJson: [{ file: "x.exe" }, 42, null, "nonsense", assets[2]],
  });
  check("畸形资产不导致崩溃", r.status === 0, `退出码 ${r.status}`);
  const text = readFileSync(outFile, "utf8");
  check("畸形输入下仍产出合法行", text.includes(assets[2]));
}

server.close();
rmSync(dir, { recursive: true, force: true });
console.log(failed === 0 ? "\n全部通过。" : `\n有 ${failed} 项失败。`);
process.exit(failed ? 1 : 0);
