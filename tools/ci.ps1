# 一键质量门禁(设计文档 10 篇;14 篇 §7.1 补齐门禁 8/9)
#
# 门禁清单(本脚本覆盖):
#   1  格式与 Lint        fmt --check
#   2  静态检查           clippy --workspace --all-targets -- -D warnings
#   3  测试               cargo test --workspace
#                        ├ 往返语料 20 个(L0 不损坏 + L1 幂等)
#                        ├ 命令全变体 apply/undo/redo 可逆性
#                        └ 门禁 9:快捷键注册表自检(冲突/菜单加速键一致/文本态守卫)
#   4  Agent 自检         vellum-cli selfcheck(无头,不需要 GPU/浏览器)
#   8  硬编码颜色扫描      棘轮:只允许减少,不允许增加(见脚本内 -Max)
#
# 用法:
#   powershell -File tools/ci.ps1                 # 全量
#   powershell -File tools/ci.ps1 -SkipClippy     # 跳过 clippy(快速循环)
param(
    [switch]$SkipFmt,
    [switch]$SkipClippy,
    [int]$ColorMax = 0
)

$ErrorActionPreference = "Continue"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

$failures = New-Object System.Collections.ArrayList
$sw = [System.Diagnostics.Stopwatch]::StartNew()

function Write-GateHeader {
    param([string]$Name)
    Write-Host ""
    Write-Host ("--- " + $Name) -ForegroundColor Cyan
}

function Mark-Gate {
    param([string]$Name, [int]$Code)
    if ($Code -ne 0) {
        Write-Host ("  [FAIL] " + $Name) -ForegroundColor Red
        [void]$failures.Add($Name)
    } else {
        Write-Host ("  [PASS] " + $Name) -ForegroundColor Green
    }
}

Write-Host "=== Vellum Bench 质量门禁 ===" -ForegroundColor White
Write-Host ("根目录:" + $Root)

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

# ---- 门禁 3:测试(含语料、命令可逆性、门禁 9) ----
Write-GateHeader "门禁 3 · 测试(含往返语料 + 命令可逆性 + 门禁 9 快捷键自检)"
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

# ---- 门禁 10:三端一致性(15 号计划 B1) ----
# 同一文档 → CPU 光栅 PNG vs SVG(resvg 参考栅格化)像素级容差比对;
# 「所见即所得」的结构级偏差(渐变错位/缺失填充/双重缩放)在此变红。
Write-GateHeader "门禁 10 · 三端一致性(CPU vs SVG 像素容差)"
cargo test -q -p vb_export --test three_backend
Mark-Gate "10 三端一致性" $LASTEXITCODE

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
foreach ($f in $failures) { Write-Host ("    - " + $f) -ForegroundColor Red }
exit 1
