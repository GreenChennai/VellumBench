# 画布 ↔ 导出 一致性门禁(W7,副文档 03 §4 / 03-3)。
#
# 对每个示例的每个画板分别取两份图:
#   ① 画布侧:vellumbench --canvas-shot(真实画布 GPU 纹理读回;
#      含"窗口增长探针"—— 画布矩形不能随窗口增长的缺陷在此必然红)
#   ② 导出侧:vellum-cli export --artboard <sid> --format png(CPU 真值)
#
# 判据:
#   硬判据(拦截):
#     H1 manifest 无错误且 fully_visible(画板在画布纹理内完整可见)
#     H2 画布上画板矩形尺寸 == round(声明尺寸 × 缩放)(容差 ≤1px)
#     H3 换算后画布画板矩形 == 导出 PNG 尺寸(容差 ≤1px;
#        即 |导出W × zoom × dpi - 画布裁剪W| ≤ 1)
#   软判据(只报告,不拦截):像素相似度分数(0..1,1 = 全同),
#     差异热力图写 $OutDir\diff-*.png。画布文字为 egui 近似层(不在
#     采样的 vello 纹理内)、抗锯齿/DPI 差异都会压低分数 —— 仅供观察趋势。
#
# 用法:
#   powershell -File tools/canvas_parity.ps1                    # 全部示例
#   powershell -File tools/canvas_parity.ps1 -Examples landing  # 指定示例
param(
    [string[]]$Examples = @("landing", "poster", "resume"),
    [string]$OutDir = "$env:TEMP\vb-iter\canvas-parity",
    [string]$Cli = "",
    [string]$App = ""
)

$ErrorActionPreference = "Continue"
# vellum-cli 的 stdout 是 UTF-8:控制台必须按 UTF-8 解码,否则中文 JSON 变
# 乱码导致 ConvertFrom-Json 失败(GBK 代码页机器上必现)
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

if (-not $Cli) {
    $Cli = "target/debug/vellum-cli.exe"
    if (-not (Test-Path $Cli)) { $Cli = "target/release/vellum-cli.exe" }
}
if (-not $App) {
    $App = "target/debug/vellumbench.exe"
    if (-not (Test-Path $App)) { $App = "target/release/vellumbench.exe" }
}
if (-not (Test-Path $Cli)) { Write-Host "找不到 vellum-cli($Cli),先 cargo build" -ForegroundColor Red; exit 1 }
if (-not (Test-Path $App)) { Write-Host "找不到 vellumbench($App),先 cargo build -p vb_app" -ForegroundColor Red; exit 1 }

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$failures = New-Object System.Collections.ArrayList
$softReports = New-Object System.Collections.ArrayList

function Get-PngSize([string]$Path) {
    Add-Type -AssemblyName System.Drawing
    $img = [System.Drawing.Image]::FromFile($Path)
    try { return @([int]$img.Width, [int]$img.Height) } finally { $img.Dispose() }
}

