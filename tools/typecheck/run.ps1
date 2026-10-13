#!/usr/bin/env pwsh
# 离线类型检查。
#
# 背景：本机无法访问 crates.io（schannel 被阻断），cargo 拉不到依赖，
# 因此无法用 cargo check。但类型检查本身不需要真实依赖——
# 这里用最小桩替代 flate2 与 thiserror，然后用 rustc 直接检查纯逻辑模块。
#
# 覆盖：error / nbt / mcver / litematic / blockdb / block_model /
#       geometry / visibility / startup / backup，含全部单元测试。
# 不覆盖：commands.rs / dto.rs / lib.rs / main.rs（依赖 tauri 与 serde）。
#
# 用途：在没有网络、没有 cargo 依赖的情况下，快速发现真实编译错误。
# 注意：这不是完整构建，也不能替代 CI —— 它只做类型检查，不链接、不运行。
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$cargo = "$env:USERPROFILE\.cargo\bin"
if (Test-Path $cargo) { $env:Path = "$cargo;$env:Path" }
# 本机拒绝对系统临时目录的写入，而部分测试需要临时目录。
# 指到工作区内可绕开该环境限制，且不影响被测代码的行为。
$localTemp = Join-Path (Split-Path (Split-Path $here -Parent) -Parent) '_temp'
New-Item -ItemType Directory -Force -Path $localTemp | Out-Null
$env:TEMP = $localTemp
$env:TMP = $localTemp
$env:CARGO_MANIFEST_DIR = (Resolve-Path "$here\..\..\src-tauri").Path

$stub = Join-Path $here 'libflate2.rlib'
# 新增模块却忘记登记到 check.rs，会导致该模块的测试静默不执行——
# 表面上「测试通过」，实际一次都没跑。这里做一次交叉核对。
$srcDir = Join-Path $here '..\..\src-tauri\src'
$checkText = Get-Content (Join-Path $here 'check.rs') -Raw
# 这些模块依赖 tauri / serde，无法用桩替代，明确排除
$excluded = @('lib.rs', 'main.rs', 'commands.rs', 'dto.rs', 'error.rs')
$missing = @()
Get-ChildItem $srcDir -Filter '*.rs' -File | ForEach-Object {
    if ($excluded -contains $_.Name) { return }
    $mod = [System.IO.Path]::GetFileNameWithoutExtension($_.Name)
    if ($checkText -notmatch "mod $mod;") { $missing += $_.Name }
}
if ($missing.Count -gt 0) {
    Write-Host "以下模块未登记到 check.rs，其测试不会被执行："
    $missing | ForEach-Object { Write-Host "  $_" }
    Write-Host "请加入 check.rs 的 mod 列表；确实无法检查时加入本文件的 excluded 列表。"
    exit 1
}

Write-Host "1/5 核对模块登记并编译 flate2 桩…"
Remove-Item $stub -Force -ErrorAction SilentlyContinue
# 必须产出真正的 rlib（--emit=link），否则 --extern 会报 archive 无法解析。
# 用 cmd 重定向，避免 PowerShell 把 rustc 的 stderr 警告当成致命错误。
& cmd /c "rustc --edition 2021 --crate-type lib --crate-name flate2 --emit=link -o `"$stub`" `"$(Join-Path $here 'flate2.rs')`" 2>nul"
if (-not (Test-Path $stub)) { Write-Host "  flate2 桩编译失败"; exit 1 }

Write-Host "2/5 类型检查逻辑模块…"
$serdeJson = Join-Path $here 'libserde_json.rlib'
$out = & rustc --edition 2021 --crate-type lib --crate-name check --test --emit=metadata `
    --extern "flate2=$stub" --extern "serde_json=$serdeJson" `
    (Join-Path $here 'check.rs') -o (Join-Path $here 'check.rmeta') 2>&1
$out | Where-Object { $_ -notmatch '^\s*$' }
$errs = @($out | Select-String -Pattern '^error').Count
Write-Host ""
Write-Host "类型检查 -> 错误: $errs   警告: $(@($out | Select-String -Pattern '^warning').Count)"
if ($errs -gt 0) { exit $errs }

