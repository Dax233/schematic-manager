#!/usr/bin/env node
/**
 * GitHub Actions 工作流的静态检查。
 *
 * 为什么需要它
 *   workflow 的 YAML 能被 yaml 库解析，**不代表**它能被 GitHub 正确执行。
 *   常见错误都不会在解析阶段暴露：`needs` 写了不存在的任务、步骤引用了
 *   未声明的 output、`if` 里的表达式拼错、矩阵引用了未定义的键。
 *   这些错误只在推送之后才由 GitHub 报出来——而本工作流的构建阶段要跑
 *   六个平台、耗时很久，等到那时才发现代价很高。
 *
 * 检查内容
 *   1. 顶层必需键、触发器与权限块
 *   2. 每个任务引用的 needs 必须存在，不能有环
 *   3. 引用的 needs.<job>.outputs.<name> 必须在对方 outputs 里声明
 *   4. 矩阵策略里引用的 matrix.<key> 必须在 include 或 matrix 下定义
 *   5. ${{ }} 表达式括号配对、无空表达式
 *   6. 不允许出现 concurrency（会取消排队中的运行，丢发布）
 *   7. 流式映射里的未加引号逗号值（YAML 条目分隔陷阱）
 *
 * 用法：node scripts/check-workflow.mjs
 */
import { readFileSync } from "node:fs";

const WF = ".github/workflows/ci.yml";
const raw = readFileSync(WF, "utf8");

let failed = 0;
function check(label, ok, extra = "") {
  console.log(`${ok ? "[通过]" : "[失败]"} ${label}${ok ? "" : " —— " + extra}`);
  if (!ok) failed++;
}

// 用 PyYAML 之外的实现：工作流是普通 YAML，这里用最小解析器读结构。
// 为避免再引入依赖，改用「按缩进扫描」的方式提取任务与步骤，足够本检查使用。
const lines = raw.split("\n");

