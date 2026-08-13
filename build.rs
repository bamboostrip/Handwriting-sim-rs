// Windows 下把多尺寸 ICO 嵌入 exe 资源（资源管理器/任务栏图标）；
// Linux/macOS 无此机制，跳过（对齐 Python 版 PyInstaller 的做法）
fn main() {
    #[cfg(target_os = "windows")]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/app-icon.ico");
        res.compile().expect("嵌入 exe 图标失败");
    }
}
