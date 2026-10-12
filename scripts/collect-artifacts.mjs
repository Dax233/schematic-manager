#!/usr/bin/env node
/**
 * 收集 Tauri 构建产物并按统一命名规则重命名，输出清单供后续步骤使用。
 *
 * 统一命名：schematic-manager-<版本>-<平台>-<架构>[-后缀].<扩展名>
 *   例：schematic-manager-0.1.0-windows-x86_64-setup.exe
 *       schematic-manager-0.1.0-linux-arm64.AppImage
 *       schematic-manager-0.1.0-macos-arm64.dmg
 *
 * 之所以重命名：Tauri 产出的包名带中文 productName，放进下载 URL
 * 需要转义且不易阅读；统一为 ASCII 后，发布说明里的表格也能稳定解析。
 *
 * 用法：
 *   node scripts/collect-artifacts.mjs --version 0.1.0 --platform windows --arch x86_64
 * 输出：
 *   dist-artifacts/            重命名后的文件
 *   assets.json                文件名数组
 */
import { readdirSync, statSync, mkdirSync, copyFileSync, writeFileSync, existsSync } from "node:fs";
import { join, extname, basename } from "node:path";

const argv = process.argv.slice(2);
function arg(name, def) {
  const i = argv.indexOf(`--${name}`);
  return i >= 0 && argv[i + 1] ? argv[i + 1] : def;
}

const VERSION = arg("version", "");
const PLATFORM = arg("platform", "");
const ARCH = arg("arch", "");
const BUNDLE_DIR = arg("bundle", "src-tauri/target");
const OUT_DIR = arg("out", "dist-artifacts");

if (!VERSION || !PLATFORM || !ARCH) {
  console.error("必须提供 --version / --platform / --arch");
  process.exit(1);
}

/** 递归找出所有构建产物 */
function walk(dir, acc = []) {
  if (!existsSync(dir)) return acc;
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    let st;
    try { st = statSync(p); } catch { continue; }
    if (st.isDirectory()) walk(p, acc);
    else acc.push(p);
  }
  return acc;
}

// 关心的扩展名（按 Tauri 各平台实际产物）
const EXT_RE = /\.(exe|msi|zip|tar\.gz|deb|rpm|AppImage|dmg)$/i;
const all = walk(BUNDLE_DIR).filter((p) => EXT_RE.test(p));
// 只取 bundle 目录下的最终产物，排除中间文件
const finals = all.filter((p) => /[\\/]bundle[\\/]/.test(p));

mkdirSync(OUT_DIR, { recursive: true });
const written = [];

for (const src of finals) {
  const base = basename(src);
  let ext = "";
  if (/\.tar\.gz$/i.test(base)) ext = "tar.gz";
  else ext = extname(base).replace(/^\./, "");

  // 后缀用于区分同平台的多种格式（安装包 / 便携版）
  let suffix = "";
  if (/setup/i.test(base)) suffix = "-setup";
  else if (/\.msi$/i.test(base)) suffix = "-msi";

  const target = `schematic-manager-${VERSION}-${PLATFORM}-${ARCH}${suffix}.${ext}`;
  copyFileSync(src, join(OUT_DIR, target));
  written.push(target);
  console.log(`  ${base}  ->  ${target}`);
}

writeFileSync("assets.json", JSON.stringify(written, null, 2) + "\n", "utf8");
console.log(`共收集 ${written.length} 个产物`);
if (written.length === 0) {
  console.error("未找到任何构建产物，请检查 bundle 目录");
  process.exit(1);
}
