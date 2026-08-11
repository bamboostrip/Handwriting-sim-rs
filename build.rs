// 编译 .slint 界面描述为 Rust 代码。
// 相关 .slint 文件变更后自动触发重编译。
fn main() {
    // 内嵌所有资源（窗口图标等）到二进制，发布时无需附带资源文件。
    // 用 EmbedFiles（PNG 原样内嵌、运行时解码）而非 EmbedForSoftwareRenderer：
    // 后者仅支持软件渲染器，且会预嵌入字形导致 CJK 大字号坐标溢出 panic
    let config = slint_build::CompilerConfiguration::new()
        .embed_resources(slint_build::EmbedResourcesKind::EmbedFiles);
    slint_build::compile_with_config("src/ui/main_window.slint", config)
        .expect("编译 main_window.slint 失败");
    println!("cargo:rerun-if-changed=src/ui/main_window.slint");
    // main_window.slint 通过 import 引用的主题文件，变更同样需要重编译
    println!("cargo:rerun-if-changed=src/ui/theme.slint");
    println!("cargo:rerun-if-changed=src/ui/app-icon.png");

    // Windows 下把多尺寸 ICO 嵌入 exe 资源（资源管理器/任务栏图标）；
    // Linux/macOS 无此机制，跳过（对齐 Python 版 PyInstaller 的做法）
    #[cfg(target_os = "windows")]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/app-icon.ico");
        res.compile().expect("嵌入 exe 图标失败");
    }
}
