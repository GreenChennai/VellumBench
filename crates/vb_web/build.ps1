# vb_web 构建脚本(K2,05-11-2):wasm 构建 + 绑定 + 本地示例就位。
# 产物:web/pkg/(可整目录静态托管;web/example/ 为内置示例副本)。
#
# 用法(仓库根):
#   powershell -File crates/vb_web/build.ps1
#   然后任意静态服务器指向 crates/vb_web/web,例如:
#     python -m http.server 8088 -d crates/vb_web/web
#   浏览器打开 http://127.0.0.1:8088/?autotest=1 自动验收。
param(
    [switch]$SkipExample
)
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot   # crates/
$Repo = Split-Path -Parent $Root           # 仓库根
$WebDir = Join-Path $PSScriptRoot "web"

Write-Host "== 1/3 cargo build (wasm32) =="
cargo build -p vb_web --target wasm32-unknown-unknown --release
if ($LASTEXITCODE -ne 0) { exit 1 }

Write-Host "== 2/3 wasm-bindgen(web 绑定) =="
wasm-bindgen --target web --out-dir (Join-Path $WebDir "pkg") `
  (Join-Path $Repo "target\wasm32-unknown-unknown\release\vb_web.wasm")
if ($LASTEXITCODE -ne 0) { exit 1 }

Write-Host "== 3/3 内置示例副本 =="
if (-not $SkipExample) {
    $dst = Join-Path $WebDir "example"
    New-Item -ItemType Directory -Force -Path (Join-Path $dst "styles") | Out-Null
    Copy-Item (Join-Path $Repo "examples/landing/index.html") $dst -Force
    Copy-Item (Join-Path $Repo "examples/landing/styles/main.css") (Join-Path $dst "styles") -Force
    Write-Host "  examples/landing → web/example/"
}
Write-Host "完成。python -m http.server 8088 -d $WebDir 后浏览器打开 /?autotest=1"
