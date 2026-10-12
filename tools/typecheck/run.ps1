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
$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$cargo = "$env:USERPROFILE\.cargo\bin"
if (Test-Path $cargo) { $env:Path = "$cargo;$env:Path" }
$env:CARGO_MANIFEST_DIR = (Resolve-Path "$here\..\..\src-tauri").Path

$stub = Join-Path $here 'libflate2.rlib'
Write-Host "1/2 编译 flate2 桩…"
& rustc --edition 2021 --crate-type lib --crate-name flate2 --emit=metadata,link `
    -o $stub (Join-Path $here 'flate2.rs') 2>$null

Write-Host "2/2 类型检查逻辑模块…"
$out = & rustc --edition 2021 --crate-type lib --crate-name check --test --emit=metadata `
    --extern "flate2=$stub" (Join-Path $here 'check.rs') -o (Join-Path $here 'check.rmeta') 2>&1
$out | Where-Object { $_ -notmatch '^\s*$' }
$errs = @($out | Select-String -Pattern '^error').Count
Write-Host ""
Write-Host "错误: $errs   警告: $(@($out | Select-String -Pattern '^warning').Count)"
exit $errs
