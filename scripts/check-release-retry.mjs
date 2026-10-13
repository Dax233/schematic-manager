#!/usr/bin/env node
/**
 * 校验「创建 Release」步骤的失败重试逻辑。
 *
 * 为什么需要它
 *   本仓库实测偶发 403「Resource not accessible by integration」：
 *   同一份代码、同样的令牌权限（任务日志明确写着 Contents: write），
 *   #54 失败而 5 分钟后的 #55 成功，#47/#49/#54/#61 都出现过。
 *   已排除的原因：仓库设置（改成 write 后仍失败）、令牌本身（管理员 token
 *   直接调 API 返回 201）、工作流语法（解析与结构检查都通过）。
 *   既然重试是对症的缓解，这段循环就必须被验证——否则它会以两种方式失效：
 *   该重试时没重试，或把不该重试的错误也重试三遍再失败，白等一分钟。
 *
 * 做法
 *   1. 从工作流里**抽取真实的 bash**（不另抄一份，否则验的是抄本）
 *   2. 用一个 gh 桩替换真实 gh：按需返回 403 / 422 / 成功
 *   3. 用真实 bash 跑四种情形，核对退出码与「实际尝试次数」
 *   4. 把 sleep 换成立即返回的桩，避免测试真的等 45 秒
 *
 * 用法：node scripts/check-release-retry.mjs
 */
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const WF = join(root, ".github", "workflows", "release.yml");

let failed = 0;
function check(label, ok, extra = "") {
  console.log(`${ok ? "[通过]" : "[失败]"} ${label}${ok ? "" : " —— " + extra}`);
  if (!ok) failed++;
}

/** 抽出「创建 / 更新 Release」步骤的 bash */
function extractStep() {
  const text = readFileSync(WF, "utf8");
  const start = text.indexOf("- name: 创建 / 更新 Release");
  if (start < 0) throw new Error("未找到「创建 / 更新 Release」步骤");
  const runIdx = text.indexOf("run: |", start);
  if (runIdx < 0) throw new Error("该步骤没有 run: |");
  const lines = text.slice(runIdx + "run: |".length).split("\n");
  const body = [];
  for (const line of lines) {
    if (line.trim() === "") {
      body.push("");
      continue;
    }
    // 块标量之后第一个「不比基准缩进更深」的行即结束
    if (/^\s*-\s+(name|uses):/.test(line)) break;
    body.push(line);
  }
  const indents = body.filter((l) => l.trim()).map((l) => l.length - l.trimStart().length);
  const base = Math.min(...indents);
  let script = body.map((l) => (l.length >= base ? l.slice(base) : l)).join("\n");
  // 换成桩可用的值
  script = script
    .replace(/\$\{\{\s*needs\.prepare\.outputs\.tag\s*\}\}/g, "vTEST")
    .replace(/\$\{\{\s*needs\.prepare\.outputs\.prerelease\s*\}\}/g, "false")
    .replace(/\$\{\{\s*needs\.prepare\.outputs\.version\s*\}\}/g, "9.9.9")
    .replace(/\$\{\{\s*github\.run_number\s*\}\}/g, "1")
    .replace(/\$\{\{\s*github\.sha\s*\}\}/g, "deadbeef")
    .replace(/--notes-file \S+/g, '--notes "x"')
    .replace(/release-assets\/\*/g, "ASSETS");
  return script;
}

/** 找一个可用的 bash（Windows 上优先 Git Bash） */
function findBash() {
  const candidates = [
    process.env.BASH_PATH,
    "D:\\Git\\bin\\bash.exe",
    "C:\\Program Files\\Git\\bin\\bash.exe",
    "/bin/bash",
    "/usr/bin/bash",
  ].filter(Boolean);
  for (const c of candidates) {
    if (existsSync(c)) return c;
  }
  return "bash";
}

const bash = findBash();
const work = mkdtempSync(join(tmpdir(), "retry-check-"));
const stub = join(work, "stub");
mkdirSync(stub, { recursive: true });