# 类型无误后，进一步链接出测试可执行文件并真正运行。
# 说明：flate2 桩不实现真正的压缩，因此依赖 gzip/zlib 的测试会失败——
# 这些失败属于桩的局限，不代表代码有问题，下面会单独列出。
Write-Host ""
# 命令层与 DTO 单独检查一次：它们依赖 tauri 与 serde 的过程宏，
# 无法并入上面的检查，但手工核对极易漏错（实测漏过接口名与导入）。
Write-Host ""
Write-Host "3/5 检查命令层与 DTO…"
$cmdsErr = 0
$py = "C:\Users\hsds\.dsh\dsh-runtimes\dsh-primary-runtime\dependencies\python\python.exe"
if (-not (Test-Path $py)) { $py = "python" }
& $py (Join-Path $here 'gen_cmd_check.py') | Out-Null

foreach ($pair in @(
    @{ name = 'tauri'; src = 'tauri_stub_crate.rs'; out = 'libtauri.rlib' },
    @{ name = 'serde'; src = 'serde_stub_crate.rs'; out = 'libserde.rlib' },
    @{ name = 'serde_json'; src = 'serde_json_stub_crate.rs'; out = 'libserde_json.rlib' }
)) {
    $rlib = Join-Path $here $pair.out
    if (-not (Test-Path $rlib)) {
        & cmd /c "rustc --edition 2021 --crate-type lib --crate-name $($pair.name) --emit=link -o `"$rlib`" `"$(Join-Path $here $pair.src)`" 2>nul"
    }
}
$cmdsOut = & rustc --edition 2021 --crate-type bin --crate-name cmds `
    --extern "flate2=$stub" `
    --extern "tauri=$(Join-Path $here 'libtauri.rlib')" `
    --extern "serde=$(Join-Path $here 'libserde.rlib')" `
    --extern "serde_json=$serdeJson" `
    (Join-Path $here 'check_cmds.rs') --emit=metadata -o (Join-Path $here 'cmds.rmeta') 2>&1
$cmdsErr = @($cmdsOut | Select-String -Pattern '^error').Count
if ($cmdsErr -gt 0) {
    $cmdsOut | Select-Object -First 40
}
Write-Host "命令层 -> 错误: $cmdsErr"
if ($cmdsErr -gt 0) { exit $cmdsErr }

# 命令层与 DTO 的单元测试。
#
# 上一步不带 --test，因此 cfg(test) 里的代码**完全没有被检查**——而命令层的
# 测试正是覆盖「导入的包有没有生效」这类关键行为的地方。实测这一步能抓到
# 重复定义的测试模块名、DTO 新增字段后测试未同步这类错误，
# 它们在纯类型检查下完全不可见。
Write-Host "4/5 运行命令层与 DTO 的单元测试…"
$cmdsExe = Join-Path $here 'cmdscheck.exe'
Remove-Item $cmdsExe -Force -ErrorAction SilentlyContinue
$cmdsBuild = & cmd /c "rustc --edition 2021 --crate-type lib --crate-name cmdscheck --test --extern `"flate2=$stub`" --extern `"tauri=$(Join-Path $here 'libtauri.rlib')`" --extern `"serde=$(Join-Path $here 'libserde.rlib')`" --extern `"serde_json=$serdeJson`" `"$((Join-Path $here 'check_cmds.rs'))`" -o `"$cmdsExe`" 2>&1"
$cmdsTestErr = @($cmdsBuild | Select-String -Pattern '^error').Count
if ($cmdsTestErr -gt 0 -or -not (Test-Path $cmdsExe)) {
    $cmdsBuild | Select-Object -First 40
    Write-Host "  命令层测试编译失败"
    if ($cmdsTestErr -eq 0) { $cmdsTestErr = 1 }
    exit $cmdsTestErr
}
$cmdsTestOut = & $cmdsExe --test-threads=4 2>&1
$cmdsTestExit = $LASTEXITCODE
# 受桩限制：DTO 的字段名契约靠 serde 的 `rename_all` 生成，而本地 serde 桩
# 没有过程宏，`to_value` 只能返回空对象。这些测试因此在本地必然失败，
# 但**必须保留并运行**：CI 用真实 serde，它们在那里才真正生效；
# 本地能编译它们，就足以发现字段改名后忘记同步断言这类错误。
$cmdsStubBound = 'dto::tests::'
$cmdsFailed = $cmdsTestOut | Select-String -Pattern '^    (\S+::tests::\S+)' | ForEach-Object { $_.Matches[0].Groups[1].Value }
$cmdsReal = $cmdsFailed | Where-Object { $_ -notmatch $cmdsStubBound }
if ($cmdsReal.Count -gt 0) {
    Write-Host "  命令层测试失败（与代码相关）："
    $cmdsReal | ForEach-Object { Write-Host "    $_" }
    exit $cmdsReal.Count
}
# 与第 5 步同样的道理：失败项靠正则从输出里捞，若测试进程崩溃或提前 abort，
# 输出中没有可匹配的行，$cmdsReal 为空，脚本会继续往下走并把整件事说成通过。
# 因此这里也以进程退出码兜底。
if ($cmdsTestExit -ne 0 -and $cmdsFailed.Count -eq 0) {
    Write-Host "  命令层测试进程退出码为 $cmdsTestExit，但输出中未找到失败项（可能是崩溃或超时）"
    $cmdsTestOut | Select-Object -Last 20 | ForEach-Object { Write-Host "    $_" }
    exit $cmdsTestExit
}
# 只报告与本步相关的结论，不打印原始失败清单：
# 那 6 项在本地注定失败，原样铺开会让人以为出了问题
$cmdsTotal = $cmdsTestOut | Select-String -Pattern 'test result: (ok|FAILED)\. (\d+) passed'
if ($cmdsTotal) {
    $n = $cmdsTotal.Matches[0].Groups[2].Value
    Write-Host "命令层与 DTO -> $n 项通过，$($cmdsFailed.Count) 项受本地 serde 桩限制（CI 覆盖）"
}

Write-Host "5/5 运行单元测试…"
$exe = Join-Path $here 'check.exe'
Remove-Item $exe -Force -ErrorAction SilentlyContinue
& cmd /c "rustc --edition 2021 --crate-type lib --crate-name check --test --extern `"flate2=$stub`" --extern `"serde_json=$serdeJson`" `"$((Join-Path $here 'check.rs'))`" -o `"$exe`" 2>nul"
if (-not (Test-Path $exe)) { Write-Host "  测试可执行文件编译失败"; exit 1 }
$testOut = & $exe --test-threads=4 2>&1
$testExit = $LASTEXITCODE
$testOut | Where-Object { $_ -match 'test result|^failures:|^    ' } | Select-Object -First 40

