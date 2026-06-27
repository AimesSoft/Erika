use std::env;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=wrapper.h");
    println!("cargo:rerun-if-env-changed=ERIKA_NATIVE_PROFILE");
    println!("cargo:rerun-if-env-changed=ERIKA_NATIVE_TARGET");
    println!("cargo:rerun-if-env-changed=ERIKA_FFMPEG_DIR");

    let dist_dir = ffmpeg_dist_dir();
    let include_dir = dist_dir.join("include");
    let lib_dir = dist_dir.join("lib");

    if !include_dir.join("libavformat/avformat.h").exists() {
        panic!(
            "FFmpeg headers were not found at {}. Run `cargo run -p xtask -- deps build --profile {}` first, or set ERIKA_FFMPEG_DIR.",
            include_dir.display(),
            native_profile()
        );
    }

    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=static=avdevice");
    println!("cargo:rustc-link-lib=static=avfilter");
    println!("cargo:rustc-link-lib=static=avformat");
    println!("cargo:rustc-link-lib=static=avcodec");
    println!("cargo:rustc-link-lib=static=swresample");
    println!("cargo:rustc-link-lib=static=swscale");
    println!("cargo:rustc-link-lib=static=avutil");

    if matches!(
        env::var("CARGO_CFG_TARGET_OS").as_deref(),
        Ok("macos" | "ios")
    ) {
        println!("cargo:rustc-link-lib=framework=CoreFoundation");
        println!("cargo:rustc-link-lib=framework=CoreMedia");
        println!("cargo:rustc-link-lib=framework=CoreVideo");
        println!("cargo:rustc-link-lib=framework=VideoToolbox");
        println!("cargo:rustc-link-lib=iconv");
        println!("cargo:rustc-link-lib=bz2");
        println!("cargo:rustc-link-lib=z");
    }

    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .clang_arg(format!("-I{}", include_dir.display()))
        .allowlist_function("av_.*")
        .allowlist_function("avio_.*")
        .allowlist_function("avcodec_.*")
        .allowlist_function("avsubtitle_.*")
        .allowlist_function("avformat_.*")
        .allowlist_function("swr_.*")
        .allowlist_function("sws_.*")
        .allowlist_type("AV.*")
        .allowlist_type("Swr.*")
        .allowlist_type("Sws.*")
        .allowlist_var("AV.*")
        .allowlist_var("FF_.*")
        .allowlist_var("SWS_.*")
        .allowlist_var("AVERROR.*")
        .blocklist_item("FP_.*")
        .generate_comments(false)
        .derive_debug(true)
        .derive_default(true)
        .generate()
        .expect("generate FFmpeg bindings");

    let out_path = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is set"));
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("write FFmpeg bindings");
}

fn ffmpeg_dist_dir() -> PathBuf {
    if let Ok(path) = env::var("ERIKA_FFMPEG_DIR") {
        return PathBuf::from(path);
    }
    if let Ok(target) = env::var("ERIKA_NATIVE_TARGET") {
        return workspace_root()
            .join("third_party/dist")
            .join(target)
            .join(native_profile())
            .join("ffmpeg");
    }
    let mut dist = workspace_root().join("third_party/dist");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("ios") {
        dist = dist.join("ios");
    }
    dist.join(native_profile()).join("ffmpeg")
}

fn native_profile() -> String {
    env::var("ERIKA_NATIVE_PROFILE").unwrap_or_else(|_| "lgpl".to_string())
}

fn workspace_root() -> PathBuf {
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set"));
    manifest_dir
        .ancestors()
        .nth(2)
        .map(Path::to_path_buf)
        .expect("crate lives under workspace/crates/name")
}
