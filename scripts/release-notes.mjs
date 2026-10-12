#!/usr/bin/env node
/**
 * 生成 GitHub Release 的更新说明。
 *
 * 收集规则：
 *   - latest-release（正式版）：收集到「上一个 latest-release」
 *   - pre-release（预发布）：收集到「上一个 latest-release 或 pre-release」（取更近者）
 *
 * 结尾附文件表：Platform / 平台 | Architecture / 架构 | Format / 格式 | File / 文件
 * File 列超链接到 https://github.com/<user>/<repo>/releases/download/<tag>/<file>
 *
 * 环境变量：
 *   GITHUB_TOKEN / GH_TOKEN   必填
 *   GITHUB_REPOSITORY         必填，形如 user/repo
 *   TAG                       必填，本次发布的 tag（同时用于拼下载链接）
 *   HEAD_REF                  可选，用于提交对比的 ref，默认取 TAG。
 *                             快照发布时 tag 尚未创建，必须传 github.sha
 *   PRERELEASE                可选，'true' 表示预发布
 *   ASSETS_JSON               可选，资产文件名的 JSON 数组（形如 ["a.zip", ...]）
 *   OUTPUT                    可选，输出文件路径，默认 release-notes.md
 */
import { writeFileSync, readFileSync, existsSync } from "node:fs";

const REPO = process.env.GITHUB_REPOSITORY || "";
const TAG = process.env.TAG || "";
// 快照发布的 tag 由 gh release create 创建，生成说明时它还不存在于远端，
// 因此对比要用提交 sha；下载链接仍然用 tag。
const HEAD_REF = process.env.HEAD_REF || TAG;
const PRERELEASE = String(process.env.PRERELEASE || "false") === "true";
const OUTPUT = process.env.OUTPUT || "release-notes.md";
const TOKEN = process.env.GITHUB_TOKEN || process.env.GH_TOKEN || "";

if (!REPO || !TAG) {
  console.error("缺少 GITHUB_REPOSITORY 或 TAG");
  process.exit(1);
}

const API = "https://api.github.com";
const headers = {
  Accept: "application/vnd.github+json",
  "X-GitHub-Api-Version": "2022-11-28",
  ...(TOKEN ? { Authorization: `Bearer ${TOKEN}` } : {}),
  "User-Agent": "release-notes-generator",
};

async function api(path) {
  const res = await fetch(`${API}/repos/${REPO}${path}`, { headers });
  if (!res.ok) {
    // 404 属于"没有上一版"的正常情况，交由调用方处理
    if (res.status === 404) return null;
    throw new Error(`GitHub API ${res.status} ${path}: ${await res.text()}`);
  }
  return res.json();
}

/** 取全部 release（分页），按创建时间倒序 */
async function listReleases() {
  const all = [];
  for (let page = 1; page <= 5; page++) {
    const chunk = await api(`/releases?per_page=100&page=${page}`);
    if (!chunk || chunk.length === 0) break;
    all.push(...chunk);
    if (chunk.length < 100) break;
  }
  return all;
}

/**
 * 找到本次对比的起点 tag。
 * 正式版 -> 上一个 latest-release
 * 预发布 -> 上一个 latest-release 或 pre-release（两者中更近的一个）
 */
function pickPrevious(releases, isPrerelease) {
  const candidates = releases.filter((r) => r.tag_name !== TAG);
  if (!isPrerelease) {
    const prev = candidates.find((r) => !r.prerelease);
    return prev ? prev.tag_name : null;
  }
  // 预发布：正式版与预发布都算，取创建时间最新的一个
  if (candidates.length === 0) return null;
  const sorted = [...candidates].sort(
    (a, b) => new Date(b.created_at) - new Date(a.created_at),
  );
  return sorted[0].tag_name;
}

const CONVENTIONAL_SECTIONS = [
  { key: "feat", title: "### ✨ 新增" },
  { key: "fix", title: "### 🐛 修复" },
  { key: "perf", title: "### ⚡ 性能" },
  { key: "refactor", title: "### ♻️ 重构" },
  { key: "style", title: "### 💄 样式" },
  { key: "docs", title: "### 📝 文档" },
  { key: "build", title: "### 📦 构建" },
  { key: "ci", title: "### 🤖 持续集成" },
  { key: "test", title: "### ✅ 测试" },
  { key: "chore", title: "### 🔧 杂项" },
];

const BREAKING_RE = /^(\w+)(\([^)]*\))?!:/;

