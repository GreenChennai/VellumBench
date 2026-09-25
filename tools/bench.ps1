# tools/bench.ps1 — P5.3 性能基线记录与回归告警(10 篇门禁 5 的本地落地)
#
# 用法:
#   pwsh tools/bench.ps1                 # 跑基线,追加 bench-history.json,超回归阈值报错
#   pwsh tools/bench.ps1 -RecordOnly     # 只记录,不做回归判定(首个基线用)
#   pwsh tools/bench.ps1 -Boot           # 06-3:冷启动/打开耗时基线(主页首帧/项目首帧/
#                                        #   landing 打开/大文档夹具打开),追加历史
#
# 策略:release 构建一次 vellum-cli,对 200/2000/10000 对象各跑 3 轮取最小值
# (min 对抖动最稳),与历史基线的中位数比较,回退 >50% 视为回归(本地硬件
# 抖动大,不用 10 篇的 10% 硬阈值;正式 CI 硬件就位后再收紧)。
#
# -Boot 取数方法(06-3-1,成文防失传):以 VB_FPS_LOG=1 启动 GUI,测量
# 「进程启动 → stderr 出现 `[fps-log] <home|project> first-frame` 一行」的
# 墙钟时长。该标记由 shell.rs(主页根视口)与 app/frame.rs(项目窗口)在
# 各自首帧打出 —— 含进程/GPU/字体初始化与文档打开全链,是「到主页可见 /
# 到画布首帧」的保守下界(可交互略晚于首帧,不虚报)。每项 3 轮取最小。
# 大文档夹具(06-3-5)在 %TEMP%\vb-bigdoc-fixture 生成(数百节点,不入仓库),
# 只测打开首帧;缩放响应无法经无头通道自动化,留人工取证(诚实边界)。
param(
    [switch]$RecordOnly,
    [switch]$Boot,
    [string]$HistoryPath = (Join-Path $PSScriptRoot "..\bench-history.json")
)
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

function Get-GitShort {
    $h = (git rev-parse --short HEAD) 2>$null
    if ($h) { return $h } else { return "unknown" }
}

function Add-HistoryEntry($Entry) {
    $history = @()
    if (Test-Path $HistoryPath) {
        try { $history = Get-Content $HistoryPath -Raw | ConvertFrom-Json } catch { $history = @() }
    }
    $history = @($history) + $Entry
    if ($history.Count -gt 50) { $history = $history[-50..-1] }
    $history | ConvertTo-Json -Depth 6 | Set-Content $HistoryPath -Encoding UTF8
    Write-Host ("  已记录 → {0}({1} 条历史)" -f $HistoryPath, $history.Count)
}

# 大文档夹具生成(%TEMP%;数百节点绝对定位,形状与 examples 同构)
function New-BigDocFixture([int]$Nodes) {
    $dir = Join-Path $env:TEMP "vb-bigdoc-fixture"
    New-Item -ItemType Directory -Force -Path (Join-Path $dir "styles") | Out-Null
    $sb = New-Object System.Text.StringBuilder
    [void]$sb.AppendLine('<!DOCTYPE html><html lang="zh-CN"><head><meta charset="UTF-8"><title>大文档夹具</title>')
    [void]$sb.AppendLine('<link rel="stylesheet" href="styles/main.css"></head><body>')
    [void]$sb.AppendLine('  <section class="vb-artboard big" data-vb-id="b00001" data-vb-name="大文档">')
    for ($i = 0; $i -lt $Nodes; $i++) {
        $x = ($i % 25) * 56
        $y = [math]::Floor($i / 25) * 56
        [void]$sb.AppendLine(('    <div class="cell" style="left:{0}px;top:{1}px" data-vb-id="n{2:d5}" data-vb-name="块{2}"></div>' -f $x, $y, ($i + 2)))
    }
    [void]$sb.AppendLine('  </section></body></html>')
    Set-Content -Path (Join-Path $dir "index.html") -Value $sb.ToString() -Encoding UTF8
    Set-Content -Path (Join-Path $dir "styles\main.css") -Value ".vb-artboard{position:relative;width:1500px;height:1200px;background:#ffffff}.cell{position:absolute;width:40px;height:40px;background:#3b82f6}" -Encoding UTF8
    return $dir
}

