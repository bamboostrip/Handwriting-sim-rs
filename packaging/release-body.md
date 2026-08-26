## 下载说明（v0.3.1 起：Windows 新增内置 WebView2 便携版，解决 `Could not find the WebView2 Runtime`）

> **一句话选包**：Windows 报 `WebView2 Runtime` 错误 / 内网离线 / 受限账户 / 精简系统 → 下 `handwrite-sim-windows-x86_64-webview2.zip`（内置 WebView2，解压即用，无需安装）；已有 WebView2（Edge 能打开）且追求轻量 → 下 `handwrite-sim-windows-x86_64.zip`（~10 MB）。

| 文件名 | 平台 | 是否内置 WebView2 | 大小（约） | 适合谁 |
|---|---|---|---|---|
| `handwrite-sim-windows-x86_64.zip` | Windows x64 | ❌ 否（需系统已装 WebView2，Win10/11 通常自带） | ~10 MB | 已有 WebView2、追求轻量/便携，U盘拷贝 |
| `handwrite-sim-windows-x86_64-webview2.zip` | Windows x64 | ✅ 是（**Fixed Runtime 便携**，exe 旁 `WebView2/` 目录，启动时 `WEBVIEW2_BROWSER_EXECUTABLE_FOLDER` 指向它） | ~150 MB | **报错/离线/受限账户/精简系统**：解压即用，无需安装，无需联网，无需管理员 |
| `handwrite-sim-linux-x86_64.zip` | Linux x64 | —（依赖系统 WebKitGTK，非 WebView2） | ~10 MB | Linux 用户 |
| `handwrite-sim-macos-arm64.zip` | macOS Apple Silicon | —（系统 WKWebView） | ~10 MB | macOS 用户 |

> 整体 4 文件：原 3 便携（win/linux/macos）保留，v0.3.1 为 Windows **额外新增 1 个便携免安装的 WebView2 版**。Linux/macOS 不使用 WebView2，无需额外打包。小工具免安装，暂不提供 NSIS/MSI 安装包。

### Windows 该下哪个？

- **报 `Could not find the WebView2 Runtime` / `on another user account` / 内网离线 / 非管理员 / 另一个用户账户不可见** → `handwrite-sim-windows-x86_64-webview2.zip`：解压后 `handwrite-sim.exe` + `WebView2/` + `presets/` + `backgrounds/` + `fonts/`，直接运行 exe 即可。`src-tauri/src/main.rs:283` 启动前探测 `WebView2/msedgewebview2.exe` 并设 `WEBVIEW2_BROWSER_EXECUTABLE_FOLDER`，完全离线、跨用户可用，整个文件夹可 U 盘拷贝。约 150 MB 为 Fixed Runtime 固定开销（`128.0.2739.54.x64`）。
- **已有 WebView2（Edge 能正常打开）且想要轻量** → `handwrite-sim-windows-x86_64.zip`，~10 MB，解压即用。
- **不确定** → 优先 `*-webview2.zip`（最稳妥，免安装）。

### 为什么 Windows 会报 WebView2 错误？

Tauri 2 底层依赖系统 **Microsoft Edge WebView2 Runtime**（Win10 1809+ / Win11 通常预装，但精简系统、LTSC、企业镜像、跨用户隔离等场景可能缺失）。v0.3.0 仅提供轻量便携，未内置运行时，故弹窗：

```
Could not find the WebView2 Runtime
Make sure it is installed or download it from
https://developer.microsoft.com/en-us/microsoft-edge/webview2
You may have it installed on another user account, but it is not available for this one
```

**v0.3.1 修复**：`src-tauri/src/main.rs` 新增 `init_webview2_fixed_runtime()`，CI 新增 `portable-webview2` 任务下载 `Microsoft.Web.WebView2.FixedVersionRuntime.128.0.2739.54`（nuget.org，约 180 MB 解压）并与 exe 打成 `*-webview2.zip`，便携免安装。

### 便携 zip 使用说明

解压后即可使用：可执行文件 + `presets/` + `backgrounds/` + `fonts/` + （webview2 版额外 `WebView2/`）。
把 exe 放进任意目录，同目录下的资源目录即为便携根目录，整个文件夹可随意拷贝移动。webview2 版删除 `WebView2/` 即退化为轻量版（需系统 WebView2）。

## 为什么没有字体？

手写体字体多为商业版权字体（如汉呈、华阳、云江等字库），开源分发需要授权，
因此本仓库与发布包均**不包含字体文件**。

请自备字体放入 `fonts/` 目录，或从开源 / 免费可商用字体中下载：
- 霞鹜文楷（LXGW WenKai）：OFL 1.1 协议，可商用，最接近手写体观感
- 沐瑶随心手写体 / 站酷小薇 / 站酷快乐体：免费可商用

预设中的字体路径（如 `fonts/云烟体.ttf`）为占位名称，
放入对应字体文件后即可直接使用，也可以改用其他字体。

## Linux 运行依赖

Tauri 2 界面需要 WebKitGTK 4.1 运行时库（主流发行版通常已自带）：
```bash
sudo apt install libwebkit2gtk-4.1-0 libayatana-appindicator3-1 librsvg2-2
```
