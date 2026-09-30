# VellumBench Windows 发行包脚本
# 产出 dist/VellumBench-portable.zip:GUI 主程序 + 两个 CLI + 示例 + 调用封装。
#
# 用法(仓库根目录):
#   powershell -ExecutionPolicy Bypass -File dist\package.ps1
# 前置:
#   cargo build --release --workspace
#
# 布局与运行期查找对应(vb_ui::fonts::bundled_dirs):
#   <包根>/vellumbench.exe        —— GUI(启动主页 + 画布)
#   <包根>/kiln-cli.exe           —— 无头导出
#   <包根>/vellum-cli.exe         —— Agent CLI
#   <包根>/assets/fonts/*.ttf     —— 可选:放进来即启用随包字体(Inter/MiSans 等)
#   <包根>/examples/…             —— 示例稿件(kiln-cli 可直接复现 README 产物)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$rel  = Join-Path $root "target\release"
$stage = Join-Path $root "target\package\VellumBench"
$zip   = Join-Path $root "dist\VellumBench-portable.zip"

$bins = @("vellumbench.exe", "kiln-cli.exe", "vellum-cli.exe", "vellum-mcp.exe", "kiln.exe")
foreach ($b in $bins) {
    if (-not (Test-Path (Join-Path $rel $b))) {
        Write-Warning "缺少 $b(可能未构建该 bin),跳过"
    }
}

if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Force -Path $stage | Out-Null

foreach ($b in $bins) {
    $src = Join-Path $rel $b
    if (Test-Path $src) { Copy-Item $src $stage }
}

# 可选运行时组件:存在才打包(pdfium 用于 PDF/AI 导入;ffmpeg 用于 GIF/MP4 桥)
foreach ($dll in @("pdfium.dll")) {
    $envHit = $env:PDFIUM_DLL
    if ($envHit -and (Test-Path $envHit)) { Copy-Item $envHit (Join-Path $stage $dll) }
}

# 示例稿件(验证导出链路最短的复现材料)
Copy-Item -Recurse (Join-Path $root "examples") (Join-Path $stage "examples")

# 调用封装
Copy-Item (Join-Path $root "dist\kiln-call.bat") $stage
Copy-Item (Join-Path $root "dist\kiln-call.ps1") $stage

# 随包字体目录(空目录占位 + 说明):把 Inter/MiSans 的 ttf/otf 放进来即可生效,
# 不放则自动回退系统字体(%WINDIR%\Fonts 的微软雅黑等),界面可正常显示中文
New-Item -ItemType Directory -Force -Path (Join-Path $stage "assets\fonts") | Out-Null
Set-Content -Path (Join-Path $stage "assets\fonts\README.txt") -Encoding UTF8 -Value @(
    "把字体文件放进本目录即可启用随包字体,候选文件名(vb_ui::fonts):",
    "  Inter-Regular.otf/.ttf  Inter-Medium.otf/.ttf  Inter-SemiBold.otf/.ttf",
    "  MiSans-Regular.ttf/.otf  JetBrainsMono-Regular.ttf/.otf",
    "留空 = 使用系统字体回退链(%WINDIR%\Fonts:微软雅黑/等线/黑体/宋体)。",
    "也可用环境变量 VB_FONTS_DIR 指向任意字体目录。"
)

if (Test-Path $zip) { Remove-Item -Force $zip }
Compress-Archive -Path "$stage\*" -DestinationPath $zip
Write-Host "发行包完成: $zip"
Write-Host "自检: 展开后运行 .\kiln-cli.exe export --source examples\poster --output poster.png --format PNG"