# 退出码以测试可执行文件自身的返回值为准。
# 此前只看输出里能否匹配到失败项列表：若二进制崩溃或提前 abort，
# 输出里没有匹配行，$real 为空，脚本会报告「测试全部通过」并 exit 0
# ——把一次彻底的失败说成成功。
if ($testExit -eq 0) {
    Write-Host ""
    Write-Host "测试全部通过。"
} else {
    # 已知受桩限制的测试：依赖真实的 gzip/zlib 解压
    # gzip/zlib 曾在此清单里，自本地 flate2 桩改为真实解压后已不受限，
    # 真实 .litematic 样本也能完整往返。
    $stubBound = 'metadata_edit_roundtrips_through_disk'
    $names = $testOut | Where-Object { $_ -match '^    [a-z_]+::tests::' } | ForEach-Object { $_.Trim() }
    $real = $names | Where-Object { $_ -notmatch $stubBound }
    Write-Host ""
    Write-Host "失败项中与代码相关的:"
    if ($real.Count -eq 0) {
        Write-Host "  未能从输出中定位失败项（测试进程退出码 $testExit；可能是崩溃或超时）"
        exit $testExit
    }
    $real | ForEach-Object { Write-Host "  $_" }
    exit $real.Count
}

# 演示程序单独编译一次。
#
# 它用真实数据走完整条链路，是人工查看进展的主要入口，但此前没有任何
# 脚本编译它——改坏了不会有人发现，而它引用的模块一旦更名（例如删掉
# camera）就会静默失效。这里做一次类型检查，成本只有几百毫秒。
Write-Host ""
Write-Host "附加：类型检查进展演示程序…"
$demoOut = & rustc --edition 2021 --crate-type bin --crate-name demo `
    --extern "flate2=$stub" --extern "serde_json=$serdeJson" `
    (Join-Path $here 'demo.rs') --emit=metadata -o (Join-Path $here 'demo.rmeta') 2>&1
$demoErr = @($demoOut | Select-String -Pattern '^error').Count
if ($demoErr -gt 0) {
    $demoOut | Select-Object -First 30
    Write-Host "  进展演示程序类型检查失败"
    exit $demoErr
}
Write-Host "  进展演示程序 -> 错误: 0"
exit 0
