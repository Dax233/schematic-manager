#!/usr/bin/env node
/**
 * 把仓库内的版本号统一改写为指定版本。
 *
 * 涉及三处，必须保持一致：
 *   1. package.json          -> "version"
 *   2. src-tauri/Cargo.toml  -> [package] version
 *   3. src-tauri/tauri.conf.json -> "version"
 *
 * 用法：
 *   node scripts/bump-version.mjs 0.1.1        # 指定版本
 *   node scripts/bump-version.mjs --next       # 当前版本 PATCH + 1
 */
import { readFileSync, writeFileSync } from "node:fs";

const PKG = "package.json";
const CARGO = "src-tauri/Cargo.toml";
const CONF = "src-tauri/tauri.conf.json";

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

function apply(v) {
  setPackage(v);
  setCargo(v);
  setConf(v);
  console.log(`版本号已统一为 ${v}`);
}

// 作为 CLI 运行时才执行
const isCli = process.argv[1] && process.argv[1].replace(/\\/g, "/").endsWith("scripts/bump-version.mjs");
if (isCli) {
  const arg = process.argv[2] || "--next";
  const v = arg === "--next" ? nextPatch(currentVersion()) : arg;
  if (!/^\d+\.\d+\.\d+$/.test(v)) {
    console.error(`版本号必须是 <MAJOR>.<MINOR>.<PATCH> 形式，收到: ${v}`);
    process.exit(1);
  }
  apply(v);
}
