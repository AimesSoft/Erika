fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "macos" {
        return;
    }

    println!("cargo:rerun-if-changed=native/DanmakuPerfLab.m");

    cc::Build::new()
        .file("native/DanmakuPerfLab.m")
        .flag("-fobjc-arc")
        .flag("-fmodules")
        .compile("ErikaDanmakuPerfLab");

    println!("cargo:rustc-link-lib=framework=AppKit");
    println!("cargo:rustc-link-lib=framework=QuartzCore");
    println!("cargo:rustc-link-lib=framework=Metal");
}