/** 把提交列表按 conventional-commit 类型分组 */
function groupCommits(commits) {
  const groups = new Map(CONVENTIONAL_SECTIONS.map((s) => [s.key, []]));
  const breaking = [];
  const other = [];

  for (const c of commits) {
    const subject = String(c.commit?.message || "").split("\n")[0].trim();
    if (!subject) continue;
    // 跳过合并提交
    if (/^Merge (branch|pull request)/i.test(subject)) continue;

    const m = subject.match(/^(\w+)(?:\(([^)]*)\))?(!)?:\s*(.+)$/);
    if (!m) {
      other.push(subject);
      continue;
    }
    const [, type, scope, bang, desc] = m;
    const text = scope ? `**${scope}**: ${desc}` : desc;
    if (bang || BREAKING_RE.test(subject)) breaking.push(text);
    if (groups.has(type)) groups.get(type).push(text);
    else other.push(subject);
  }
  return { groups, breaking, other };
}

const PLATFORM_LABEL = { windows: "Windows", linux: "Linux", macos: "macOS" };
const ARCH_LABEL = { x86_64: "x86_64", arm64: "arm64", universal: "universal" };

/** 从规范化资产名解析平台/架构/格式：<name>-<ver>-<platform>-<arch>[-suffix].<ext> */
function parseAsset(name, ver) {
  const re = new RegExp(
    `^schematic-manager-${ver.replace(/\./g, "\\.")}-(windows|linux|macos)-(x86_64|arm64|universal)(?:-[^.]+)*\\.(.+)$`,
  );
  const m = name.match(re);
  if (!m) return null;
  const [, platform, arch, ext] = m;
  const FORMAT_LABEL = {
    exe: "EXE (安装包)", msi: "MSI", zip: "ZIP", "tar.gz": "tar.gz",
    deb: "deb", rpm: "rpm", AppImage: "AppImage", dmg: "DMG", app: "APP",
  };
  return {
    platform: PLATFORM_LABEL[platform] || platform,
    arch: ARCH_LABEL[arch] || arch,
    format: FORMAT_LABEL[ext] || ext,
  };
}

/** 生成结尾文件表 */
function buildTable(assets, ver) {
  const lines = [
    "| Platform / 平台 | Architecture / 架构 | Format / 格式 | File / 文件 |",
    "|---|---|---|---|",
  ];
  const rows = [];
  for (const name of assets) {
    const info = parseAsset(name, ver);
    if (!info) continue;
    const url = `https://github.com/${REPO}/releases/download/${TAG}/${encodeURIComponent(name)}`;
    rows.push(
      `| ${info.platform} | ${info.arch} | ${info.format} | [${name}](${url}) |`,
    );
  }
  if (rows.length === 0) return "";
  // 按 平台 -> 架构 -> 格式 稳定排序
  rows.sort();
  return [...lines, ...rows].join("\n");
}

async function main() {
  const releases = await listReleases();
  const prev = pickPrevious(releases, PRERELEASE);

  let commits = [];
  if (prev) {
    const cmp = await api(`/compare/${prev}...${HEAD_REF}`);
    if (cmp && Array.isArray(cmp.commits)) commits = cmp.commits;
  } else {
    // 没有上一版：取到仓库首个提交为止
    const list = await api(`/commits?sha=${HEAD_REF}&per_page=100`);
    if (Array.isArray(list)) commits = list.reverse();
  }

  const { groups, breaking, other } = groupCommits(commits);

  const version = TAG.replace(/^v/, "");
  const out = [];
  out.push(`## ${version}${PRERELEASE ? "（预发布）" : ""}`);
  out.push("");
  if (PRERELEASE) {
    out.push("> 这是由每次推送自动生成的快照构建，可能不稳定，请勿用于生产环境。");
    out.push("");
  }
  out.push(prev ? `对比范围：\`${prev}...${TAG}\`` : "初始版本");
  out.push("");

  for (const s of CONVENTIONAL_SECTIONS) {
    const items = groups.get(s.key);
    if (!items || items.length === 0) continue;
    out.push(s.title);
    for (const it of items) out.push(`- ${it}`);
    out.push("");
  }

  if (other.length > 0) {
    out.push("### 📌 其它");
    for (const it of other) out.push(`- ${it}`);
    out.push("");
  }

  if (breaking.length > 0) {
    out.push("### ⚠️ 破坏性改动");
    for (const it of breaking) out.push(`- ${it}`);
    out.push("");
  }

  let assets = [];
  if (process.env.ASSETS_JSON) {
    try { assets = JSON.parse(process.env.ASSETS_JSON); } catch { assets = []; }
  } else if (existsSync("assets.json")) {
    try { assets = JSON.parse(readFileSync("assets.json", "utf8")); } catch { assets = []; }
  } else {
    // 回退：直接问 GitHub 当前 release 的资产
    const rel = await api(`/releases/tags/${TAG}`);
    if (rel && Array.isArray(rel.assets)) assets = rel.assets.map((a) => a.name);
  }

  const table = buildTable(assets, version);
  if (table) {
    out.push("---");
    out.push("");
    out.push(table);
    out.push("");
  }

  const text = out.join("\n").replace(/\n{3,}/g, "\n\n");
  writeFileSync(OUTPUT, text, "utf8");
  console.log(`已生成 ${OUTPUT}`);
  console.log(text);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
