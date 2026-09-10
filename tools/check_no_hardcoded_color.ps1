# 硬编码颜色扫描(设计文档 03 篇 §九 验收项;14 篇 §7.1 门禁 8)
#
# 目标:UI 颜色只允许来自设计令牌(`vb_ui::theme` / `vb-ui-tokens.json`),
#       代码里不得出现字面量颜色。
#
# P1 建立基线(允许存量违规,只报数,超标即失败);
# P2 目标:把 -Max 降到 0。
#
# 用法:
#   powershell -File tools/check_no_hardcoded_color.ps1            # 用默认基线
#   powershell -File tools/check_no_hardcoded_color.ps1 -List      # 列出全部违规行
#   powershell -File tools/check_no_hardcoded_color.ps1 -Max 0     # P2 清零模式
#
# 白名单:
#   * `crates/vb_common/src/color.rs` —— 颜色解析器本身(内含 #rrggbb 测试数据)
#   * 违规行本身或**上一行**带 `// vb-token-ok` —— 语义色(如智能参考线品红 #FF00FF)
param(
    [int]$Max = 0,
    [switch]$List,
    [string]$Root = (Split-Path -Parent $PSScriptRoot)
)

Set-Location $Root

# 文件级白名单:相对路径 -> 放行理由
$AllowedFiles = @{
    "crates\vb_common\src\color.rs" = "颜色解析器本身(测试数据含 #rrggbb)"
    "crates\vb_ui\src\theme.rs"     = "设计令牌唯一源(所有颜色的合法居所,P2.1)"
}

# 行级白名单标记:写在违规行本身,或写在**紧邻的上一行**(等价于 eslint-disable-next-line)
$LineMarker = "vb-token-ok"

# 只扫 src(测试代码里的颜色属于测试数据,不算 UI 硬编码)
$files = Get-ChildItem -Path "crates" -Recurse -Filter *.rs -File |
    Where-Object { $_.FullName -match "\\src\\" }

$hits = New-Object System.Collections.ArrayList
$rel = { param($p) $p.Replace("$Root\", "") }

foreach ($f in $files) {
    $key = & $rel $f.FullName
    if ($AllowedFiles.ContainsKey($key)) { continue }

    $lineNo = 0
    $prevLine = ""
    foreach ($line in Get-Content -LiteralPath $f.FullName -Encoding UTF8) {
        $lineNo++
        $skip = $line.Contains($LineMarker) -or $prevLine.Contains($LineMarker)
        $prevLine = $line
        if ($skip) { continue }

        # `Color32::from_rgb(0x..)` / `from_rgb(12, ...)` / `from_rgba*(...)` / 字面量十六进制色
        $hit = $false
        $kind = ""
        if ($line -match "Color32::from_rgb\s*\(\s*(0x|\d)") { $hit = $true; $kind = "from_rgb 字面量" }
        elseif ($line -match "Color32::from_rgba(_unmultiplied)?\s*\(\s*\d") { $hit = $true; $kind = "from_rgba 字面量" }
        elseif ($line -match '"#[0-9a-fA-F]{3,8}"') { $hit = $true; $kind = "十六进制色字符串" }
        elseif ($line -match "egui::Color32::from_rgb\s*\(\s*(0x|\d)") { $hit = $true; $kind = "from_rgb 字面量" }

        if ($hit) {
            [void]$hits.Add([pscustomobject]@{
                File  = $key
                Line  = $lineNo
                Kind  = $kind
                Text  = $line.Trim()
            })
        }
    }
}

Write-Host ""
Write-Host "=== 硬编码颜色扫描(门禁 8)===" -ForegroundColor Cyan
Write-Host "  扫描范围:crates\*\src\**\*.rs"
Write-Host "  白名单文件:$(($AllowedFiles.Keys | Sort-Object) -join ', ')"
Write-Host "  白名单标记:$LineMarker"
Write-Host ""

if ($hits.Count -eq 0) {
    Write-Host "  ✓ 零违规" -ForegroundColor Green
} else {
    $byFile = $hits | Group-Object File | Sort-Object Name
    foreach ($g in $byFile) {
        Write-Host ("  {0}  ({1} 处)" -f $g.Name, $g.Count) -ForegroundColor Yellow
        if ($List) {
            foreach ($h in ($g.Group | Sort-Object Line)) {
                Write-Host ("      L{0,-5} [{1}] {2}" -f $h.Line, $h.Kind, $h.Text) -ForegroundColor DarkGray
            }
        }
    }
    Write-Host ""
    Write-Host ("  违规合计:{0} 处(预算 {1})" -f $hits.Count, $Max) -ForegroundColor Yellow
    if ($hits.Count -gt $Max) {
        Write-Host "  ✗ 超出预算 —— 新增了硬编码颜色,请改用设计令牌" -ForegroundColor Red
        Write-Host "    (P2 目标:把 -Max 降到 0;令牌见 docs/design/assets/vb-ui-tokens.json)" -ForegroundColor DarkGray
        exit 1
    }
    Write-Host "  ✓ 未超出预算(存量待 P2 清除)" -ForegroundColor Green
}

exit 0
