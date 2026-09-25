# 一键质量门禁(设计文档 10 篇;14 篇 §7.1 补齐门禁 8/9;阶段 6 分档)
#
# 门禁清单(本脚本覆盖):
#   1  格式与 Lint        fmt --check
#   2  静态检查           clippy --workspace --all-targets -- -D warnings
#   3  测试               cargo test --workspace
#                        ├ 往返语料 20 个(L0 不损坏 + L1 幂等)
#                        ├ 命令全变体 apply/undo/redo 可逆性
#                        ├ 门禁 9:快捷键注册表自检(冲突/菜单加速键一致/文本态守卫)
#                        └ 06-4:台账 Planned=0 收口门禁(capabilities.rs)
#   4  Agent 自检         vellum-cli selfcheck(无头,不需要 GPU/浏览器)
#   6  术语禁用词扫描      check_terminology.py(i18n/ 中英两套)
#   7  输出校验           validate:示例工程全检(良构 + sid 唯一 + CSS 合法 + L1 幂等)
#   8  硬编码颜色扫描      棘轮:只允许减少,不允许增加(见脚本内 -Max)
#   9  示例体检           check_examples.py:tree 可读 + 基线 + 覆叠 + 漂位(W9)
#   10 三端一致性         CPU 光栅 vs SVG 像素容差(15 号计划 B1)
#   11 画布↔导出一致性    canvas_parity.ps1:画板矩形硬判据 + 像素软分数(仅报告)
#   12 UI 截图基线比对    ui_shots.ps1(报告模式,可选档)
#   13 巨型文件扫描       >25KB 软指标,报告模式不拦截(06-1-6)
#   14 许可口径一致性     check_license.py:四处口径 ACL-1.0 + 历史许可声明残留扫描(06-2)
#
# 分档(06-4-5;重门禁耗 GPU/窗口系统,CI 无头机跑轻量档):
#   -Light  轻量档:1/2/3/4/6/7/8/9/10/13/14 —— 无头机可跑,几分钟级
#   默认    全量档:轻量档 + 11(canvas_parity,可 -SkipParity 单跳)+ 12(可 -UiShots 开启)
#
# 用法:
#   powershell -File tools/ci.ps1                 # 全量(含画布↔导出对拍)
#   powershell -File tools/ci.ps1 -Light          # 轻量档(无头 CI / 快速循环)
#   powershell -File tools/ci.ps1 -SkipClippy     # 跳过 clippy(快速循环)
#   powershell -File tools/ci.ps1 -UiShots        # 附带 UI 截图基线比对(04-7,重门禁/本地 gate)
param(
    [switch]$Light,
    [switch]$SkipFmt,
    [switch]$SkipClippy,
    [switch]$SkipParity,
    [switch]$UiShots,
    # 06-4 棘轮基线:阶段 6 实测存量 35 处(5-C..5-F 批次遗留:breakpoints/
    # commands/dock_layout/plugins/timeline/toolbar/slice/import_svg/pathfinder)。
    # 只许减不许增 —— 新增任何一处即超标;P2 目标仍是清零(逐批降 -ColorMax)。
    [int]$ColorMax = 35
)
$ErrorActionPreference = "Continue"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

$failures = New-Object System.Collections.ArrayList
$sw = [System.Diagnostics.Stopwatch]::StartNew()

# 失败补救提示(06-4-6):每条门禁给出**可执行**的下一步命令,沿用
# 「提示里带命令」的项目风格;定位类提示指向会输出违例明细的同一脚本。
$Remedy = @{
    "1 格式"           = "cargo fmt --all"
    "2 Lint"           = "cargo clippy --workspace --all-targets -- -D warnings  # 按输出逐条修"
    "3 测试"           = "cargo test --workspace  # 按首个失败用例定位;台账 Planned=0 属 capabilities.rs 门禁"
    "4 Agent 自检"     = "cargo run -q -p vb_agent --bin vellum-cli -- selfcheck"
    "6 术语扫描"       = "python -X utf8 tools/check_terminology.py  # 按违例行改词"
    "7 输出校验"       = "vellum-cli --doc <示例目录> validate  # 按报告修文档(17 种 patch op)"
    "8 硬编码颜色"     = "tools/check_no_hardcoded_color.ps1  # 按清单改设计令牌(vb_ui/theme)"
    "9 示例体检"       = "python -X utf8 tools/check_examples.py  # 修示例或经评审后重建基线"
    "10 三端一致性"    = "cargo test -p vb_export --test three_backend"
    "11 画布导出一致性" = "tools/canvas_parity.ps1  # 画板几何不一致,查 app/canvas.rs 装配"
    "14 许可口径"      = "python -X utf8 tools/check_license.py  # 统一 ACL-1.0(ADR-0030)或加白名单注明理由"
}

