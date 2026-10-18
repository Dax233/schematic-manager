// Release 说明分类器与链接渲染的测试。
// 运行：node --test scripts/release-notes-render.test.mjs
import { test } from "node:test";
import assert from "node:assert/strict";
import { classify, render } from "./release-notes-render.mjs";

const TAG = "v0.0.1-dev.42";
const REPO = "heiheids/schematic-manager";

const COMMITS = ["feat(ui): 新增示例功能", "fix(png): 修复示例缺陷", "ci: 调整发布流程"];

// 各平台的真实产物名：前缀取自 productName，dev 构建的版本号带 -dev.N 段
const ASSETS = [
  "schematic-manager_0.0.1-dev.42_x64-setup.exe",
  "schematic-manager_0.0.1-dev.42_arm64-setup.exe",
  "schematic-manager_0.0.1-dev.42_amd64.deb",
  "schematic-manager_0.0.1-dev.42_arm64.deb",
  "schematic-manager_0.0.1-dev.42_x64.dmg",
  "schematic-manager_0.0.1-dev.42_aarch64.dmg",
];

test("分类器能把各平台产物名映射到平台/架构/格式", () => {
  const expected = {
    "schematic-manager_0.0.1_x64-setup.exe": { platform: "Windows", arch: "x86_64", format: "EXE (安装包)" },
    "schematic-manager_0.0.1_arm64-setup.exe": { platform: "Windows", arch: "arm64", format: "EXE (安装包)" },
    "schematic-manager_0.0.1_amd64.deb": { platform: "Linux", arch: "x86_64", format: "deb" },
    "schematic-manager_0.0.1_arm64.deb": { platform: "Linux", arch: "arm64", format: "deb" },
    "schematic-manager_0.0.1_x64.dmg": { platform: "macOS", arch: "x86_64", format: "DMG" },
    "schematic-manager_0.0.1_aarch64.dmg": { platform: "macOS", arch: "arm64", format: "DMG" },
  };
  for (const [name, exp] of Object.entries(expected)) {
    const c = classify(name);
    assert.ok(c, `应能分类 ${name}`);
    assert.equal(c.platform, exp.platform);
    assert.equal(c.arch, exp.arch);
    assert.equal(c.format, exp.format);
  }
});

test("分类器拒绝非产物文件", () => {
  for (const bad of [
    "README.md",
    "schematic-manager_0.0.1_x64-setup.exe.sig",
    "schematic-manager_0.0.1_x64.exe",
    "totally-unrelated.zip",
    42,
    null,
  ]) {
    assert.equal(classify(bad), null, `${bad} 不应被分类`);
  }
});

test("每个产物行都是指向资产下载地址的可点链接，资产名经过 URL 编码", () => {
  const body = render(TAG, ASSETS, COMMITS, { repo: REPO });
  for (const name of ASSETS) {
    const url = `https://github.com/${REPO}/releases/download/${TAG}/${encodeURIComponent(name)}`;
    const esc = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    const row = new RegExp(`\\| [^|]+ \\| [^|]+ \\| [^|]+ \\| \\[${esc(name)}\\]\\(${esc(url)}\\)`);
    assert.match(body, row, `应有 ${name} 的可点链接行`);
  }
});

test("渲染结果含双语下载表头与提交列表", () => {
  const body = render(TAG, ASSETS, COMMITS, { repo: REPO });
  assert.ok(body.includes("## 下载"));
  assert.ok(body.includes("| Platform / 平台 | Architecture / 架构 | Format / 格式 | File / 文件 |"));
  assert.ok(body.includes("## What's Changed"));
  for (const c of COMMITS) {
    assert.ok(body.includes(`* ${c}`), `应有提交行 ${c}`);
  }
});

test("dev tag 带预发布提示，正式 tag 不带", () => {
  assert.ok(render(TAG, ASSETS, COMMITS).includes("开发构建"));
  assert.ok(!render("v0.0.1", ASSETS, COMMITS).includes("开发构建"));
});

test("仅提交模式省略下载表、保留提交列表", () => {
  // 初始正文在 Release 创建前渲染，此时还没有任何资产
  const body = render(TAG, null, COMMITS, { repo: REPO });
  assert.ok(!body.includes("## 下载"));
  assert.ok(body.includes("## What's Changed"));
  for (const c of COMMITS) {
    assert.ok(body.includes(`* ${c}`), `应有提交行 ${c}`);
  }
});

test("空提交列表也能渲染出 What's Changed 小节", () => {
  const body = render(TAG, ASSETS, [], { repo: REPO });
  assert.ok(body.includes("## What's Changed"));
});
