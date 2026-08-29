use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_else(|_| "unknown".to_string());
    if target_os != "windows" {
        println!(
            "cargo:warning=skipping Windows VST3 bundle emission on non-Windows target ({target_os})"
        );
        return;
    }

    let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());
    let target_dir = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set"))
                .join("target")
        });
    let version = env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.1.0".to_string());
    let output_root = if profile == "release" {
        PathBuf::from("C:/dist")
    } else {
        target_dir.join(profile)
    };
    let output_path = output_root.join(format!("Chromascope-v{version}-win.vst3"));

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).unwrap_or_else(|error| {
            panic!(
                "failed to create VST3 output directory {}: {error}",
                parent.display()
            )
        });
    }

    println!("cargo:rustc-cdylib-link-arg=/OUT:{}", output_path.display());
    println!(
        "cargo:warning=writing VST3 binary to {}",
        output_path.display()
    );
}
