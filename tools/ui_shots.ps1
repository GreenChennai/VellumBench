# UI 截图基线与比对(04-7 / 副文档 04 P0-⑥ P1-①②③ 的防复发门禁)。
#
# 功能:
#   -Generate  启动固定项目(examples/landing)、固定窗口尺寸、固定面板态,
#              用 PrintWindow 抓取关键界面 → 存 tools/baselines/ui/<dpi>/;
#   -Compare   重新抓取当前截图,与基线做像素 diff(内嵌 Python PIL),
#              差异超阈值**只报告清单,不拦截**(阈值拦截留阶段 6 CI 分档)。
#
# 场景(与 04-7-2 对应):首屏 / 七面板全开 / 命令面板 / 开发者统计 / 主页。
# DPI 基线(04-3-4):100% / 125% / 150% / 200% 各存一张首屏,经
# VB_UI_SCALE 环境变量夹具驱动(叠加在系统 DPI 之上的 UI 缩放因子)。
#
# 确定性手段:
#   - VB_WORKSPACE 指向脚本写死的固定工作区文件(工具栏底部 / 坞展开 /
#     次级坞默认),布局不随机器真实 workspace.json 漂移;
#   - VB_SMOKE_COMMAND 在首帧派发场景命令(04-7 起支持 + 分隔多条);
#   - 抓图等待 SettleMs(默认 2500ms),避开首帧面板未稳与自动 fit 前的空帧。
#
# 用法:
#   powershell -File tools/ui_shots.ps1 -Generate          # 生成基线
#   powershell -File tools/ui_shots.ps1 -Compare           # 比对(只报告)
#   powershell -File tools/ui_shots.ps1 -Generate -Scale 1.0,1.5  # 只生成部分档
#   powershell -File tools/ui_shots.ps1 -Generate -Theme dark,light -OutDir <目录>
#       # 07-I:明/暗两主题各跑一轮(经 VB_THEME 夹具驱动),存到自定目录
param(
    [switch]$Generate,
    [switch]$Compare,
    [double[]]$Scale = @(1.0, 1.25, 1.5, 2.0),
    [string]$Project = "examples/landing",
    [int]$SettleMs = 2500,
    # 07-I:主题档(dark / light)。默认 dark = 历史基线路径(无主题子目录),
    # 请求 light 或多主题时输出进 <dpi>\<theme>\ 子目录(不动旧基线)。
    [string[]]$Theme = @("dark"),
    [string]$OutDir = ""
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root
$Exe = "target/debug/vellumbench.exe"
$BaseDir = if ($OutDir) { $OutDir } else { Join-Path $PSScriptRoot "baselines/ui" }
$UseThemeDir = ($Theme.Count -gt 1) -or ($Theme -notcontains "dark")
$WsFile = Join-Path $env:TEMP "vb-ui-shots-workspace.json"

if (-not (Test-Path $Exe)) {
    Write-Host "未找到 $Exe,先构建…" -ForegroundColor Yellow
    cargo build -p vb_app
    if ($LASTEXITCODE -ne 0) { throw "构建失败" }
}

# ── 固定工作区(布局确定性的单一来源;schema v2 + 04-3/04-6/07-K 字段)──
$ws = @"
{
  "schema_version": 2,
  "toolbar_dock": "bottom",
  "toolbar_columns": 1,
  "dock_collapsed": false,
  "panel_order": [0, 1, 2, 3],
  "panel_tab": 0,
  "panels_hidden": false,
  "sec_floating": [false, false, false, false, false, false, false, false, false, false, false],
  "sec_pos": [[0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0], [0, 0]],
  "sec_group_order": [0, 1, 2, 3, 4],
  "sec_active_group": 0,
  "dev_stats": false,
  "hints": true,
  "ui_scale": 1.0,
  "show_all_tools": false
}
"@
# PS5.1 的 UTF8 带 BOM,serde_json 会拒读 → 必须**无 BOM**写入
    [System.IO.File]::WriteAllText($WsFile, $ws)

# ── Win32 抓窗(PrintWindow + PW_RENDERFULLCONTENT,兼容 GPU/DWM 窗口)──
# 注意:vellumbench 是控制台子系统程序,进程还会有若干辅助小窗
# (含一个 16×16 的可见占位窗),Process.MainWindowHandle 常指错 →
# 必须按 PID 枚举顶层窗口,取**最大的可见窗口**才是渲染主窗。
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public class VbWin32 {
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hwnd, IntPtr hdc, uint flags);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out RECT rect);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc proc, IntPtr lParam);
    public delegate bool EnumProc(IntPtr hwnd, IntPtr lParam);
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left, Top, Right, Bottom; }

    // 主窗判定:属于该 PID、可见、宽 > 400 的窗口里取最宽的一个。
    public static IntPtr FindMainWindow(int pid) {
        IntPtr best = IntPtr.Zero;
        int bestW = 400;
        EnumWindows(delegate(IntPtr h, IntPtr _l) {
            uint wpid; GetWindowThreadProcessId(h, out wpid);
            if (wpid == (uint)pid && IsWindowVisible(h)) {
                RECT r; GetWindowRect(h, out r);
                int w = r.Right - r.Left;
                if (w > bestW) { bestW = w; best = h; }
            }
            return true;
        }, IntPtr.Zero);
        return best;
    }
}
"@
[VbWin32]::SetProcessDPIAware() | Out-Null

