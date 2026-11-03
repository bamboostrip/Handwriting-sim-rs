use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// pdfium-render 0.9 的默认绑定集（特性 `pdfium_latest`）对应 pdfium build 8125，
/// 该构建新增了导出符号 `FPDF_StructElement_GetRawType`。内置库若早于该构建，
/// 运行期 `GetProcAddress` 会返回 127，导入 PDF 时直接报「pdfium 加载失败」。
///
/// 这里在构建期做一次符号探测并告警（导出名以 ASCII 字符串存在于 DLL 的导出名表，
/// 直接按字节搜索即可，无需解析 PE）。刻意只告警不中断：部分构建依赖系统库或
/// 外部库，不应因此硬失败。
const REQUIRED_PDFIUM_SYMBOL: &str = "FPDF_StructElement_GetRawType";

/// 字节子串查找（`[u8]` 没有内置的 contains，用 windows 线性扫描；
/// 目标文件约 7 MB，构建期开销可忽略）。
fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    needle.len() <= haystack.len() && haystack.windows(needle.len()).any(|w| w == needle)
}

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let out_path = Path::new(&out_dir).join("embedded_pdfium.bin");
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    println!("cargo:rerun-if-changed=build.rs");

    let mut found_source: Option<PathBuf> = None;

    let filename = match target_os.as_str() {
        "windows" => "pdfium.dll",
        "linux" => "libpdfium.so",
        "macos" => "libpdfium.dylib",
        _ => "pdfium.bin",
    };

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let manifest_path = Path::new(&manifest_dir);

    let candidates = [
        manifest_path.join("assets").join(filename),
        manifest_path.join(filename),
        manifest_path.join("../../pdfium.dll"),
        manifest_path.join("../..").join(filename),
    ];

    for candidate in &candidates {
        if candidate.is_file() {
            found_source = Some(candidate.clone());
            break;
        }
    }

    if let Some(src) = found_source {
        println!("cargo:rerun-if-changed={}", src.display());

        // 版本守卫：确认内置库确实带有 0.9 绑定所需的导出符号
        match fs::read(&src) {
            Ok(bytes) => {
                if !contains_bytes(&bytes, REQUIRED_PDFIUM_SYMBOL.as_bytes()) {
                    println!(
                        "cargo:warning=内置 pdfium 库缺少导出符号 {REQUIRED_PDFIUM_SYMBOL}：\
                         {} 很可能早于 pdfium build 8125（pdfium-render 0.9 的默认绑定集）。\
                         该产物导入 PDF 会报 GetProcAddress(127)。请换成 \
                         pdfium >= 158.0.8125.0 后重新构建；换库后务必先执行 \
                         cargo clean -p handwrite-sim，否则 build.rs 的 rerun-if-changed \
                         可能不触发，旧字节会被重新嵌入。",
                        src.display()
                    );
                }
            }
            Err(e) => println!(
                "cargo:warning=无法读取 {} 做 pdfium 版本校验：{e}",
                src.display()
            ),
        }

        let should_copy = match (fs::metadata(&src), fs::metadata(&out_path)) {
            (Ok(src_meta), Ok(out_meta)) => src_meta.len() != out_meta.len(),
            _ => true,
        };

        if should_copy {
            if let Err(e) = fs::copy(&src, &out_path) {
                println!(
                    "cargo:warning=复制 {} 到构建输出失败：{e}（内嵌 pdfium 可能未更新）",
                    src.display()
                );
            }
        }
        println!("cargo:rustc-cfg=has_embedded_pdfium");
        return;
    }

    // 找不到资产时不能静默通过：否则会产出一个「内嵌 0 字节 pdfium」的 exe，
    // 装到用户机器上要等导入 PDF 才暴露问题（CI 会在构建前自动下载，本地需自备）。
    println!(
        "cargo:warning=未找到 {filename}，产物将不内嵌 pdfium，导入 PDF 会失败。\
         已查找：crates/core/assets/{filename}、crates/core/{filename}、仓库根 {filename}。\
         请把 pdfium >= 158.0.8125.0 放到仓库根目录后重新构建\
         （Windows 用 pdfium.dll；Linux/macOS 用 crates/core/assets/ 下的 .so/.dylib；\
         CI 会在构建前自动下载）。"
    );

    if !out_path.exists() {
        let _ = fs::write(&out_path, b"");
    }
}
