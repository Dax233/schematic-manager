#!/usr/bin/env node
/**
 * Release 说明渲染器与产物分类器。
 * 由 scripts/update-release-notes.sh（CLI）与
 * scripts/release-notes-render.test.mjs（测试）共用。
 *
 * 产物名由 tauri-action 按 Tauri 原生命名直传，形如：
 *   schematic-manager_0.1.1-dev.42_x64-setup.exe   （Windows NSIS）
 *   schematic-manager_0.1.1_amd64.deb              （Linux deb）
 *   schematic-manager_0.1.1_aarch64.dmg            （macOS dmg）
 * 前缀取自 tauri.conf.json 的 productName，改名字时前缀跟着变，
 * 因此分类只按**后缀**匹配，不依赖前缀。
 * 下载地址用 encodeURIComponent 转义资产名，保证拼出的是合法 URL。
 */

/**
 * 把 Release 资产文件名映射为 { platform, arch, format, name }，
 * 不认识的名字返回 null。
 */
export function classify(name) {
  if (typeof name !== "string") return null;
  // NSIS 安装包带 -setup 后缀；裸 _x64.exe 不是本仓库的产物
  const m = /_(x64|arm64|amd64|aarch64)(-setup\.exe|\.deb|\.dmg)$/.exec(name);
  if (!m) return null;
  const [, archRaw, suffix] = m;
  const ext = suffix === "-setup.exe" ? "exe" : suffix.slice(1);
  const PLATFORM_OF_EXT = { exe: "Windows", deb: "Linux", dmg: "macOS" };
  const FORMAT_LABEL = { exe: "EXE (安装包)", deb: "deb", dmg: "DMG" };
  return {
    name,
    platform: PLATFORM_OF_EXT[ext],
    arch: archRaw === "amd64" || archRaw === "x64" ? "x86_64" : "arm64",
    format: FORMAT_LABEL[ext],
  };
}

const KIND_ORDER = { exe: 0, deb: 1, dmg: 2 };
const EXT_OF = (name) => name.slice(name.lastIndexOf(".") + 1);

/**
 * 渲染双语下载表与「What's Changed」提交列表。每个产物行都直接链接到
 * 该资产的下载地址。assets 为 null 时（仅提交模式，用于打包完成前的
 * 初始正文）省略下载表，改为放置提示语。
 *
 * tag 形如 v0.0.1-dev.42 时附带预发布提示。
 */
export function render(tag, assets, commits, { repo = "REPO" } = {}) {
  const commitLines = (commits ?? []).map((c) => `* ${c}`).join("\n");
  const prereleaseNote = tag.includes("-dev.")
    ? "> 这是由每次推送自动生成的开发构建，可能不稳定，请勿用于生产环境。\n\n"
    : "";

  if (assets === null) {
    return `${prereleaseNote}_产物正在构建中，下载表将在打包完成后出现在这里。_

---

## What's Changed

${commitLines}
`;
  }

  const sorted = [...assets]
    .filter((a) => classify(a))
    .sort((a, b) => {
      const c1 = classify(a);
      const c2 = classify(b);
      if (c1.platform !== c2.platform) return c1.platform.localeCompare(c2.platform);
      if (c1.arch !== c2.arch) return c1.arch.localeCompare(c2.arch);
      return KIND_ORDER[EXT_OF(c1.name)] - KIND_ORDER[EXT_OF(c2.name)];
    });

  const link = (name) =>
    `https://github.com/${repo}/releases/download/${tag}/${encodeURIComponent(name)}`;
  const rows = sorted.map((name) => {
    const c = classify(name);
    return `| ${c.platform} | ${c.arch} | ${c.format} | [${name}](${link(name)}) |`;
  });

  return `${prereleaseNote}## 下载

| Platform / 平台 | Architecture / 架构 | Format / 格式 | File / 文件 |
| --- | --- | --- | --- |
${rows.join("\n")}

---

## What's Changed

${commitLines}
`;
}

// CLI 模式：
//   node scripts/release-notes-render.mjs <tag> <assets-json> <commits-file>
//   node scripts/release-notes-render.mjs <tag> - <commits-file>   （仅提交）
import { fileURLToPath, pathToFileURL } from "node:url";
import { readFileSync } from "node:fs";
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [, , tag, assetsJson, commitsFile] = process.argv;
  const assets = assetsJson === "-" ? null : JSON.parse(assetsJson);
  const commits = commitsFile
    ? readFileSync(commitsFile, "utf8").split("\n").map((s) => s.trim()).filter(Boolean)
    : [];
  const repo = process.env.GITHUB_REPOSITORY ?? "REPO";
  if (assets !== null) {
    // 不认识的资产名直接失败：分类器漏了新产物格式时说明里会静默少行，
    // 宁可让整个发布停下来，也不要发出一份缺文件的说明
    const unknown = assets.filter((a) => !classify(a));
    if (unknown.length > 0) {
      console.error(`产物分类器不认识这些资产：${unknown.join(", ")}`);
      process.exit(1);
    }
  }
  process.stdout.write(render(tag, assets, commits, { repo }));
}