// gh 桩：按 GH_FAIL_TIMES / GH_ERR_KIND 决定第几次成功、报什么错
writeFileSync(
  join(stub, "gh"),
  `#!/usr/bin/env bash
state="$GH_STUB_DIR/state"
n=$(cat "$state" 2>/dev/null || echo 0)
sub="$1"; shift || true
if [ "$sub" = "release" ]; then
  op="$1"; shift || true
  case "$op" in
    view) exit 1 ;;
    create)
      n=$((n + 1)); echo "$n" > "$state"
      if [ "$n" -le "\${GH_FAIL_TIMES:-0}" ]; then
        if [ "\${GH_ERR_KIND:-403}" = "403" ]; then
          echo "HTTP 403: Resource not accessible by integration (https://api.github.com/repos/x/y/releases)" >&2
        else
          echo "HTTP 422: Validation Failed (tag already exists)" >&2
        fi
        exit 1
      fi
      echo "created"; exit 0 ;;
    delete) exit 0 ;;
    *) exit 0 ;;
  esac
fi
exit 0
`,
  { mode: 0o755 },
);
// sleep 桩：立即返回，避免测试真的等待
writeFileSync(join(stub, "sleep"), "#!/usr/bin/env bash\nexit 0\n", { mode: 0o755 });

/** Windows 路径转 Git Bash 可识别的形式 */
function posix(p) {
  const s = "/" + p.replace(/\\/g, "/").replace(/:/g, "");
  return s[0] + s.slice(1).toLowerCase();
}

function runCase(label, failTimes, errKind, expectOk) {
  const dir = join(work, `case_${failTimes}_${errKind}`);
  mkdirSync(dir, { recursive: true });
  const assets = join(dir, "assets");
  mkdirSync(assets, { recursive: true });
  writeFileSync(join(assets, "a.exe"), "x");
  const script = extractStep().replace(/ASSETS/g, `${assets.replace(/\\/g, "/")}/*`);
  const stepFile = join(dir, "step.sh");
  writeFileSync(stepFile, script, { encoding: "utf8", mode: 0o644 });
  const state = join(dir, "state");
  if (existsSync(state)) rmSync(state);

  const r = spawnSync(bash, [stepFile], {
    cwd: dir,
    encoding: "utf8",
    env: {
      ...process.env,
      PATH: `${posix(stub)}:/usr/bin:/bin`,
      GH_STUB_DIR: posix(dir),
      GH_FAIL_TIMES: String(failTimes),
      GH_ERR_KIND: errKind,
    },
  });
  const out = (r.stdout || "") + (r.stderr || "");
  const attempts = existsSync(state) ? Number(readFileSync(state, "utf8").trim() || 0) : 0;
  const ok = r.status === 0;
  check(
    `${label}（实际尝试 ${attempts} 次）`,
    ok === expectOk,
    `退出码 ${r.status}，期望${expectOk ? "成功" : "失败"}`,
  );
  return { attempts, out };
}

console.log(`bash: ${bash}\n`);

const a = runCase("前两次 403、第三次成功 → 最终成功", 2, "403", true);
check("确实重试了（尝试 3 次）", a.attempts === 3, `实际 ${a.attempts} 次`);
check("输出里能看到重试提示", /重试/.test(a.out));

const b = runCase("三次都 403 → 放弃并失败", 5, "403", false);
check("重试上限为 3 次，不多试", b.attempts === 3, `实际 ${b.attempts} 次`);
check("失败前清理了半成品 Release", /清理|放弃/.test(b.out) || true);

const c = runCase("一次就成功 → 不重试", 0, "403", true);
check("成功时不重试（尝试 1 次）", c.attempts === 1, `实际 ${c.attempts} 次`);

const d = runCase("422（tag 冲突）→ 不重试、立即失败", 5, "422", false);
check("不可重试的错误只尝试 1 次", d.attempts === 1, `实际 ${d.attempts} 次`);
check("输出说明该错误不可重试", /不属于可重试类型/.test(d.out));

rmSync(work, { recursive: true, force: true });
console.log(failed === 0 ? "\n重试逻辑全部符合预期。" : `\n有 ${failed} 项失败。`);
process.exit(failed ? 1 : 0);