# 冷启动取数:进程启动 → stderr 首个匹配 $Marker 的 [fps-log] 行(60s 超时)
function Measure-BootFirstFrame([string[]]$AppArgs, [string]$Marker, [string]$Exe, [string]$Root) {
    $best = $null
    for ($i = 0; $i -lt 3; $i++) {
        $tag = Get-Random
        $errLog = Join-Path $env:TEMP "vb-boot-$tag-err.log"
        $outLog = Join-Path $env:TEMP "vb-boot-$tag-out.log"
        Remove-Item $errLog, $outLog -ErrorAction SilentlyContinue
        $env:VB_FPS_LOG = "1"
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        $p = Start-Process -FilePath $Exe -ArgumentList $AppArgs -WorkingDirectory $Root -PassThru `
            -RedirectStandardError $errLog -RedirectStandardOutput $outLog
        $ms = $null
        while ($sw.Elapsed.TotalSeconds -lt 60 -and -not $p.HasExited) {
            Start-Sleep -Milliseconds 10
            $hit = Select-String -Path $errLog -Pattern "fps-log.*first-frame" -ErrorAction SilentlyContinue |
                Where-Object { $_.Line -match $Marker } | Select-Object -First 1
            if ($hit) { $ms = $sw.ElapsedMilliseconds; break }
        }
        if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
        if ($null -ne $ms -and ($null -eq $best -or $ms -lt $best)) { $best = $ms }
        Remove-Item $errLog, $outLog -ErrorAction SilentlyContinue
    }
    return $best
}

# ────────────── -Boot:冷启动 / 打开耗时基线(06-3) ──────────────
if ($Boot) {
    Write-Host "=== 06-3 冷启动 / 打开耗时基线 ===" -ForegroundColor Cyan
    Write-Host "  构建 release vellumbench…"
    cargo build --release -p vb_app --bin vellumbench 2>&1 | Out-Null
    $exe = Join-Path (Get-Location) "target\release\vellumbench.exe"
    if (-not (Test-Path $exe)) { throw "vellumbench 构建失败" }

    # 大文档夹具(数百节点;写 %TEMP%,不入仓库)
    $nodes = 500
    $fixture = New-BigDocFixture -Nodes $nodes
    Write-Host ("  大文档夹具:{0} 节点 → {1}" -f $nodes, $fixture)

    $launcherMs = Measure-BootFirstFrame -AppArgs @() -Marker "home first-frame" -Exe $exe -Root (Get-Location)
    $projectMs  = Measure-BootFirstFrame -AppArgs @("--project", "examples\poster") -Marker "project first-frame" -Exe $exe -Root (Get-Location)
    $landingMs  = Measure-BootFirstFrame -AppArgs @("--project", "examples\landing") -Marker "project first-frame" -Exe $exe -Root (Get-Location)
    $bigdocMs   = Measure-BootFirstFrame -AppArgs @("--project", $fixture) -Marker "project first-frame" -Exe $exe -Root (Get-Location)

    Write-Host ("  ① 冷启动到主页可见(home first-frame):{0} ms" -f $launcherMs)
    Write-Host ("  ② 冷启动 --project 到画布首帧(poster):{0} ms" -f $projectMs)
    Write-Host ("  ③ examples/landing 打开到画布首帧:{0} ms" -f $landingMs)
    Write-Host ("  ④ 大文档夹件({1} 节点)打开到画布首帧:{0} ms" -f $bigdocMs, $nodes)

    $entry = [ordered]@{
        ts  = (Get-Date -Format "yyyy-MM-ddTHH:mm:sszzz")
        git = Get-GitShort
        boot = [ordered]@{
            launcher_ms    = $launcherMs
            project_ms     = $projectMs
            landing_ms     = $landingMs
            bigdoc_nodes   = $nodes
            bigdoc_open_ms = $bigdocMs
            method         = "进程启动→VB_FPS_LOG 首帧标记(home/project first-frame);3 轮取最小;夹具 %TEMP%\vb-bigdoc-fixture"
        }
    }
    Add-HistoryEntry $entry
    if ($null -eq $launcherMs -or $null -eq $projectMs) {
        Write-Host "  [FAIL] 冷启动基线取数失败(60s 内未见首帧标记)" -ForegroundColor Red
        exit 1
    }
    Write-Host "  [PASS] 冷启动基线(记录模式,无回归阈值;预算另见 docs/ 与迭代报告)" -ForegroundColor Green
    exit 0
}

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