function Write-GateHeader {
    param([string]$Name)
    Write-Host ""
    Write-Host ("--- " + $Name) -ForegroundColor Cyan
}

function Mark-Gate {
    param([string]$Name, [int]$Code)
    if ($Code -ne 0) {
        Write-Host ("  [FAIL] " + $Name) -ForegroundColor Red
        $hint = $Remedy[$Name]
        if ($hint) { Write-Host ("         补救: " + $hint) -ForegroundColor Yellow }
        [void]$failures.Add($Name)
    } else {
        Write-Host ("  [PASS] " + $Name) -ForegroundColor Green
    }
}

Write-Host "=== Vellum Bench 质量门禁 ===" -ForegroundColor White
Write-Host ("根目录:" + $Root)
if ($Light) {
    Write-Host "分档:轻量(-Light;无头 CI)—— 跳过门禁 11/12(需 GPU 与窗口系统)" -ForegroundColor DarkCyan
} else {
    Write-Host "分档:全量(默认)—— 门禁 11 画布对拍执行,-UiShots 可再加 12" -ForegroundColor DarkCyan
}

# ---- 门禁 1:格式 ----
if (-not $SkipFmt) {
    Write-GateHeader "门禁 1 · 格式(fmt)"
    cargo fmt --all --check
    Mark-Gate "1 格式" $LASTEXITCODE
} else {
    Write-Host "`n--- 门禁 1 · 格式(已跳过)" -ForegroundColor DarkGray
}

# ---- 门禁 2:Lint ----
if (-not $SkipClippy) {
    Write-GateHeader "门禁 2 · Lint(clippy -D warnings)"
    cargo clippy --workspace --all-targets -- -D warnings
    Mark-Gate "2 Lint" $LASTEXITCODE
} else {
    Write-Host "`n--- 门禁 2 · Lint(已跳过)" -ForegroundColor DarkGray
}

# ---- 门禁 3:测试(含语料、命令可逆性、门禁 9、Planned=0 收口门禁) ----
Write-GateHeader "门禁 3 · 测试(含往返语料 + 命令可逆性 + 快捷键自检 + 台账 Planned=0)"
cargo test --workspace
Mark-Gate "3 测试" $LASTEXITCODE

# ---- 门禁 4:Agent 自检(无头) ----
Write-GateHeader "门禁 4 · Agent 自检(selfcheck)"
cargo run -q -p vb_agent --bin vellum-cli -- selfcheck --json
Mark-Gate "4 Agent 自检" $LASTEXITCODE

# ---- 门禁 6:术语禁用词扫描(01 篇 §七;i18n/ 未建立时自动 PASS) ----
Write-GateHeader "门禁 6 · 术语禁用词扫描"
python -X utf8 tools/check_terminology.py
Mark-Gate "6 术语扫描" $LASTEXITCODE

# ---- 门禁 7:输出校验(良构 + sid 唯一 + CSS 合法 + L1 幂等) ----
Write-GateHeader "门禁 7 · 输出校验(validate:示例工程全检)"
$cli = 'target/debug/vellum-cli.exe'
if (-not (Test-Path $cli)) { $cli = 'target/release/vellum-cli.exe' }
cargo build --bin vellum-cli 2>&1 | Out-Null
& $cli --doc examples/landing/index.html validate
Mark-Gate "7 输出校验" $LASTEXITCODE

# ---- 门禁 8:硬编码颜色(棘轮) ----
Write-GateHeader "门禁 8 · 硬编码颜色扫描"
& "$PSScriptRoot\check_no_hardcoded_color.ps1" -Max $ColorMax
Mark-Gate "8 硬编码颜色" $LASTEXITCODE

# ---- 门禁 9:示例体检(W9,P0-④ 防复发) ----
Write-GateHeader "门禁 9 · 示例体检(tree 可读 + 基线 + 覆叠 + 漂位)"
python -X utf8 tools/check_examples.py --cli $cli
Mark-Gate "9 示例体检" $LASTEXITCODE

# ---- 门禁 10:三端一致性(15 号计划 B1) ----
# 同一文档 → CPU 光栅 PNG vs SVG(resvg 参考栅格化)像素级容差比对;
# 「所见即所得」的结构级偏差(渐变错位/缺失填充/双重缩放)在此变红。
Write-GateHeader "门禁 10 · 三端一致性(CPU vs SVG 像素容差)"
cargo test -q -p vb_export --test three_backend
Mark-Gate "10 三端一致性" $LASTEXITCODE