foreach ($ex in $Examples) {
    Write-Host ""
    Write-Host "--- 示例 ${ex} ---" -ForegroundColor Cyan
    $docDir = Join-Path $Root "examples/$ex"
    if (-not (Test-Path (Join-Path $docDir "index.html"))) {
        [void]$failures.Add("${ex}: index.html 不存在")
        continue
    }

    # 画板清单(sid)来自 tree --json(门禁脚本自己发现画板,不写死)
    $treeJson = & $Cli --doc $docDir tree --json 2>$null | ConvertFrom-Json
    if (-not $treeJson -or -not $treeJson.artboards) {
        [void]$failures.Add("${ex}: tree --json 不可读")
        continue
    }
    foreach ($ab in $treeJson.artboards) {
        $sid = $ab.sid
        $name = $ab.name
        $shotPng = Join-Path $OutDir "canvas-$ex-$sid.png"
        $shotJson = Join-Path $OutDir "canvas-$ex-$sid.json"
        $expPng = Join-Path $OutDir "export-$ex-$sid.png"

        # ① 导出侧(CPU 真值)
        & $Cli --doc $docDir export --artboard $sid --format png --out $expPng *> $null
        if (-not (Test-Path $expPng)) {
            [void]$failures.Add("${ex}/${name}: 导出 PNG 失败")
            continue
        }
        $expSize = Get-PngSize $expPng

        # ② 画布侧(真实画布纹理读回;进程自动退出)
        & $App --project $docDir --canvas-shot $shotPng --artboard $sid 2>$null | Out-Null
        if (-not (Test-Path $shotJson)) {
            [void]$failures.Add("${ex}/${name}: canvas-shot 未产出 manifest")
            continue
        }
        # manifest 由应用以 UTF-8(无 BOM)落盘:必须显式按 UTF-8 读,
        # 否则 GBK 代码页下中文变乱码,ConvertFrom-Json 抛错且 $m 残留
        # 上一画板的旧值 → 跨画板串数据(实测踩过)
        $raw = Get-Content $shotJson -Raw -Encoding UTF8
        $m = $null
        try { $m = $raw | ConvertFrom-Json -ErrorAction Stop } catch {
            [void]$failures.Add("${ex}/${name}: manifest 解析失败:$($_.Exception.Message)")
            continue
        }
        if ($m.error) {
            [void]$failures.Add("${ex}/${name}: canvas-shot 错误:$($m.error)")
            continue
        }

        # H1 完整可见
        if (-not $m.fully_visible) {
            [void]$failures.Add(
                "${ex}/${name}: H1 画板在画布纹理内不完整(crop=$($m.crop -join ','), " +
                "texture=$($m.texture_w)x$($m.texture_h))—— 画布视口/纹理小于画板矩形")
            continue
        }

        # H2 画布画板矩形 == 声明尺寸 × 缩放(≤1px)
        $cropW = [double]$m.crop[2]; $cropH = [double]$m.crop[3]
        $wantW = [math]::Round([double]$m.artboard_w * [double]$m.zoom * [double]$m.point_to_px)
        $wantH = [math]::Round([double]$m.artboard_h * [double]$m.zoom * [double]$m.point_to_px)
        if ([math]::Abs($cropW - $wantW) -gt 1 -or [math]::Abs($cropH - $wantH) -gt 1) {
            [void]$failures.Add(
                "${ex}/${name}: H2 画布画板矩形 ${cropW}x${cropH} ≠ 声明 ${wantW}x${wantH}")
            continue
        }

        # H3 换算后 == 导出 PNG 尺寸(≤1px;缩放系数 = zoom × point_to_px)
        $k = [double]$m.zoom * [double]$m.point_to_px
        if ([math]::Abs($expSize[0] * $k - $cropW) -gt 1 -or
            [math]::Abs($expSize[1] * $k - $cropH) -gt 1) {
            [void]$failures.Add(
                "${ex}/${name}: H3 导出 $($expSize[0])x$($expSize[1]) × $k ≠ " +
                "画布 ${cropW}x${cropH}(画布与导出几何不一致)")
            continue
        }

        Write-Host ("  [PASS] {0} ({1}):画布 {2}x{3} == 导出 {4}x{5} × {6:N3}" -f `
            $name, $sid, $cropW, $cropH, $expSize[0], $expSize[1], $k) -ForegroundColor Green

        # 软判据:像素相似度(只报告;夹具生成,禁止手写阈值拦截)
        $py = @'
import json, sys
try:
    from PIL import Image
except ImportError:
    print(json.dumps({"score": None, "reason": "PIL 不可用"})); sys.exit(0)
shot_p, exp_p, diff_p = sys.argv[1], sys.argv[2], sys.argv[3]
shot, exp = Image.open(shot_p).convert("L"), Image.open(exp_p).convert("L")
exp = exp.resize(shot.size)
a, b = shot.getdata(), exp.getdata()
n = len(a)
diff_px = sum(1 for x, y in zip(a, b) if abs(x - y) > 16)
score = 1.0 - diff_px / n
# 差异热力图(红 = 不一致)
heat = Image.new("L", shot.size)
heat.putdata([min(255, abs(x - y) * 3) for x, y in zip(a, b)])
heat.convert("RGB").save(diff_p)
heat.close(); shot.close(); exp.close()
print(json.dumps({"score": round(score, 4), "diff_ratio": round(diff_px / n, 4)}))
'@
        $pyFile = Join-Path $OutDir "soft_score.py"
        [System.IO.File]::WriteAllText($pyFile, $py)
        $diffPng = Join-Path $OutDir "diff-$ex-$sid.png"
        $soft = python -X utf8 $pyFile $shotPng $expPng $diffPng 2>$null | ConvertFrom-Json
        if ($soft -and $null -ne $soft.score) {
            Write-Host ("         软分数(像素相似度)={0}(差异像素占比 {1};差异图 {2})" -f `
                $soft.score, $soft.diff_ratio, $diffPng) -ForegroundColor DarkGray
            [void]$softReports.Add("${ex}/${name} score=$($soft.score)")
        } else {
            Write-Host "         软分数:Python/PIL 不可用,跳过(不影响门禁)" -ForegroundColor DarkGray
        }
    }
}

Write-Host ""
if ($failures.Count -gt 0) {
    Write-Host "=== canvas_parity: FAIL($($failures.Count) 项)===" -ForegroundColor Red
    foreach ($f in $failures) { Write-Host ("  - " + $f) -ForegroundColor Red }
    exit 1
}
Write-Host "=== canvas_parity: PASS(硬判据全通过;$($softReports.Count) 项软分数仅报告)===" -ForegroundColor Green
exit 0
