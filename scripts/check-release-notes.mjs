// 校验 Release 说明的生成结果：直接跑一遍 release-notes.mjs 并检查产物。
//
// 存在的理由：说明的格式（尤其是结尾的文件表与下载链接）是发布流程的
// 对外契约，而它在工作流里排在**全部六个平台构建之后**才执行——
// 表格一旦写错，要等整轮多平台构建跑完才会发现。这个脚本把这一步提前，
// 由前端检查任务顺手验证（几秒），本地也能随时跑。
//
// 用法：node scripts/check-release-notes.mjs
//
// 注意：release-notes.mjs 会调用 GitHub API 查对比范围与资产，
// 因此运行需要网络。

import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const repo = "heiheids/schematic-manager";
const tag = "v0.1.1";
const ver = "0.1.1";

/** 资产名清单，与 collect-artifacts.mjs 的命名规则、以及各平台实际产物一致。
 *
 * 取值对照已发布的 Release（v1.0.1+snapshot.60 共 8 个文件）：
 * Windows 出 setup.exe，macOS 出 dmg，Linux 同时出 deb 与 tar.gz。
 * 因此这里是 8 项而不是 6 项——Linux 的两种格式都要覆盖，
 * 尤其 tar.gz 的双点扩展名（`.tar.gz`）与 `-setup` 后缀，
 * 它们走的是 parseAsset 里不同的正则分支。
 */
const assets = [
  `schematic-manager-${ver}-windows-x86_64-setup.exe`,
  `schematic-manager-${ver}-windows-arm64-setup.exe`,
  `schematic-manager-${ver}-linux-x86_64.deb`,
  `schematic-manager-${ver}-linux-x86_64.tar.gz`,
  `schematic-manager-${ver}-linux-arm64.deb`,
  `schematic-manager-${ver}-linux-arm64.tar.gz`,
  `schematic-manager-${ver}-macos-x86_64.dmg`,
  `schematic-manager-${ver}-macos-arm64.dmg`,
];

const dir = mkdtempSync(join(tmpdir(), "notes-check-"));
const outFile = join(dir, "notes.md");

function run(assetsJson) {
  return spawnSync(process.execPath, [join(here, "release-notes.mjs")], {
    cwd: root,
    env: {
      ...process.env,
      GITHUB_REPOSITORY: repo,
      TAG: tag,
      VERSION: ver,
      PRERELEASE: "false",
      ASSETS_JSON: JSON.stringify(assetsJson),
      OUTPUT: outFile,
      // 透传 token：release-notes.mjs 一律会调用 GitHub API（对比提交、
      // 回退查询资产），匿名调用有额度限制，CI 里容易撞限流而误报失败。
      // 因此这一步需要网络；没有 token 时也能跑，只是额度更少。
      GITHUB_TOKEN: process.env.GITHUB_TOKEN || "",
      GH_TOKEN: process.env.GH_TOKEN || "",
    },
    encoding: "utf8",
  });
}

let failed = 0;
function check(label, ok, extra = "") {
  console.log(`${ok ? "[通过]" : "[失败]"} ${label}${ok ? "" : " —— " + extra}`);
  if (!ok) failed++;
}

// 第一轮：正常六平台
const r = run(assets);
check("脚本正常退出", r.status === 0, `退出码 ${r.status}，stderr: ${(r.stderr || "").slice(-300)}`);
const text = readFileSync(outFile, "utf8");

check(
  "含表头 Platform / 平台 | Architecture / 架构 | Format / 格式 | File / 文件",
  text.includes("| Platform / 平台 | Architecture / 架构 | Format / 格式 | File / 文件 |"),
);
// 表格列顺序是「平台 | 架构」，不要写反
for (const [plat, arch] of [
  ["Windows", "x86_64"], ["Windows", "arm64"],
  ["Linux", "x86_64"], ["Linux", "arm64"],
  ["macOS", "x86_64"], ["macOS", "arm64"],
]) {
  const re = new RegExp(`\\| ${plat} \\| ${arch} \\|`);
  check(`表格含 ${plat} / ${arch} 一行`, re.test(text));
}
// 下载链接必须是 https://github.com/<user>/<repo>/releases/download/<tag>/<file>
const badUrl = text.match(/\((https?:\/\/[^)]*)\)/g)?.filter((u) => !u.includes(`https://github.com/${repo}/releases/download/${tag}/`)) ?? [];
check("全部下载链接指向 releases/download/<tag>/<file>", badUrl.length === 0, badUrl.join(", "));
check("每个资产都出现在表格中", assets.every((a) => text.includes(a)));

// Format 列的取值：这两条覆盖 parseAsset 里两个易错分支——
// `-setup.exe` 要拆出 exe 而不是把 setup 当成格式，
// `.tar.gz` 的双点扩展名不能被截成 gz。
check(
  "安装包显示为 EXE (安装包)",
  /\| Windows \| x86_64 \| EXE \(安装包\) \|/.test(text),
  "Format 列未按预期渲染",
);
check(
  "tar.gz 的格式列完整显示为 tar.gz",
  /\| Linux \| x86_64 \| tar\.gz \|/.test(text),
  "双点扩展名可能被截断",
);

// 第二轮：畸形输入不应让脚本崩溃（表格少几行不该阻塞整个发布）
const bad = run([{ file: "x.exe" }, 42, null, "nonsense", assets[4]]);
check("畸形资产不导致崩溃", bad.status === 0, `退出码 ${bad.status}`);
const badText = readFileSync(outFile, "utf8");
check("畸形输入下仍产出合法行", badText.includes(assets[4]));

rmSync(dir, { recursive: true, force: true });
console.log(failed === 0 ? "\n全部通过。" : `\n有 ${failed} 项失败。`);
process.exit(failed ? 1 : 0);
