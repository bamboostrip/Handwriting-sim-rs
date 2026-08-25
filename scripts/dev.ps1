# 开发模式一键启动：Vite HMR + Rust 增量编译 + 桌面窗口。
#
# 用法（仓库根目录）:
#   pwsh scripts/dev.ps1          # 等价于 web\node_modules\.bin\tauri.CMD dev
#
# 说明: tauri CLI 必须从仓库根目录运行才能找到 src-tauri/tauri.conf.json，
#       所以不能写 `pnpm --dir web exec tauri dev`（CLI 只向下查找）。

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot

$Tauri = Join-Path $Root "web\node_modules\.bin\tauri.CMD"
if (-not (Test-Path $Tauri)) {
    Write-Host "未找到 $Tauri，请先安装前端依赖：" -ForegroundColor Yellow
    Write-Host "  pnpm --dir web install"
    exit 1
}

Push-Location $Root
try {
    & $Tauri dev @args
} finally {
    Pop-Location
}
