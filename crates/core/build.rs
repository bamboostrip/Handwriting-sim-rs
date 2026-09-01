use std::env;
use std::fs;
use std::path::{Path, PathBuf};

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
        let should_copy = match (fs::metadata(&src), fs::metadata(&out_path)) {
            (Ok(src_meta), Ok(out_meta)) => src_meta.len() != out_meta.len(),
            _ => true,
        };

        if should_copy {
            let _ = fs::copy(&src, &out_path);
        }
        println!("cargo:rustc-cfg=has_embedded_pdfium");
        return;
    }

    if !out_path.exists() {
        let _ = fs::write(&out_path, b"");
    }
}

