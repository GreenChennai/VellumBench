# 打包脚本(设计文档 09 篇 §十):release 构建 + 归档 + 冒烟
# 用法:powershell -File tools/build.ps1 [-SkipSmoke]
param(
    [switch]$SkipSmoke
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

# 1) 版本与短哈希
$Version = (Select-String -Path "$Root\Cargo.toml" -Pattern '^version = "(.*)"$' |
    Select-Object -First 1).Matches[0].Groups[1].Value
$ShortHash = git rev-parse --short HEAD
$Tag = "$Version-$ShortHash"
$ArchiveRoot = "E:\平日资料\构建\VellumBench"
$ArchiveDir = Join-Path $ArchiveRoot $Tag

Write-Host "=== VellumBench 打包 $Tag ===" -ForegroundColor Cyan

# 2) release 构建
cargo build --release -p vb_app -p vb_agent
if ($LASTEXITCODE -ne 0) { throw "构建失败" }

# 3) 包目录
New-Item -ItemType Directory -Force -Path $ArchiveDir | Out-Null
Copy-Item "$Root\target\release\vellumbench.exe" $ArchiveDir
Copy-Item "$Root\target\release\vellum-cli.exe" $ArchiveDir
Copy-Item "$Root\target\release\vellum-mcp.exe" $ArchiveDir -ErrorAction SilentlyContinue
Copy-Item "$Root\README.md" $ArchiveDir
Copy-Item "$Root\examples" "$ArchiveDir\examples" -Recurse -Force
Set-Content -Path "$ArchiveDir\VERSION.txt" -Value "$Version`ngit: $ShortHash`nbuild: $(Get-Date -Format 'yyyy-MM-dd HH:mm')"

# 4) 离线冒烟:打开示例 → 导出 PNG(全程无 GUI)
if (-not $SkipSmoke) {
    Write-Host "=== 冒烟:selfcheck ==="
    & "$ArchiveDir\vellum-cli.exe" selfcheck
    if ($LASTEXITCODE -ne 0) { throw "selfcheck 失败" }
    Write-Host "=== 冒烟:示例导出 ==="
    $smoke = Join-Path $env:TEMP "vb-smoke-$(Get-Random)"
    New-Item -ItemType Directory -Force -Path $smoke | Out-Null
    & "$ArchiveDir\vellum-cli.exe" --doc "$Root\examples\landing" export `
        --artboard hero --scale 2 --out "$smoke\hero@2x.png" --json
    if (-not (Test-Path "$smoke\hero@2x.png")) { throw "冒烟导出失败" }
    Remove-Item $smoke -Recurse -Force
}

Write-Host "=== 完成:归档于 $ArchiveDir ===" -ForegroundColor Green
