# 带自动清理的测试入口(2026-10-02)
#
# 背景:直接跑 cargo test 会在共享 target/debug 累积测试工件与 incremental 缓存。
# 守卫为本工作区使用独立 Cargo target；达到 10 GB 时先删 incremental，仍超限才重置，
# 测试成功或失败退出后都复核容量，并保留 cargo 原始退出码。
#
# 用法:
#   powershell -ExecutionPolicy Bypass -File tools/test.ps1              # cargo test
#   powershell -ExecutionPolicy Bypass -File tools/test.ps1 -Fast        # 只测受影响包(无变更时不回退全仓)
#   后接 cargo test 参数:powershell -File tools/test.ps1 -CargoArgs "-p vb_kiln"
#
# 所有测试通过 test_artifact_guard.py 在隔离目录运行,目录上限 10 GB(十进制)。
# Agent 会话约定:跑测试一律走本脚本,别裸跑 cargo test。
param(
    [switch]$Fast,
    [string]$CargoArgs = ""
)

$ErrorActionPreference = "Continue"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

# 本机构建四件套:工具链在 D 盘,TEMP 指 D 盘(防 C 盘满截断工件)。
# 只在缺失时补,不覆盖用户已有配置。
if (-not $env:CARGO_HOME)   { $env:CARGO_HOME = "D:\tools\rust-home\cargo" }
if (-not $env:RUSTUP_HOME)  { $env:RUSTUP_HOME = "D:\tools\rust-home\rustup" }
if ($env:TEMP -like "C:*" -or -not $env:TEMP) { $env:TEMP = "D:\Temp"; $env:TMP = "D:\Temp" }
if (-not (Test-Path $env:TEMP)) { New-Item -ItemType Directory -Force -Path $env:TEMP | Out-Null }

# 并发安全档(2026-10-02 实测):本机 16GB RAM + 4GB 页面文件,空闲提交已
# 16.6GB,满核并行 rustc 必撞提交上限——症状是 rustc 成批 0xc0000409 崩溃
# + "rlib required but not found" 级联。2 是实测安全档;扩页面文件(重启
# 生效)后可用 CARGO_BUILD_JOBS 环境变量覆盖为更高。
if (-not $env:CARGO_BUILD_JOBS) { $env:CARGO_BUILD_JOBS = "2" }

$Python = Get-Command python -ErrorAction SilentlyContinue
$PythonArgs = @()
if (-not $Python) { $Python = Get-Command python3 -ErrorAction SilentlyContinue }
if (-not $Python) {
    $Python = Get-Command py -ErrorAction SilentlyContinue
    if ($Python) { $PythonArgs = @("-3") }
}
if (-not $Python) {
    Write-Error "需要 Python 3.10+ 才能运行有 10 GB 容量守卫的测试入口。"
    exit 127
}

$TestCommand = @("test")
if ($Fast) {
    # 快档只测 git status 中有变更的 vb_* crate；无命中时禁止静默跑全仓。
    $Changed = git status --short | ForEach-Object {
        if ($_ -match '^\s*[MADRCU?]{1,2}\s+crates/(vb_[a-z]+)/') { $Matches[1] }
    } | Sort-Object -Unique
    if ($Changed) {
        foreach ($Crate in $Changed) { $TestCommand += @("-p", $Crate) }
    } elseif (-not $CargoArgs) {
        Write-Error "-Fast 未发现已变更 crate；请指定 -CargoArgs，或显式移除 -Fast 运行全仓。"
        exit 2
    }
}
$TestCommand += @($CargoArgs -split '\s+' | Where-Object { $_ })

# 在独立 test target 中执行；脚本在退出(包括失败)时执行容量检查与清理。
& $Python.Source @PythonArgs (Join-Path $PSScriptRoot "test_artifact_guard.py") run -- cargo @TestCommand
exit $LASTEXITCODE
