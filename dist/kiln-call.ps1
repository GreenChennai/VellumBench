# Kiln-noGUI-CLI 调用封装(供 artboard 技能/任意脚本使用)
# 用法: .\kiln-call.ps1 -Source <html|dir> -Output <file> [-Format PNG|JPG|GIF|MP4|SVG|PDF|EPS|AI|PPTX] [-Scale 2] [-Transparent]
param(
    [Parameter(Mandatory)][string]$Source,
    [Parameter(Mandatory)][string]$Output,
    [string]$Format,
    [int]$Scale = 2,
    [switch]$Transparent,
    [int]$Fps = 25,
    [double]$Duration = 2.0
)
$exe = Join-Path $PSScriptRoot "Kiln-noGUI-CLI.exe"
if (-not (Test-Path $exe)) { Write-Error "未找到 $exe"; exit 1 }
$args = @("export", "--source", $Source, "--output", $Output, "--scale", $Scale, "--fps", $Fps, "--duration", $Duration)
if ($Format) { $args += @("--format", $Format) }
if ($Transparent) { $args += "--transparent" }
& $exe @args
exit $LASTEXITCODE