function Wait-Window([System.Diagnostics.Process]$Proc, [int]$TimeoutMs) {
    # wgpu/vello 初始化 + 字体安装较慢(实机 ~9s),窗口出现前先轮询 PID 枚举
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    while ($sw.ElapsedMilliseconds -lt $TimeoutMs) {
        if ($Proc.HasExited) { throw "应用进程提前退出(码 $($Proc.ExitCode))" }
        $hwnd = [VbWin32]::FindMainWindow($Proc.Id)
        if ($hwnd -ne [IntPtr]::Zero) { return $hwnd }
        Start-Sleep -Milliseconds 400
    }
    throw "等待主窗口超时(${TimeoutMs}ms)"
}

function Capture-Window([IntPtr]$Hwnd, [string]$OutPath) {
    [System.IO.Directory]::CreateDirectory((Split-Path $OutPath)) | Out-Null
    $rect = New-Object VbWin32+RECT
    [VbWin32]::GetWindowRect($Hwnd, [ref]$rect) | Out-Null
    $w = $rect.Right - $rect.Left
    $h = $rect.Bottom - $rect.Top
    if ($w -le 0 -or $h -le 0) { throw "窗口矩形为空:${w}x${h}" }
    $bmp = New-Object System.Drawing.Bitmap($w, $h)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $hdc = $g.GetHdc()
    [VbWin32]::PrintWindow($Hwnd, $hdc, 2) | Out-Null   # 2 = PW_RENDERFULLCONTENT
    $g.ReleaseHdc($hdc)
    $g.Dispose()
    if ($bmp.GetPixel(5, 5).A -eq 0) {
        Write-Host "  [警告] 抓图边角全透明(PrintWindow 可能未拿到 DWM 内容)" -ForegroundColor Yellow
    }
    $bmp.Save($OutPath, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
}

# 场景表:name = 基线文件名;smoke = 首帧派发的命令(VB_SMOKE_COMMAND);
# home = 无项目启动(主页窗口)。文件名用 ASCII,避免脚本编码差异。
$panelsCmd = (@(
    "view.toggle_char_panel", "view.toggle_para_panel", "view.toggle_appearance_panel",
    "view.toggle_stroke_panel", "view.toggle_gradient_panel", "view.toggle_opacity_panel",
    "view.toggle_color_panel", "view.toggle_transform_panel", "view.toggle_align_panel"
) -join "+")
$Scenarios = @(
    @{ Name = "first.png";  Smoke = ""; }
    @{ Name = "panels7.png"; Smoke = $panelsCmd; }
    @{ Name = "palette.png"; Smoke = "app.command_palette"; }
    @{ Name = "devstats.png"; Smoke = "view.developer_stats"; }
    @{ Name = "home.png";   Smoke = ""; Home = $true; }
)

function Invoke-Shot([hashtable]$Sc, [double]$UiScale, [string]$OutPath, [string]$ThemeName = "dark") {
    # 每次启动前**重写**固定工作区:应用会把开关变化(如开发者统计)写回
    # 该文件,不重写会污染后续场景(实测 devstats 场景后的截图全带统计窗)。
    # PS5.1 的 UTF8 带 BOM,serde_json 会拒读 → 必须**无 BOM**写入
    [System.IO.File]::WriteAllText($WsFile, $ws)
    $env:VB_WORKSPACE = $WsFile
    $env:VB_UI_SCALE = "$UiScale"
    # 07-I:主题夹具(整壳生效:主页与项目窗口同一真相)
    if ($ThemeName -eq "light") { $env:VB_THEME = "light" } else { Remove-Item Env:VB_THEME -ErrorAction SilentlyContinue }
    if ($Sc.Smoke) { $env:VB_SMOKE_COMMAND = $Sc.Smoke } else { Remove-Item Env:VB_SMOKE_COMMAND -ErrorAction SilentlyContinue }
    # PS5.1 的 Start-Process 拒绝空 -ArgumentList → 主页场景(无参)用 splatting 跳过
    $psi = @{ FilePath = $Exe; PassThru = $true }
    if (-not $Sc.Home) { $psi.ArgumentList = @("--project", $Project) }
    $proc = Start-Process @psi
    try {
        $hwnd = Wait-Window $proc 30000
        Start-Sleep -Milliseconds $SettleMs
        Capture-Window $hwnd $OutPath
        Write-Host ("  [OK] {0} (scale {1}, theme {2})" -f (Split-Path $OutPath -Leaf), $UiScale, $ThemeName) -ForegroundColor Green
    } finally {
        if (-not $proc.HasExited) { Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue }
        Start-Sleep -Milliseconds 300
    }
}

function Get-ScenarioList([bool]$IsPrimary) {
    # 主档位(第一个 scale)抓全场景;其余档位只抓首屏(多 DPI 对比,04-3-4)
    if ($IsPrimary) { return $Scenarios }
    @($Scenarios | Where-Object { $_.Name -eq "first.png" })
}

if ($Generate) {
    Write-Host "=== 生成 UI 截图基线 → $BaseDir ===" -ForegroundColor White
    foreach ($th in $Theme) {
        foreach ($s in $Scale) {
            $dpiDir = Join-Path $BaseDir ("{0}" -f [int]($s * 100))
            # 07-I:多主题/浅色时加主题子目录;默认单 dark 走历史路径(不动旧基线)
            if ($UseThemeDir) { $dpiDir = Join-Path $dpiDir $th }
            foreach ($sc in (Get-ScenarioList ($s -eq $Scale[0]))) {
                Invoke-Shot $sc $s (Join-Path $dpiDir $sc.Name) $th
            }
        }
    }
    Remove-Item Env:VB_THEME -ErrorAction SilentlyContinue
    Write-Host "基线生成完毕。" -ForegroundColor Green
}

if ($Compare) {
    Write-Host "=== 与基线比对(报告模式,不拦截)===" -ForegroundColor White
    $report = New-Object System.Collections.ArrayList
    foreach ($s in $Scale) {
        $dpiDir = Join-Path $BaseDir ("{0}" -f [int]($s * 100))
        foreach ($sc in (Get-ScenarioList ($s -eq $Scale[0]))) {
            $base = Join-Path $dpiDir $sc.Name
            if (-not (Test-Path $base)) {
                [void]$report.Add(@{ Name = $sc.Name; Scale = $s; Status = "MISS"; Ratio = -1; Note = "缺基线 $base" })
                continue
            }
            $cur = Join-Path $env:TEMP ("vb-ui-shot-cur-{0}-{1}" -f $s, $sc.Name)
            Invoke-Shot $sc $s $cur
            # 内嵌 Python PIL 像素 diff(与仓库门禁的 Python 先例同源)
            $py = @'
import sys
from PIL import Image
base_path, cur_path = sys.argv[1], sys.argv[2]
a, b = Image.open(base_path).convert("RGB"), Image.open(cur_path).convert("RGB")
if a.size != b.size:
    print(f"SIZE {a.size} vs {b.size}")
    print(f"RATIO 1.0")
    sys.exit(0)
ia, ib = a.tobytes(), b.tobytes()
n = a.size[0] * a.size[1]
diff = 0
tol = 16 * 3  # 每像素三通道合计容差
px = 0
for i in range(0, len(ia), 3):
    d = abs(ia[i] - ib[i]) + abs(ia[i+1] - ib[i+1]) + abs(ia[i+2] - ib[i+2])
    if d > tol:
        diff += 1
print(f"SIZE {a.size}")
print(f"RATIO {diff / n:.4f}")
'@
            $py | python -X utf8 - $base $cur | Tee-Object -Variable pyout | Out-Null
            $ratioLine = ($pyout | Select-String "^RATIO ").Line
            $sizeLine = ($pyout | Select-String "^SIZE ").Line
            $ratio = 1.0
            if ($ratioLine) { $ratio = [double]($ratioLine -replace "^RATIO ", "") }
            Remove-Item $cur -ErrorAction SilentlyContinue
            $status = "OK"
            if ($ratio -ge 0.02) { $status = "DIFF" }
            [void]$report.Add(@{
                Name = $sc.Name; Scale = $s; Status = $status; Ratio = $ratio
                Note = "$sizeLine vs 基线"
            })
        }
    }
    Write-Host ""
    Write-Host "--- 比对报告(阈值 2%,超阈值**只报告**)---" -ForegroundColor Cyan
    foreach ($r in $report) {
        $color = switch ($r.Status) { "OK" { "Green" } "DIFF" { "Yellow" } default { "Red" } }
        Write-Host ("  [{0}] scale {1:P0} {2}  差异 {3:P2}  {4}" -f $r.Status, $r.Scale, $r.Name, [Math]::Max($r.Ratio, 0), $r.Note) -ForegroundColor $color
    }
    $bad = @($report | Where-Object { $_.Status -ne "OK" })
    if ($bad.Count -gt 0) {
        Write-Host ("  {0} 项差异/缺失 —— 报告模式不拦截;确认UI改动后请 -Generate 重制基线。" -f $bad.Count) -ForegroundColor Yellow
    } else {
        Write-Host "  全部与基线一致。" -ForegroundColor Green
    }
}

if (-not $Generate -and -not $Compare) {
    Write-Host "用法:tools/ui_shots.ps1 -Generate(产基线)/ -Compare(比对,只报告)"
    Write-Host "示例:powershell -File tools/ui_shots.ps1 -Generate"
}
Write-Host ""
