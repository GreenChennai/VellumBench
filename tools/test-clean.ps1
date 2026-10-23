# 兼容入口：清理 VellumBench 专属 Cargo 测试产物(10 GB 十进制上限)
# 用法: powershell -ExecutionPolicy Bypass -File tools/test-clean.ps1 [-Full]
# 常规模式只在超过上限时清理 incremental 缓存，必要时重置隔离的 test target。
# -Full 清空该工作区的隔离 test target；不会碰其他项目或普通 target/debug。
param(
    [switch]$Full
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

$Python = Get-Command python -ErrorAction SilentlyContinue
$PythonArgs = @()
if (-not $Python) { $Python = Get-Command python3 -ErrorAction SilentlyContinue }
if (-not $Python) {
    $Python = Get-Command py -ErrorAction SilentlyContinue
    if ($Python) { $PythonArgs = @("-3") }
}
if (-not $Python) {
    Write-Error "需要 Python 3.10+ 才能运行测试产物容量守卫。"
    exit 127
}

$Guard = Join-Path $PSScriptRoot "test_artifact_guard.py"
$Arguments = @($Guard, "clean")
if ($Full) { $Arguments += "--full" }
& $Python.Source @PythonArgs @Arguments
exit $LASTEXITCODE
