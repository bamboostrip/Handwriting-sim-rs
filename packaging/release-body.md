## 下载说明

每个平台提供一个**便携 zip**（内含可执行程序 + 预设 + 背景）：
- `handwrite-sim-windows-x86_64.zip`：Windows 版（内含 `handwrite-sim.exe`）
- `handwrite-sim-linux-x86_64.zip`：Linux 版
- `handwrite-sim-macos-arm64.zip`：macOS（Apple Silicon）版

解压后即可使用（无需安装）：可执行文件 + `presets/` 预设 + `backgrounds/` 背景 + `fonts/` 字体目录。
把 exe 放进任意目录，同目录下的资源目录即为便携根目录，整个文件夹可随意拷贝移动。

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
