# 从源 PNG 生成窗口图标（src/ui/app-icon.png）与多尺寸 exe 图标（assets/app-icon.ico）。
# 用法: pwsh scripts/make-icon.ps1 -Source <源PNG>
#   - 窗口图标 256x256 -> src/ui/app-icon.png（Slint @image-url 引用，需与 .slint 同目录）
#   - exe 图标 16/24/32/48/64/128/256 -> assets/app-icon.ico（build.rs 中 winresource 嵌入）
# 依赖: ffmpeg（缩放）+ PowerShell（组装 ICO 容器）

param(
    [Parameter(Mandatory = $true)]
    [string]$Source,
    [string]$Ffmpeg = "D:\AllCode\pj\ffmpeg_temp\ffmpeg-8.1.2-essentials_build\bin\ffmpeg.exe"
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$WindowPng = Join-Path $Root "src\ui\app-icon.png"
$IconIco = Join-Path $Root "assets\app-icon.ico"
$tmp = Join-Path $env:TEMP "hs-icon-sizes"

New-Item -ItemType Directory -Path $tmp -Force | Out-Null

# 1. 窗口图标（256x256）
& $Ffmpeg -y -loglevel error -i $Source -vf "scale=256:256:flags=lanczos" $WindowPng

# 2. exe 图标各尺寸 PNG
foreach ($s in 16, 24, 32, 48, 64, 128, 256) {
    & $Ffmpeg -y -loglevel error -i $Source -vf "scale=$($s):$($s):flags=lanczos" "$tmp\icon-$s.png"
}

# 3. 组装 ICO（PNG 条目，Windows Vista+ 支持）
$sizes = 16, 24, 32, 48, 64, 128, 256
$pngs = foreach ($s in $sizes) {
    , [System.IO.File]::ReadAllBytes((Join-Path $tmp "icon-$s.png"))
}

$ms = New-Object System.IO.MemoryStream
$bw = New-Object System.IO.BinaryWriter($ms)

$bw.Write([uint16]0)   # reserved
$bw.Write([uint16]1)   # type: icon
$bw.Write([uint16]$sizes.Count)

$offset = 6 + 16 * $sizes.Count
$index = 0
foreach ($p in $pngs) {
    $dim = if ($sizes[$index] -ge 256) { 0 } else { $sizes[$index] }
    $bw.Write([byte]$dim)   # width (0 = 256)
    $bw.Write([byte]$dim)   # height
    $bw.Write([byte]0)      # color count
    $bw.Write([byte]0)      # reserved
    $bw.Write([uint16]1)    # planes
    $bw.Write([uint16]32)   # bit count
    $bw.Write([uint32]$p.Length)
    $bw.Write([uint32]$offset)
    $offset += $p.Length
    $index++
}
foreach ($p in $pngs) { $bw.Write($p) }
$bw.Flush()
[System.IO.File]::WriteAllBytes($IconIco, $ms.ToArray())
$bw.Dispose(); $ms.Dispose()

Write-Host "窗口图标: $WindowPng"
Write-Host "exe 图标: $IconIco ($((Get-Item $IconIco).Length) bytes)"
