#!/usr/bin/env pwsh
# 前端检查：类型检查 + 构建。
# 需要 node 与已安装的依赖（npm install，本机 pnpm 亦可）。
$ErrorActionPreference = 'Continue'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location (Resolve-Path "$here\..\..")
$node = "C:\Program Files\nodejs"
if (Test-Path $node) { $env:Path = "$node;$env:APPDATA\npm;$env:Path" }

Write-Host "1/2 类型检查（vue-tsc）…"
& npx vue-tsc --noEmit
$tsErr = $LASTEXITCODE

Write-Host ""
Write-Host "2/2 构建（vite build）…"
& npx vite build
$buildErr = $LASTEXITCODE

Write-Host ""
if ($tsErr -eq 0 -and $buildErr -eq 0) { Write-Host "前端检查通过。" }
else { Write-Host "前端检查失败：vue-tsc=$tsErr  vite=$buildErr" }
exit ($tsErr + $buildErr)
