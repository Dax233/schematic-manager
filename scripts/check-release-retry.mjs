#!/usr/bin/env node
/**
 * 校验 scripts/resolve-release.sh 里「创建 Release」的失败重试逻辑。
 *
 * 为什么需要它
 *   本仓库实测偶发 403「Resource not accessible by integration」：
 *   同一份代码、同样的令牌权限，相邻两次运行一成一败（#47/#49/#54/#61）。
 *   已排除权限声明与仓库设置问题，重试是对症的缓解。这段循环必须被验证——
 *   否则它会以两种方式失效：该重试时没重试，或把不该重试的错误也重试三遍
 *   再失败，白等一分钟。
 *
 * 做法
 *   1. 直接执行真实的 resolve-release.sh（不抄副本，否则验的是抄本）
 *   2. 用 gh / git 桩替换真实命令：gh 按需返回 403 / 422 / 成功 / 已存在，
 *      git 返回固定的空 tag 列表与两条提交
 *   3. 核对退出码与「实际创建尝试次数」
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
const SCRIPT = join(root, "scripts", "resolve-release.sh");

let failed = 0;
function check(label, ok, extra = "") {
  console.log(`${ok ? "[通过]" : "[失败]"} ${label}${ok ? "" : " —— " + extra}`);
  if (!ok) failed++;
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

// gh 桩：只认 api 子命令。
//   .../releases?per_page=100  查询既有 Release：GH_EXISTING=1 时返回 id 456，否则空
//   .../releases               创建 Release：按 GH_FAIL_TIMES / GH_ERR_KIND 决定第几次成功、报什么错
// 桩直接输出 --jq 之后的最终结果（--jq 由真实 gh 内部处理，桩看不到也不需要它）。
writeFileSync(
  join(stub, "gh"),
  `#!/usr/bin/env bash
state="$GH_STUB_DIR/state"
n=$(cat "$state" 2>/dev/null || echo 0)
if [ "$1" = "api" ]; then
  case "$2" in
    *"releases?per_page=100"*)
      if [ "\${GH_EXISTING:-0}" = "1" ]; then echo 456; fi
      exit 0 ;;
    */releases)
      n=$((n + 1)); echo "$n" > "$state"
      if [ "$n" -le "\${GH_FAIL_TIMES:-0}" ]; then
        if [ "\${GH_ERR_KIND:-403}" = "403" ]; then
          echo "HTTP 403: Resource not accessible by integration (https://api.github.com/repos/x/y/releases)" >&2
        else
          echo "HTTP 422: Validation Failed (tag already exists)" >&2
        fi
        exit 1
      fi
      echo 123; exit 0 ;;
  esac
fi
exit 0
`,
  { mode: 0o755 },
);

// git 桩：collect-commits.sh 会用到的最小面——非浅检出、fetch 成功、
// 没有任何历史 tag、固定两条提交
writeFileSync(
  join(stub, "git"),
  `#!/usr/bin/env bash
case "$1" in
  rev-parse) echo false ;;
  fetch) exit 0 ;;
  tag) exit 0 ;;
  log) printf 'feat: 示例提交\\nfix: 示例修复\\n' ;;
