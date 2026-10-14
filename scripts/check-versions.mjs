#!/usr/bin/env node
/**
 * 校验三个清单文件中的版本号保持一致。
 *
 * 程序内置版本号取自 Cargo.toml，安装包文件名取自 tauri.conf.json，
 * 发布流程的版本基数取自 package.json——任何一个漂移，
 * 「装的版本」与「显示的版本」就会对不上。发布前必须同步，
 * 由 scripts/bump-version.mjs 统一改写。
 *
 * 用法：node scripts/check-versions.mjs
 */
import { readFileSync } from "node:fs";

const pkgVersion = JSON.parse(readFileSync("package.json", "utf8")).version;
const cargoVersion = readFileSync("src-tauri/Cargo.toml", "utf8").match(/^version = "([^"]+)"/m)?.[1];
const tauriVersion = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8")).version;

const errors = [];
if (cargoVersion !== pkgVersion) {
  errors.push(`src-tauri/Cargo.toml version ${cargoVersion} != package.json ${pkgVersion}`);
}
if (tauriVersion !== pkgVersion) {
  errors.push(`src-tauri/tauri.conf.json version ${tauriVersion} != package.json ${pkgVersion}`);
}

if (errors.length > 0) {
  console.error(errors.join("\n"));
  process.exit(1);
}
console.log(`版本号一致：${pkgVersion}`);