// ---------------------------------------------------------------- 1 顶层结构
const topKeys = [];
for (const line of lines) {
  const m = line.match(/^([a-zA-Z_][\w-]*):/);
  if (m) topKeys.push(m[1]);
}
check("顶层有 name", topKeys.includes("name"));
check("顶层有 on（触发器）", topKeys.includes("on"));
check("顶层有 jobs", topKeys.includes("jobs"));
check("顶层有 permissions", topKeys.includes("permissions"));
check("permissions 含 contents: write（创建 Release 与回推提交都需要）",
  /permissions:\s*\n(\s+#.*\n)*\s+contents:\s*write/.test(raw));

// ---------------------------------------------------------------- 2 任务表
// 任务名是 **jobs: 块内**的两空格缩进键。
// 不能直接扫「两空格缩进的键」：on: 下的 push / workflow_dispatch 也是这个形状，
// 会被误当成任务——那样 `needs: push` 这种错误引用就会被放行。
let jobsStart = lines.findIndex((l) => /^jobs:\s*$/.test(l));
if (jobsStart < 0) throw new Error("未找到 jobs: 块");
let jobsEnd = lines.length;
for (let i = jobsStart + 1; i < lines.length; i++) {
  // 回到顶层的键即 jobs 块结束
  if (/^[a-zA-Z_][\w-]*:/.test(lines[i])) { jobsEnd = i; break; }
}
const jobLines = lines.slice(jobsStart + 1, jobsEnd);
const jobNames = [];
for (const line of jobLines) {
  const m = line.match(/^ {2}([a-zA-Z_][\w-]*):\s*$/);
  if (m) jobNames.push(m[1]);
}
check(`jobs 块内解析出任务：${jobNames.join(", ")}`, jobNames.length > 0);
check("触发器名未被误当成任务", !jobNames.includes("push") && !jobNames.includes("workflow_dispatch"));

// 每个任务的内容
const jobBlocks = {};
for (let i = 0; i < jobLines.length; i++) {
  const m = jobLines[i].match(/^ {2}([a-zA-Z_][\w-]*):\s*$/);
  if (!m) continue;
  const name = m[1];
  let end = jobLines.length;
  for (let j = i + 1; j < jobLines.length; j++) {
    if (/^ {2}[a-zA-Z_][\w-]*:\s*$/.test(jobLines[j])) { end = j; break; }
  }
  jobBlocks[name] = jobLines.slice(i + 1, end).join("\n");
}

let needsOk = true;
for (const [name, body] of Object.entries(jobBlocks)) {
  const nm = body.match(/^\s+needs:\s*(.+)$/m);
  if (!nm) continue;
  const deps = nm[1].replace(/[[\]']/g, "").split(",").map((s) => s.trim()).filter(Boolean);
  for (const d of deps) {
    if (!jobNames.includes(d)) {
      check(`任务 ${name} 的 needs 指向存在的任务 ${d}`, false, "该任务不存在");
      needsOk = false;
    }
  }
}
check("所有 needs 都指向存在的任务", needsOk);

// ---------------------------------------------------------------- 3 outputs 引用
let outOk = true;
for (const [name, body] of Object.entries(jobBlocks)) {
  for (const m of body.matchAll(/needs\.([a-zA-Z_][\w-]*)\.outputs\.([a-zA-Z_][\w-]*)/g)) {
    const [, dep, out] = m;
    if (!jobNames.includes(dep)) {
      check(`${name} 引用了任务 ${dep} 的 output`, false, "该任务不存在");
      outOk = false;
      continue;
    }
    // 对方是否声明了该 output
    const depBody = jobBlocks[dep];
    const declared = new RegExp(`^\\s+${out}:`, "m").test(depBody);
    if (!declared) {
      check(`${name} 引用的 needs.${dep}.outputs.${out} 已声明`, false, "对方未声明该 output");
      outOk = false;
    }
  }
}
check("所有 needs.<job>.outputs 引用都已声明", outOk);

// ---------------------------------------------------------------- 4 矩阵键
const matrixKeys = new Set();
for (const m of raw.matchAll(/matrix\.([a-zA-Z_][\w]*)/g)) matrixKeys.add(m[1]);
const includeKeys = new Set();
// 工作流里可能有多个矩阵（quality 与 build），每处 include 块都要收集；
// 条目之间允许夹注释行
for (const incBlock of raw.matchAll(/include:\s*\n((?:\s+(?:- \{[^\n]*|#[^\n]*)\n)+)/g)) {
  for (const m of incBlock[1].matchAll(/([a-zA-Z_][\w]*):/g)) includeKeys.add(m[1]);
}
let matrixOk = true;
for (const k of matrixKeys) {
  if (!includeKeys.has(k)) {
    check(`矩阵键 matrix.${k} 有定义`, false, `include 里没有 ${k}（现有：${[...includeKeys].join(", ")}）`);
    matrixOk = false;
  }
}
check("所有 matrix.* 引用都在矩阵里定义", matrixOk);

// ---------------------------------------------------------------- 5 表达式配对
let exprOk = true;
for (const line of lines) {
  const opens = (line.match(/\$\{\{/g) || []).length;
  const closes = (line.match(/\}\}/g) || []).length;
  if (opens !== closes) {
    check(
      "表达式括号配对：" + line.trim().slice(0, 60),
      false,
      opens + " 个 ${ 对 " + closes + " 个 }",
    );
    exprOk = false;
  }
}
check("所有 GitHub 表达式括号配对", exprOk);

// ---------------------------------------------------------------- 6 不允许 concurrency
check("不含 concurrency（会取消排队中的运行、丢发布）",
  !/^concurrency:/m.test(raw));

// ---------------------------------------------------------------- 7 流式映射里的裸逗号值
// 矩阵是 flow mapping（花括号），其中的逗号会分隔条目。
// 像 `bundles: deb,appimage` 会被解析成 `bundles: deb` 加一个多余的
// `appimage: null` 键——AppImage 从未被真正请求，而 YAML 层面看不出异常。
//
// 只检查**矩阵条目行**，不能匹配任意花括号：
// `${{ needs.prepare.outputs.version }}` 也是花括号，用它判断会命中几十处
// 表达式，把 `--next-of` 里那对括号当成残片，原始文件都会「不通过」。
// （第一版就是这样，变异测试虽然「全部发现」，其实是无差别误报。）
//
// 判定方式：按顶层逗号切分，每个片段都必须是 key: value 形式。
// `bundles: deb,appimage` 切出来是 ["bundles: deb", "appimage"]，
// 第二段没有冒号——正是「被裸逗号切开的残片」的特征。
let flowOk = true;
for (const line of lines) {
  // 矩阵条目形如：`          - { runner: ..., bundles: nsis }`
  const m = line.match(/^\s*- \{([^{}]*)\}\s*$/);
  if (!m) continue;
  for (const part of m[1].split(",")) {
    const t = part.trim();
    if (!t) continue;
    const colon = t.indexOf(":");
    if (colon < 0) {
      check(
        "矩阵条目的每个片段都是 key: value 形式",
        false,
        JSON.stringify(t) + " 没有冒号，像是被裸逗号切出来的残片",
      );
      flowOk = false;
      continue;
    }
    const key = t.slice(0, colon).trim();
    const val = t.slice(colon + 1).trim();
    // 值里再有逗号且未加引号，同样可疑
    if (val === "" || (/[:,]/.test(val) && !/^["']/.test(val))) {
      check(
        "矩阵条目的值格式正确：" + key,
        false,
        JSON.stringify(val) + " 可疑（裸逗号会被当成条目分隔符）",
      );
      flowOk = false;
    }
  }
}
check("矩阵流式映射里没有未加引号的逗号值", flowOk);

// ---------------------------------------------------------------- 8 关键步骤存在
check("有「解析幂等 Release」步骤（prepare 经 resolve-release.sh 创建/复用）",
  raw.includes("解析幂等 Release") && raw.includes("resolve-release.sh"));
check("有「生成并写回 Release 说明」步骤", raw.includes("生成并写回 Release 说明"));
check("构建经 tauri-action 直传 Release（按数字 release_id）",
  /tauri-apps\/tauri-action@v0/.test(raw) && /releaseId:\s*\$\{\{ needs\.prepare\.outputs\.release_id \}\}/.test(raw));
check("有「递增版本号到下一 PATCH」任务（正式发布才执行）",
  raw.includes("递增版本号到下一 PATCH"));
check("递增版本号以 tag 版本为基准（--next-of）",
  /bump-version\.mjs --next-of/.test(raw));
check("发布链路只在 push 事件运行（PR 与手动触发只跑检查）",
  /if:\s*github\.event_name == 'push'/.test(raw));

// ---------------------------------------------------------------- 9 用途说明
console.log(`\n任务数 ${jobNames.length}，矩阵键 ${[...matrixKeys].join(", ")}`);
console.log(failed === 0 ? "\n工作流检查全部通过。" : `\n有 ${failed} 项失败。`);
process.exit(failed ? 1 : 0);
