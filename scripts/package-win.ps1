# 本地 Windows 一键打包：构建 release → 组装便携目录（exe + 背景 + 预设 + 字体）→ 打成 zip。
#
# 用法:
#   pwsh scripts/package-win.ps1                              # 默认打包（自动探测字体目录）
#   pwsh scripts/package-win.ps1 -FontsDir D:\my\fonts         # 指定字体目录
#   pwsh scripts/package-win.ps1 -SkipBuild                    # 跳过 cargo build（用现有 release exe）
#   pwsh scripts/package-win.ps1 -OutDir D:\dist               # 指定 zip 输出目录
#
# 说明:
#   - 字体目录优先级: -FontsDir 参数 > 本地测试目录 D:\code\手写模拟\fonts > 仓库 fonts/
#     （字体仅打包进本地 zip 供自用测试，GitHub Actions 发布包不含字体，见 packaging/fonts-README.txt）
#   - zip 命名与 CI 发布产物一致: handwrite-sim-windows-x86_64.zip

param(
    [string]$FontsDir = "",
    [string]$OutDir = "",
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$Exe = Join-Path $Root "target\release\handwrite-sim.exe"
$Staging = Join-Path $env:TEMP ("hs-staging-" + [guid]::NewGuid().ToString("N"))
$ZipName = "handwrite-sim-windows-x86_64.zip"

# ---- 1. release 构建（Tauri CLI：先构建前端再编译，嵌入 WebView 资源） ----
if (-not $SkipBuild) {
    Write-Host "[1/4] pnpm tauri build --no-bundle ..."
    Push-Location $Root
    try {
        pnpm tauri build --no-bundle
        if ($LASTEXITCODE -ne 0) { throw "tauri build 失败（exit $LASTEXITCODE）" }
    } finally {
        Pop-Location
    }
} else {
    Write-Host "[1/4] 跳过构建（-SkipBuild），使用现有 release exe"
}
if (-not (Test-Path $Exe)) {
    throw "未找到 $Exe，请先构建或去掉 -SkipBuild"
}

# ---- 2. 组装便携目录 ----
Write-Host "[2/4] 组装便携目录 ..."
New-Item -ItemType Directory -Force "$Staging\fonts", "$Staging\backgrounds", "$Staging\presets" | Out-Null
Copy-Item $Exe "$Staging\handwrite-sim.exe"
Copy-Item "$Root\backgrounds\*" "$Staging\backgrounds\" -Recurse
Copy-Item "$Root\presets\*" "$Staging\presets\" -Recurse
Copy-Item "$Root\packaging\fonts-README.txt" "$Staging\fonts\README.txt"
# PDF 底图栅格化运行时依赖（本地有就带上；CI 发布包不含，见 README）
if (Test-Path "$Root\pdfium.dll") {
    Copy-Item "$Root\pdfium.dll" "$Staging\"
    Write-Host "      已附带 pdfium.dll（PDF 文档底图导入用）"
}

# ---- 3. 拷贝字体（本地自用；自动探测目录） ----
$fontCandidates = @($FontsDir, "D:\code\手写模拟\fonts", (Join-Path $Root "fonts"))
$fontsSrc = $fontCandidates | Where-Object { -not [string]::IsNullOrWhiteSpace($_) -and (Test-Path $_) } | Select-Object -First 1
if ($fontsSrc) {
    $fontFiles = Get-ChildItem $fontsSrc -File | Where-Object { $_.Extension -in ".ttf", ".ttc", ".otf" }
    if ($fontFiles.Count -gt 0) {
        Copy-Item $fontFiles.FullName "$Staging\fonts\"
        Write-Host "      字体: $($fontFiles.Count) 个（来自 $fontsSrc）"
    } else {
        Write-Host "      [警告] $fontsSrc 下没有 .ttf/.ttc/.otf 字体文件，zip 不含字体"
    }
} else {
    Write-Host "      [警告] 未找到字体目录（可用 -FontsDir 指定），zip 不含字体"
}

# ---- 4. 打 zip ----
Write-Host "[4/4] 打包 zip ..."
if (-not $OutDir) { $OutDir = $Root }
New-Item -ItemType Directory -Force $OutDir | Out-Null
$ZipPath = Join-Path $OutDir $ZipName
if (Test-Path $ZipPath) { Remove-Item $ZipPath -Force }
Compress-Archive -Path "$Staging\*" -DestinationPath $ZipPath -CompressionLevel Optimal

Remove-Item $Staging -Recurse -Force
$size = [math]::Round((Get-Item $ZipPath).Length / 1MB, 1)
Write-Host ""
Write-Host "打包完成: $ZipPath ($size MB)"
Write-Host "结构: handwrite-sim.exe + backgrounds/ + presets/ + fonts/（含字体）"
