use std::env;
use std::path::Path;
use std::process::Command;

fn main() {
    // Only build Swift bridge on macOS
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "macos" {
        return;
    }

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let out_dir = env::var("OUT_DIR").unwrap();
    let macos_dir = Path::new(&manifest_dir).join("src").join("macos");

    // Swift source file
    let swift_source = macos_dir.join("VMBridge.swift");

    // Output paths
    let swift_obj = Path::new(&out_dir).join("VMBridge.o");
    let swift_header = Path::new(&out_dir).join("VMBridge-Swift.h");

    // Get SDK path
    let sdk_output = Command::new("xcrun")
        .args(["--show-sdk-path"])
        .output()
        .expect("Failed to get SDK path");
    let sdk_path = String::from_utf8_lossy(&sdk_output.stdout)
        .trim()
        .to_string();

    // Compile Swift to object file
    let swift_status = Command::new("swiftc")
        .args([
            "-c",
            swift_source.to_str().unwrap(),
            "-o",
            swift_obj.to_str().unwrap(),
            "-emit-objc-header",
            "-emit-objc-header-path",
            swift_header.to_str().unwrap(),
            "-sdk",
            &sdk_path,
            "-target",
            "arm64-apple-macosx12.0",
            "-O",
        ])
        .status()
        .expect("Failed to run swiftc");

    if !swift_status.success() {
        panic!("Swift compilation failed");
    }

    // Create static library from object file
    let lib_path = Path::new(&out_dir).join("libvmbridge.a");
    let ar_status = Command::new("ar")
        .args([
            "rcs",
            lib_path.to_str().unwrap(),
            swift_obj.to_str().unwrap(),
        ])
        .status()
        .expect("Failed to run ar");

    if !ar_status.success() {
        panic!("Failed to create static library");
    }

    // Tell Cargo to link the library
    println!("cargo:rustc-link-search=native={}", out_dir);
    println!("cargo:rustc-link-lib=static=vmbridge");

    // Link required frameworks
    println!("cargo:rustc-link-lib=framework=Foundation");
    println!("cargo:rustc-link-lib=framework=Virtualization");

    // Link Swift runtime libraries
    // Get Swift library path
    let swift_lib_output = Command::new("xcrun")
        .args(["--toolchain", "default", "--find", "swift"])
        .output()
        .expect("Failed to find swift");
    let swift_path = String::from_utf8_lossy(&swift_lib_output.stdout)
        .trim()
        .to_string();
    if let Some(toolchain_dir) = Path::new(&swift_path).parent().and_then(|p| p.parent()) {
        let swift_lib_dir = toolchain_dir.join("lib").join("swift").join("macosx");
        if swift_lib_dir.exists() {
            println!("cargo:rustc-link-search=native={}", swift_lib_dir.display());
        }
    }

    // Rebuild if source files change
    println!("cargo:rerun-if-changed={}", swift_source.display());
    println!(
        "cargo:rerun-if-changed={}",
        macos_dir.join("VMBridge.h").display()
    );

    println!("cargo:warning=Swift VM bridge compiled successfully");
}
