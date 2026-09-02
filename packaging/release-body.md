## 产物下载

> 点击链接直接下载本版本对应文件，无需翻找页面底部 Assets。

- Windows 便携完整包 ([handwrite-sim-windows-x86_64.zip](https://github.com/bamboostrip/Handwriting-sim-rs/releases/download/{TAG}/handwrite-sim-windows-x86_64.zip))： 首次使用，解压即用（需系统 WebView2，Win10/11 通常自带）
- Windows 单文件升级包 ([handwrite-sim-windows-x86_64.exe](https://github.com/bamboostrip/Handwriting-sim-rs/releases/download/{TAG}/handwrite-sim-windows-x86_64.exe))： 老用户下载后直接替换旧的 `handwrite-sim.exe` 即可
- Windows 离线完整包 ([handwrite-sim-windows-x86_64-webview2.zip](https://github.com/bamboostrip/Handwriting-sim-rs/releases/download/{TAG}/handwrite-sim-windows-x86_64-webview2.zip))： 报 `WebView2 Runtime` 错误 / 内网离线 / 受限账户，内置运行时，解压即用免安装
- Linux 便携完整包 ([handwrite-sim-linux-x86_64.zip](https://github.com/bamboostrip/Handwriting-sim-rs/releases/download/{TAG}/handwrite-sim-linux-x86_64.zip))： 需 WebKitGTK（见下方说明）；单文件升级 [handwrite-sim-linux-x86_64](https://github.com/bamboostrip/Handwriting-sim-rs/releases/download/{TAG}/handwrite-sim-linux-x86_64)，替换旧二进制后 `chmod +x`
- macOS 便携完整包 ([handwrite-sim-macos-arm64.zip](https://github.com/bamboostrip/Handwriting-sim-rs/releases/download/{TAG}/handwrite-sim-macos-arm64.zip))： Apple Silicon（M1–M4）；单文件升级 [handwrite-sim-macos-arm64](https://github.com/bamboostrip/Handwriting-sim-rs/releases/download/{TAG}/handwrite-sim-macos-arm64)，直接替换旧二进制

## 说明

- 便携包解压即用，整个文件夹可随意移动 / U 盘拷贝；离线版删除 `WebView2/` 目录即退化为轻量版（需系统 WebView2）。
- 字体请自备：商业字体版权原因发布包不含字体，下载后放入 `fonts/` 即可，推荐免费可商用的霞鹜文楷（LXGW WenKai）。
- Linux 需 WebKitGTK 运行库：`sudo apt install libwebkit2gtk-4.1-0 libayatana-appindicator3-1 librsvg2-2`
