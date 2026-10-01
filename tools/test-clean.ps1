# target/debug 测试产物清理(2026-10-02)
#
# 背景:cargo 从不回收旧哈希工件——每轮改代码重跑测试,deps/ 里都会留下
# 上一版测试 exe/pdb(单个可达数百 MB),incremental/ 缓存只增不减,
# 长迭代下来 target/debug 动辄几十 GB 塞爆磁盘(C 盘满曾致 cargo 级联损坏,
# 见 docs/ 与记忆 devbox-build-env-quirks)。Cargo.toml 的 profile.dev debug=1
# 只能减缓增速,治标;本脚本治本:每轮测试后收一次。
#
# 用法:powershell -ExecutionPolicy Bypass -File tools/test-clean.ps1 [-Full]
#   常规:删 incremental + deps/examples 里旧哈希的 exe/pdb(保留每组最新)
#   -Full:整个 target/debug 清掉(下次全量重编,磁盘紧张时用)
# 常规每轮 cargo test 后跑一次;tools/test.ps1 已自动带这一步。
param(
    [switch]$Full
)

$Root = Split-Path -Parent $PSScriptRoot
$Dbg = Join-Path $Root "target\debug"

function Get-DirSize([string]$Path) {
    if (-not (Test-Path $Path)) { return 0 }
    (Get-ChildItem $Path -Recurse -File -ErrorAction SilentlyContinue |
        Measure-Object Length -Sum).Sum
}

function Format-MB([double]$Bytes) { "{0:N0} MB" -f ($Bytes / 1MB) }

if (-not (Test-Path $Dbg)) {
    Write-Host "target/debug 不存在,无需清理"
    exit 0
}

$Before = Get-DirSize $Dbg

if ($Full) {
    Remove-Item -Recurse -Force $Dbg
    Write-Host "已整目录清除 target/debug(回收 $(Format-MB $Before));下次构建将全量重编"
    exit 0
}

# 1) incremental 编译缓存:重建免费,直接删
$Inc = Join-Path $Dbg "incremental"
if (Test-Path $Inc) {
    Remove-Item -Recurse -Force $Inc
    Write-Host "已删 incremental 编译缓存"
}

# 2) deps / examples 里旧哈希的测试二进制:按主干分组,只留最新一组 exe/pdb。
#    cargo 对“文件丢失”的反应是按指纹重编该单元,不会报错,所以删旧的永远安全;
#    删当前指纹指向的那份才会触发重编,而我们留的就是最新(mtime 最大)那组。
foreach ($Dir in @((Join-Path $Dbg "deps"), (Join-Path $Dbg "examples"))) {
    if (-not (Test-Path $Dir)) { continue }
    $Bins = Get-ChildItem $Dir -File -Include *.exe,*.pdb -Recurse -ErrorAction SilentlyContinue
    if (-not $Bins) { continue }
    # 测试二进制名形如 vb_kiln-1a2b3c4d5e6f7890.exe;主干 = 末尾 16 位哈希前的部分
    $Groups = $Bins | ForEach-Object {
        if ($_.BaseName -match '^(?<stem>.+)-[0-9a-f]{16}$') {
            [PSCustomObject]@{ File = $_; Stem = $Matches['stem'] }
        }
    } | Group-Object Stem | Where-Object Count -gt 1
    $Removed = 0
    foreach ($G in $Groups) {
        # 按扩展名分组各留最新(exe 与 pdb 的 mtime 一致,分别处理防错杀)
        foreach ($Ext in @('.exe', '.pdb')) {
            $G.Group | Where-Object { $_.File.Extension -eq $Ext } |
                Sort-Object { $_.File.LastWriteTimeUtc } -Descending |
                Select-Object -Skip 1 |
                ForEach-Object {
                    Remove-Item -Force $_.File.FullName -ErrorAction SilentlyContinue
                    $Removed++
                }
        }
    }
    Write-Host "${Dir}: 清掉 $Removed 个旧哈希二进制"
}

# 3) %TEMP% 里 kiln-* 残留(超 24h 的;kiln 自身 sweep 白名单见 vb_browser/browser.rs)
$KilnTemp = Get-ChildItem $env:TEMP -Directory -Filter "kiln-*" -ErrorAction SilentlyContinue |
    Where-Object { $_.LastWriteTime -lt (Get-Date).AddHours(-24) }
foreach ($D in $KilnTemp) {
    Remove-Item -Recurse -Force $D.FullName -ErrorAction SilentlyContinue
}
if ($KilnTemp) { Write-Host "已清 $($KilnTemp.Count) 个超期 kiln-* 临时目录" }

# 4) 尺寸守卫:清完仍超 8GB,说明 rlib/工件历史太厚,整目录重来比继续挤划算
$After = Get-DirSize $Dbg
if ($After -gt 8GB) {
    Write-Warning "target/debug 仍占 $(Format-MB $After)(>8GB),建议 -Full 整清后全量重编"
}

Write-Host ("清理完成:{0} -> {1}(回收 {2})" -f `
    (Format-MB $Before), (Format-MB $After), (Format-MB ($Before - $After)))