esac
exit 0
`,
  { mode: 0o755 },
);

// sleep 桩：立即返回，避免测试真的等待
writeFileSync(join(stub, "sleep"), "#!/usr/bin/env bash\nexit 0\n", { mode: 0o755 });

/**
 * 把工作目录转成 bash 能用的路径。
 *
 * Windows 上 node 给出的是 `C:\a\b`，得转成 Git Bash 认的 `/c/a/b`；
 * Linux / macOS 上本来就是 POSIX 路径，**不能**再转换。
 * 早前版本无条件按 Windows 规则转，在 Linux 上把 `/home/runner/...` 改写坏，
 * 桩目录找不到、gh 根本没被执行——CI 上断言大片失败而本地完全看不出来。
 */
function posix(p) {
  if (process.platform !== "win32") return p;
  const s = "/" + p.replace(/\\/g, "/").replace(/:/g, "");
  return s[0] + s.slice(1).toLowerCase();
}

function runCase(label, { failTimes = 0, errKind = "403", existing = false, expectOk }) {
  const dir = join(work, `case_${failTimes}_${errKind}_${existing ? "exist" : "new"}`);
  mkdirSync(dir, { recursive: true });
  const state = join(dir, "state");
  if (existsSync(state)) rmSync(state);
  const ghOutput = join(dir, "github_output");

  const r = spawnSync(bash, [posix(SCRIPT), "branch", "main", "7", "with-commits"], {
    // resolve-release.sh 内部用相对路径调 scripts/，cwd 必须是仓库根
    cwd: root,
    encoding: "utf8",
    env: {
      ...process.env,
      PATH: `${posix(stub)}:${process.env.PATH}`,
      GH_STUB_DIR: posix(dir),
      GH_FAIL_TIMES: String(failTimes),
      GH_ERR_KIND: errKind,
      GH_EXISTING: existing ? "1" : "0",
      GITHUB_REPOSITORY: "x/y",
      GITHUB_SHA: "deadbeef",
      GH_TOKEN: "fake-token",
      GITHUB_OUTPUT: posix(ghOutput),
    },
  });
  const out = (r.stdout || "") + (r.stderr || "");
  const attempts = existsSync(state) ? Number(readFileSync(state, "utf8").trim() || 0) : 0;
  const ok = r.status === 0;
  check(
    `${label}（实际创建尝试 ${attempts} 次）`,
    ok === expectOk,
    `退出码 ${r.status}，期望${expectOk ? "成功" : "失败"}，输出尾部：${out.trim().split("\n").slice(-3).join(" / ")}`,
  );
  const outputs = existsSync(ghOutput) ? readFileSync(ghOutput, "utf8") : "";
  return { attempts, out, outputs };
}

console.log(`bash: ${bash}\n`);

const a = runCase("前两次 403、第三次成功 → 最终成功", { failTimes: 2, expectOk: true });
check("确实重试了（尝试 3 次）", a.attempts === 3, `实际 ${a.attempts} 次`);
check("输出里能看到重试提示", /重试/.test(a.out));
check("成功后写出 release_id 输出", /release_id=123/.test(a.outputs), a.outputs.trim());
check("dev 构建 tag 形如 v<版本>-dev.<运行号>", /tag=v\d+\.\d+\.\d+-dev\.7/.test(a.outputs), a.outputs.trim());

const b = runCase("三次都 403 → 放弃并失败", { failTimes: 5, expectOk: false });
check("重试上限为 3 次，不多试", b.attempts === 3, `实际 ${b.attempts} 次`);
check("失败时输出放弃说明", /放弃/.test(b.out));

const c = runCase("一次就成功 → 不重试", { failTimes: 0, expectOk: true });
check("成功时不重试（尝试 1 次）", c.attempts === 1, `实际 ${c.attempts} 次`);

const d = runCase("422（tag 冲突）→ 不重试、立即失败", { failTimes: 5, errKind: "422", expectOk: false });
check("不可重试的错误只尝试 1 次", d.attempts === 1, `实际 ${d.attempts} 次`);
check("输出说明该错误不可重试", /不属于可重试类型/.test(d.out));

const e = runCase("Release 已存在 → 复用、不重复创建", { existing: true, expectOk: true });
check("已存在时不发起创建（尝试 0 次）", e.attempts === 0, `实际 ${e.attempts} 次`);
check("输出说明复用了既有 Release", /复用/.test(e.out));
check("复用时写出既有 release_id", /release_id=456/.test(e.outputs), e.outputs.trim());

rmSync(work, { recursive: true, force: true });
console.log(failed === 0 ? "\n重试逻辑全部符合预期。" : `\n有 ${failed} 项失败。`);
process.exit(failed ? 1 : 0);
