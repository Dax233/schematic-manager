#!/usr/bin/env node
/**
 * 把仓库内的版本号统一改写为指定版本。
 *
 * 涉及四处，必须保持一致：
 *   1. package.json               -> "version"
 *   2. src-tauri/Cargo.toml       -> [package] version
 *   3. src-tauri/tauri.conf.json  -> "version"
 *   4. src-tauri/Cargo.lock       -> 本包（name = "schematic-manager"）的 version
 *
 * 用法：
 *   node scripts/bump-version.mjs 1.0.1              # 指定版本
 *   node scripts/bump-version.mjs 1.0.1-dev.42       # 指定预发布版本（CI 盖章用，
 *                                                    # 不写回仓库，仅构建工作区）
 *   node scripts/bump-version.mjs --next             # 仓库当前版本 PATCH + 1
 *   node scripts/bump-version.mjs --next-of 2.0.0    # 指定版本 PATCH + 1
 *                                                    # （正式发布用 tag 的版本作基准）
 */
import { readFileSync, writeFileSync, existsSync } from "node:fs";

const PKG = "package.json";
const CARGO = "src-tauri/Cargo.toml";
const CONF = "src-tauri/tauri.conf.json";
const LOCK = "src-tauri/Cargo.lock";

function readJson(p) { return JSON.parse(readFileSync(p, "utf8")); }

function currentVersion() {
  return readJson(PKG).version;
}

/** PATCH 位 +1，MAJOR/MINOR 保持不变 */
export function nextPatch(v) {
  const m = String(v).trim().match(/^(\d+)\.(\d+)\.(\d+)/);
  if (!m) throw new Error(`无法解析版本号: ${v}`);
  const [, major, minor, patch] = m;
  return `${major}.${minor}.${Number(patch) + 1}`;
}

function setPackage(v) {
  const j = readJson(PKG);
  j.version = v;
  writeFileSync(PKG, JSON.stringify(j, null, 2) + "\n", "utf8");
}

function setCargo(v) {
  const src = readFileSync(CARGO, "utf8");
  // 只改 [package] 段落里的第一个 version，避免误伤 [dependencies] 中的版本
  const out = src.replace(
    /(\[package\][\s\S]*?\nversion\s*=\s*)"[^"]*"/,
    `$1"${v}"`,
  );
  if (out === src && !src.includes(`version = "${v}"`)) {
    throw new Error("在 Cargo.toml 中未找到 [package] version 字段");
  }
  writeFileSync(CARGO, out, "utf8");
}

function setConf(v) {
  const j = readJson(CONF);
  j.version = v;
  writeFileSync(CONF, JSON.stringify(j, null, 2) + "\n", "utf8");
}

/**
 * 同步 Cargo.lock 里本包的版本号。
 *
 * 锁文件里也有一份本包版本（`[[package]]` 下的 `name` / `version`）。
 * 不改它的话，正式发布后 Cargo.toml 与 Cargo.lock 会长期不一致——
 * 构建时 cargo 会改写锁文件，但那发生在工作区里，不会提交回仓库，
 * 于是仓库永远停在旧版本上（用 `--locked` 构建时会直接失败）。
 *
 * 只改本包那一条，依赖项的版本保持原样：用 `name` 紧邻的前缀锚定，
 * 避免误伤同名的依赖或其它条目。
 *
 * 行尾用 `\r?\n`：Windows 上 `core.autocrlf=true`（Git for Windows 安装时的
 * 默认选项之一）会把检出的锁文件变成 CRLF，而 Cargo.toml 那条正则因为有
 * `[\s\S]*?` 兜着、不受影响。若这里写死 `\n`，本地执行会以一个看不出原因的
 * 「未找到 version 字段」失败——文件里明明有。
 */
function setLock(v) {
  // 锁文件还没生成时（例如刚克隆后未构建）跳过，不算错误
  if (!existsSync(LOCK)) return;
  const src = readFileSync(LOCK, "utf8");
  const out = src.replace(
    /(\[\[package\]\]\r?\nname = "schematic-manager"\r?\nversion = )"[^"]*"/,
    `$1"${v}"`,
  );
  // 已经是目标版本时替换不会生效，这属于幂等的情形，不能当成「找不到字段」
  const already = src
    .replace(/\r\n/g, "\n")
    .includes(`name = "schematic-manager"\nversion = "${v}"`);
  if (out === src && !already) {
    throw new Error("在 Cargo.lock 中未找到本包的 version 字段");
  }
  writeFileSync(LOCK, out, "utf8");
}

function apply(v) {
  setPackage(v);
  setCargo(v);
  setConf(v);
  setLock(v);
  console.log(`版本号已统一为 ${v}`);
}

// 作为 CLI 运行时才执行
const isCli = process.argv[1] && process.argv[1].replace(/\\/g, "/").endsWith("scripts/bump-version.mjs");
if (isCli) {
  const arg = process.argv[2] || "--next";
  // 先定出「基准版本」，再由它推下一个 PATCH；直接给版本号时不推
  let base = null;
  let v = null;
  if (arg === "--next") {
    base = currentVersion();
  } else if (arg === "--next-of") {
    // 从**指定的**版本推下一个 PATCH，而不是从仓库当前版本推。
    //
    // 正式发布时必须以 tag 的版本为基准：需求规定「把仓库内各版本号 dump 到
    // <MAJOR>.<MINOR>.<PATCH+1>」，其中的 MAJOR/MINOR/PATCH 来自 tag。
    // 若按仓库当前版本推，打一个与仓库版本不同的 tag（例如直接发 2.0.0，
    // 而仓库里还是 1.0.1）就会算成 1.0.2——版本号在已发布的 2.0.0 之后
    // 反而**倒退**，之后所有快照的版本号也跟着乱。
    base = process.argv[3];
    if (!base) {
      console.error("--next-of 需要给出基准版本号，例如 --next-of 2.0.0");
      process.exit(1);
    }
    // 幂等守卫：清单版本已经不是基准版本时（重跑、或人工已递增过）
    // 直接跳过，绝不能重复递增或把版本号往回改
    const current = currentVersion();
    if (current !== base) {
      console.log(`清单版本 ${current} 与已发布版本 ${base} 不一致，跳过递增`);
      process.exit(0);
    }
  } else {
    v = arg;
  }

  if (v === null) {
    try {
      v = nextPatch(base);
    } catch (e) {
      // 基准版本号本身格式不对（含 package.json 损坏的情形）：
      // 给出可读的一句话，而不是把 JS 堆栈甩给使用者
      console.error(`无法从 ${base} 推算下一版本号：${e.message}`);
      process.exit(1);
    }
  }
  // 允许预发布段（CI 给 dev 构建盖章 0.0.1-dev.42）；
  // 但 --next / --next-of 推出来的永远是纯 X.Y.Z，不受此影响
  if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(v)) {
    console.error(`版本号必须是 <MAJOR>.<MINOR>.<PATCH>[-预发布段] 形式，收到: ${v}`);
    process.exit(1);
  }
  apply(v);
}
