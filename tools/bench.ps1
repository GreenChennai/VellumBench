# tools/bench.ps1 — P5.3 性能基线记录与回归告警(10 篇门禁 5 的本地落地)
#
# 用法:
#   pwsh tools/bench.ps1                 # 跑基线,追加 bench-history.json,超回归阈值报错
#   pwsh tools/bench.ps1 -RecordOnly     # 只记录,不做回归判定(首个基线用)
#
# 策略:release 构建一次 vellum-cli,对 200/2000/10000 对象各跑 3 轮取最小值
# (min 对抖动最稳),与历史基线的中位数比较,回退 >50% 视为回归(本地硬件
# 抖动大,不用 10 篇的 10% 硬阈值;正式 CI 硬件就位后再收紧)。
param(
    [switch]$RecordOnly,
    [string]$HistoryPath = (Join-Path $PSScriptRoot "..\bench-history.json")
)
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

Write-Host "=== P5.3 性能基线(B1 构建画像 / 编码 / 渲染)===" -ForegroundColor Cyan
Write-Host "  构建 release vellum-cli…"
cargo build --release --bin vellum-cli 2>&1 | Out-Null
$cli = Join-Path (Get-Location) "target\release\vellum-cli.exe"
if (-not (Test-Path $cli)) { throw "vellum-cli 构建失败" }

$objects = @(200, 2000, 10000)
$runs = @()
foreach ($n in $objects) {
    $best = $null
    for ($i = 0; $i -lt 3; $i++) {
        $out = & $cli bench --objects $n --json | ConvertFrom-Json
        if ($null -eq $best -or $out.total_ms -lt $best.total_ms) { $best = $out }
    }
    $runs += $best
    Write-Host ("  {0,6} 对象: build {1}ms / encode {2}ms / render {3}ms / total {4}ms" -f `
        $best.objects, $best.build_ms, $best.encode_ms, $best.render_ms, $best.total_ms)
}

$hash = (git rev-parse --short HEAD) 2>$null
if (-not $hash) { $hash = "unknown" }
$entry = [ordered]@{
    ts       = (Get-Date -Format "yyyy-MM-ddTHH:mm:sszzz")
    git      = $hash
    runs     = $runs
}

# 历史:读 → 判回归 → 追加(环形保留最近 50 条)
$history = @()
if (Test-Path $HistoryPath) {
    try { $history = Get-Content $HistoryPath -Raw | ConvertFrom-Json } catch { $history = @() }
}
$failed = $false
if (-not $RecordOnly -and $history.Count -gt 0) {
    foreach ($r in $runs) {
        $past = @()
        foreach ($h in $history) {
            foreach ($pr in $h.runs) {
                if ($pr.objects -eq $r.objects) { $past += [double]$pr.total_ms }
            }
        }
        if ($past.Count -eq 0) { continue }
        $sorted = $past | Sort-Object
        $median = $sorted[[int][Math]::Floor($sorted.Count / 2)]
        $ratio = $r.total_ms / [Math]::Max($median, 1)
        $tag = if ($ratio -gt 1.5) { "REGRESSION" } else { "ok" }
        Write-Host ("  {0} 对象: {1}ms vs 基线中位 {2}ms → x{3:N2} [{4}]" -f `
            $r.objects, $r.total_ms, $median, $ratio, $tag)
        if ($ratio -gt 1.5) { $failed = $true }
    }
}

$history = @($history) + $entry
if ($history.Count -gt 50) { $history = $history[-50..-1] }
$history | ConvertTo-Json -Depth 6 | Set-Content $HistoryPath -Encoding UTF8
Write-Host ("  已记录 → {0}({1} 条历史)" -f $HistoryPath, $history.Count)

if ($failed) {
    Write-Host "  性能回归 >50% — 请对照 bench-history.json 定位提交" -ForegroundColor Red
    exit 1
}
Write-Host "  [PASS] 性能基线" -ForegroundColor Green
