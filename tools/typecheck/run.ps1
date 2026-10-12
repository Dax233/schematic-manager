#!/usr/bin/env pwsh
# 离线类型检查。
#
# 背景：本机无法访问 crates.io（schannel 被阻断），cargo 拉不到依赖，
# 因此无法用 cargo check。但类型检查本身不需要真实依赖——
# 这里用最小桩替代 flate2 与 thiserror，然后用 rustc 直接检查纯逻辑模块。
#
# 覆盖：error / nbt / mcver / litematic / blockdb / block_model /
#       geometry / camera / visibility / backup，含全部单元测试。
# 不覆盖：commands.rs / dto.rs / lib.rs / main.rs（依赖 tauri 与 serde）。
#
# 用途：在没有网络、没有 cargo 依赖的情况下，快速发现真实编译错误。
# 注意：这不是完整构建，也不能替代 CI —— 它只做类型检查，不链接、不运行。
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$cargo = "$env:USERPROFILE\.cargo\bin"
if (Test-Path $cargo) { $env:Path = "$cargo;$env:Path" }
$env:CARGO_MANIFEST_DIR = (Resolve-Path "$here\..\..\src-tauri").Path

$stub = Join-Path $here 'libflate2.rlib'
Write-Host "1/2 编译 flate2 桩…"
Remove-Item $stub -Force -ErrorAction SilentlyContinue
# 必须产出真正的 rlib（--emit=link），否则 --extern 会报 archive 无法解析。
# 用 cmd 重定向，避免 PowerShell 把 rustc 的 stderr 警告当成致命错误。
& cmd /c "rustc --edition 2021 --crate-type lib --crate-name flate2 --emit=link -o `"$stub`" `"$(Join-Path $here 'flate2.rs')`" 2>nul"
if (-not (Test-Path $stub)) { Write-Host "  flate2 桩编译失败"; exit 1 }

Write-Host "2/2 类型检查逻辑模块…"
$out = & rustc --edition 2021 --crate-type lib --crate-name check --test --emit=metadata `
    --extern "flate2=$stub" (Join-Path $here 'check.rs') -o (Join-Path $here 'check.rmeta') 2>&1
$out | Where-Object { $_ -notmatch '^\s*$' }
$errs = @($out | Select-String -Pattern '^error').Count
Write-Host ""
Write-Host "类型检查 -> 错误: $errs   警告: $(@($out | Select-String -Pattern '^warning').Count)"
if ($errs -gt 0) { exit $errs }

# 类型无误后，进一步链接出测试可执行文件并真正运行。
# 说明：flate2 桩不实现真正的压缩，因此依赖 gzip/zlib 的测试会失败——
# 这些失败属于桩的局限，不代表代码有问题，下面会单独列出。
Write-Host ""
Write-Host "3/3 运行单元测试…"
$exe = Join-Path $here 'check.exe'
Remove-Item $exe -Force -ErrorAction SilentlyContinue
& cmd /c "rustc --edition 2021 --crate-type lib --crate-name check --test --extern `"flate2=$stub`" `"$((Join-Path $here 'check.rs'))`" -o `"$exe`" 2>nul"
if (-not (Test-Path $exe)) { Write-Host "  测试可执行文件编译失败"; exit 1 }
$testOut = & $exe --test-threads=4 2>&1
$testOut | Where-Object { $_ -match 'test result|^failures:|^    ' } | Select-Object -First 40
$failed = $testOut | Select-String -Pattern '^test result: FAILED'
$passed = $testOut | Select-String -Pattern '^test result: ok'

# 已知受桩限制的测试：都依赖真实的 gzip/zlib 解压
$stubBound = 'gzip_roundtrip|blockdb::tests::|real_sample|decoded_block_count|metadata_edit_roundtrips'
$names = $testOut | Where-Object { $_ -match '^    [a-z_]+::tests::' } | ForEach-Object { $_.Trim() }
$real = $names | Where-Object { $_ -notmatch $stubBound }
Write-Host ""
if ($passed) {
    Write-Host "测试全部通过。"
    exit 0
}
Write-Host "失败项中与代码相关的:"
if ($real.Count -eq 0) { Write-Host "  无（其余失败均由 flate2 桩不支持压缩引起）" }
else { $real | ForEach-Object { Write-Host "  $_" } }
exit $real.Count
