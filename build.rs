// 编译 .slint 界面描述为 Rust 代码。
// 相关 .slint 文件变更后自动触发重编译。
fn main() {
    slint_build::compile("src/ui/main_window.slint").expect("编译 main_window.slint 失败");
    println!("cargo:rerun-if-changed=src/ui/main_window.slint");
}