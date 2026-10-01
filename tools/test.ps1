# 带自动清理的测试入口(2026-10-02)
#
# 背景:直接跑 cargo test 会在 target/debug 留下旧哈希测试 exe/pdb 与
# incremental 缓存,多轮迭代把磁盘塞爆(本机 C 盘满曾致 cargo 工件级联损坏)。
# 本脚本是"每测试一次就清理一次"的落地:测试无论成败,清完才返回原退出码。
#
# 用法:
#   powershell -ExecutionPolicy Bypass -File tools/test.ps1              # cargo test
#   powershell -ExecutionPolicy Bypass -File tools/test.ps1 -Fast        # 只测受影响包(默认全仓)
#   后接 cargo test 参数:powershell -File tools/test.ps1 -CargoArgs "-p vb_kiln"
#
# Agent 会话约定:跑测试一律走本脚本,别裸跑 cargo test(见 AGENTS 环境文档)。
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

if ($Fast) {
    # 快档:只测改过的包(git status 里的 crate 前缀);全绿后收工前再跑全量
    $Changed = git status --short | ForEach-Object {
        if ($_ -match '^\s*M?A?\s+crates/(vb_[a-z]+)/') { $Matches[1] }
    } | Sort-Object -Unique
    if ($Changed) {
        cargo test --workspace -p @Changed @($CargoArgs -split '\s+' | Where-Object { $_ })
    } else {
        Write-Host "无已改 crate,回退全量"
        cargo test @($CargoArgs -split '\s+' | Where-Object { $_ })
    }
} else {
    cargo test @($CargoArgs -split '\s+' | Where-Object { $_ })
}
$TestExit = $LASTEXITCODE

# 测试失败更要清:中途 panic/被杀留下的正是最占盘的半成品
powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "test-clean.ps1")

exit $TestExit
