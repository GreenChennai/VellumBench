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

# 随包字体(UI-14):仓库 assets/fonts 已内置 Inter Regular/Medium/SemiBold
# (SIL OFL 1.1)与 MiSans Regular(小米免费商用,许可文本随目录分发),
# 打包时原样带入;运行期查找布局与 vb_ui::fonts::bundled_dirs 对应。
# 目录为空时自动回退系统字体(%WINDIR%\Fonts 的微软雅黑等),中文仍可显示。
New-Item -ItemType Directory -Force -Path (Join-Path $stage "assets\fonts") | Out-Null
$repoFonts = Join-Path $root "assets\fonts"
if (Test-Path $repoFonts) {
    Copy-Item -Path (Join-Path $repoFonts "*") -Destination (Join-Path $stage "assets\fonts") -Force
}
Set-Content -Path (Join-Path $stage "assets\fonts\README.txt") -Encoding UTF8 -Value @(
    "本目录字体随包分发:Inter(SIL OFL 1.1,见 LICENSE-OFL.txt)+ MiSans(小米免费商用,见 LICENSE-MiSans.txt)。",
    "候选文件名(vb_ui::fonts):Inter-Regular/Medium/SemiBold.otf|.ttf,MiSans-Regular.ttf,JetBrainsMono-Regular.ttf。",
    "删除字体文件 = 回退系统字体链(%WINDIR%\Fonts);也可用环境变量 VB_FONTS_DIR 指向任意字体目录。"
)

if (Test-Path $zip) { Remove-Item -Force $zip }
Compress-Archive -Path "$stage\*" -DestinationPath $zip
Write-Host "发行包完成: $zip"
Write-Host "自检: 展开后运行 .\kiln-cli.exe export --source examples\poster --output poster.png --format PNG"
