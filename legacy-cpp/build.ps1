# 投影文件管理器 - 构建脚本
#   .\build.ps1            构建 GUI 程序 -> build\<程序名>.exe
#   .\build.ps1 -Test      构建控制台测试 -> build\tests.exe
#   .\build.ps1 -Clean     清理
param(
    [switch]$Test,
    [switch]$Clean,
    [switch]$NoStrip
)
$ErrorActionPreference = 'Stop'

$Root  = Split-Path -Parent $MyInvocation.MyCommand.Path
$Mingw = Join-Path (Split-Path -Parent $Root) 'tools\mingw\mingw32'
$Bin   = Join-Path $Mingw 'bin'
$Out   = Join-Path $Root 'build'

if (-not (Test-Path (Join-Path $Bin 'g++.exe'))) { throw "找不到工具链: $Bin\g++.exe" }
$env:PATH = "$Bin;$env:PATH"

if ($Clean) {
    Remove-Item $Out -Recurse -Force -ErrorAction SilentlyContinue
    Write-Host "已清理 $Out"
    return
}
New-Item -ItemType Directory -Force -Path $Out | Out-Null

# Win7 目标 (6.01)，静态链接全部运行库 -> 零依赖单文件
$Common = @(
    '-std=c++17', '-O2',
    '-static', '-static-libgcc', '-static-libstdc++',
    '-D_WIN32_WINNT=0x0601', '-DWINVER=0x0601',
    '-DUNICODE', '-D_UNICODE', '-DNDEBUG',
    '-fno-ident', '-ffunction-sections', '-fdata-sections',
    '-Wl,--gc-sections',
    '-Wl,--major-subsystem-version,6,--minor-subsystem-version,1',
    '-Wl,--major-os-version,6,--minor-os-version,1',
    '-Wall', '-Wno-unused-parameter', '-Wno-unused-variable',
    '-Wno-unused-but-set-variable', '-Wno-missing-field-initializers',
    '-Wno-sign-compare', '-Wno-cast-function-type', '-Wno-unused-function'
)
if (-not $NoStrip) { $Common += '-s' }

$Libs = @('-lz', '-lgdi32', '-luser32', '-lcomdlg32', '-lshell32', '-lole32', '-luuid', '-lmsimg32')

function Invoke-Gpp {
    param([string[]]$Sources, [string]$OutFile, [string[]]$Extra)
    $gppArgs = @($Common) + @($Extra) + @($Sources) + @('-o', $OutFile) + @($Libs)
    Write-Host "编译 -> $OutFile" -ForegroundColor Cyan
    & g++ @gppArgs
    if ($LASTEXITCODE -ne 0) { throw "编译失败 (exit $LASTEXITCODE)" }
}

$SrcDir = Join-Path $Root 'src'
$CoreSources = Get-ChildItem $SrcDir -Recurse -Filter '*.cpp' |
    Where-Object { $_.Name -ne 'main.cpp' } | ForEach-Object { $_.FullName }

if ($Test) {
    $TestSources = Get-ChildItem (Join-Path $Root 'tests') -Recurse -Filter '*.cpp' |
        ForEach-Object { $_.FullName }
    Invoke-Gpp -Sources ($CoreSources + $TestSources) `
        -OutFile (Join-Path $Out 'tests.exe') -Extra @('-mconsole')
    $exe = Join-Path $Out 'tests.exe'
} else {
    $AllSources = Get-ChildItem $SrcDir -Recurse -Filter '*.cpp' | ForEach-Object { $_.FullName }
    $extra = @('-municode', '-mwindows')

    # 把内置方块库拷到 res 供 windres 打包
    $Bdz = Join-Path $Root 'data\blocks.dat.z'
    if (Test-Path $Bdz) {
        Copy-Item $Bdz (Join-Path $Root 'res\blocks.dat.z') -Force
    } else {
        Write-Warning "找不到 $Bdz，程序将无法显示方块名与预览"
    }

    $RcFile = Join-Path $Root 'res\app.rc'
    if (Test-Path $RcFile) {
        $ResObj = Join-Path $Out 'app_res.o'
        & windres -I (Join-Path $Root 'res') -i $RcFile -o $ResObj
        if ($LASTEXITCODE -ne 0) { throw "windres 失败" }
        $extra += $ResObj
    }
    $NameFile = Join-Path $Root 'res\appname.txt'
    $AppName = if (Test-Path $NameFile) {
        (Get-Content $NameFile -Encoding UTF8 -TotalCount 1).Trim()
    } else { 'SchematicManager' }
    # 注意：中文路径直接传给 gcc 会因 PowerShell 参数编码而走样。
    # 先用 ASCII 名编译，再用 PowerShell 自己重命名（Unicode 安全）。
    $tmpExe = Join-Path $Out 'SchematicManager.exe'
    Invoke-Gpp -Sources $AllSources -OutFile $tmpExe -Extra $extra
    $exe = Join-Path $Out "$AppName.exe"
    if (Test-Path $exe) { Remove-Item $exe -Force }
    Move-Item $tmpExe $exe -Force
}

Write-Host ("完成: {0}  ({1:N1} KB)" -f $exe, ((Get-Item $exe).Length / 1KB)) -ForegroundColor Green