# ---- 门禁 11:画布 ↔ 导出 一致性(W7,P0-③ 防复发;重门禁) ----
# 真实画布 GPU 纹理读回 vs CPU 导出,画板矩形硬判据;像素分数只报告。
# 需要 GPU(vello)与窗口系统:全量档默认执行;无头机走 -Light。
if (-not $Light -and -not $SkipParity) {
    Write-GateHeader "门禁 11 · 画布↔导出一致性(canvas_parity;重门禁,可 -SkipParity)"
    & "$PSScriptRoot\canvas_parity.ps1"
    Mark-Gate "11 画布导出一致性" $LASTEXITCODE
} else {
    Write-Host "`n--- 门禁 11 · 画布↔导出一致性(已跳过;-Light 分档或 -SkipParity)" -ForegroundColor DarkGray
}

# ---- 门禁 12:UI 截图基线比对(04-7;可选档,重门禁/本地 gate) ----
# 需要窗口系统与 GPU,CI 无头机默认跳过。当前为**报告模式**(差异只列清单
# 不拦截),阈值拦截留后续按基线稳定度定。
if ($UiShots) {
    Write-GateHeader "门禁 12 · UI 截图基线比对(ui_shots;报告模式,重门禁/本地 gate)"
    & "$PSScriptRoot\ui_shots.ps1" -Compare
    # 报告模式:无论差异多少都不进 failures(不拦截);缺基线时提示先生成
    Write-Host "  [PASS] 12 UI截图基线(报告模式,不拦截)" -ForegroundColor Green
} else {
    Write-Host "`n--- 门禁 12 · UI 截图基线比对(已跳过;本地可 -UiShots 开启)" -ForegroundColor DarkGray
}

# ---- 门禁 13:巨型文件扫描(06-1-6;25KB 软指标,报告模式不拦截) ----
# 副文档 06 §3:单文件 ≤ 25KB 为软指标 —— 按职责拆,不按行数机械切。
# 只输出警告清单(棘轮式的"看得见"),超限文件拆分与否由迭代批次裁量。
Write-GateHeader "门禁 13 · 巨型文件扫描(>25KB 软指标;报告模式,不拦截)"
$bigFiles = Get-ChildItem "$Root\crates" -Recurse -Filter "*.rs" |
    Where-Object { $_.FullName -notmatch '\\target\\' -and $_.Length -gt 25KB } |
    Sort-Object Length -Descending
if ($bigFiles.Count -gt 0) {
    Write-Host ("  {0} 个源文件超过 25KB 软指标(新改动请按职责拆分):" -f $bigFiles.Count) -ForegroundColor Yellow
    foreach ($f in $bigFiles) {
        Write-Host ("    {0,8:N0} B  {1}" -f $f.Length, $f.FullName.Substring($Root.Length + 1)) -ForegroundColor Yellow
    }
} else {
    Write-Host "  无超过 25KB 的源文件" -ForegroundColor Green
}
Write-Host "  [PASS] 13 巨型文件扫描(报告模式,不拦截)" -ForegroundColor Green

# ---- 门禁 14:许可口径一致性(06-2;轻量档也跑 —— 纯文本扫描零依赖) ----
# 四处口径(README / LICENSE / Cargo.toml / dist 打包说明)必须同为 ACL-1.0,
# 且"活文档"不得再出现本仓的历史许可声明(第三方依赖记述在白名单目录)。
Write-GateHeader "门禁 14 · 许可口径一致性(check_license;ACL-1.0 四处一致 + 历史许可声明残留扫描)"
python -X utf8 tools/check_license.py
Mark-Gate "14 许可口径" $LASTEXITCODE

# ---- 汇总 ----
$sw.Stop()
Write-Host ""
Write-Host "=== 汇总 ===" -ForegroundColor White
Write-Host ("  耗时:{0:N1}s" -f $sw.Elapsed.TotalSeconds)
if ($failures.Count -eq 0) {
    Write-Host "  全部通过" -ForegroundColor Green
    exit 0
}
Write-Host ("  失败 " + $failures.Count + " 项:") -ForegroundColor Red
foreach ($f in $failures) {
    Write-Host ("    - " + $f) -ForegroundColor Red
    $hint = $Remedy[$f]
    if ($hint) { Write-Host ("      补救: " + $hint) -ForegroundColor Yellow }
}
exit 1
