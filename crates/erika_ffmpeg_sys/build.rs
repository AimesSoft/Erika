use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=wrapper.h");
    println!("cargo:rerun-if-env-changed=ERIKA_NATIVE_PROFILE");
    println!("cargo:rerun-if-env-changed=ERIKA_NATIVE_TARGET");
    println!("cargo:rerun-if-env-changed=ERIKA_FFMPEG_DIR");
    println!("cargo:rerun-if-env-changed=ERIKA_ZLIB_DIR");
    println!("cargo:rerun-if-env-changed=ERIKA_ALLOW_LEGACY_FFMPEG");
    println!("cargo:rerun-if-env-changed=LIBCLANG_PATH");

    let dist_dir = ffmpeg_dist_dir();
    let zlib_dir = native_dep_dir("ERIKA_ZLIB_DIR", "zlib");
    let include_dir = dist_dir.join("include");
    let lib_dir = dist_dir.join("lib");

    if !include_dir.join("libavformat/avformat.h").exists() {
        panic!(
            "FFmpeg headers were not found at {}. Run `{}` first, or set ERIKA_FFMPEG_DIR.",
            include_dir.display(),
            xtask_build_hint()
        );
    }

    let ffmpeg_version_major = emit_ffmpeg_version_cfg(&include_dir);
    enforce_windows_ffmpeg_version(ffmpeg_version_major, &include_dir);

    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!(
        "cargo:rustc-link-search=native={}",
        zlib_dir.join("lib").display()
    );
    println!("cargo:rustc-link-lib=static=avdevice");
    println!("cargo:rustc-link-lib=static=avfilter");
    println!("cargo:rustc-link-lib=static=avformat");
    println!("cargo:rustc-link-lib=static=avcodec");
    println!("cargo:rustc-link-lib=static=swresample");
    println!("cargo:rustc-link-lib=static=swscale");
    println!("cargo:rustc-link-lib=static=avutil");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-lib=static=zlib");
    } else {
        println!("cargo:rustc-link-lib=static=z");
    }

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
    } else if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        for lib in [
            "bcrypt",
            "d3d11",
            "dxgi",
            "dxguid",
            "gdi32",
            "mf",
            "mfplat",
            "mfuuid",
            "mfreadwrite",
            "ole32",
            "secur32",
            "strmiids",
            "user32",
            "uuid",
            "ws2_32",
        ] {
            println!("cargo:rustc-link-lib={lib}");
        }
    }

    ensure_libclang_path();

    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .clang_arg(format!("-I{}", include_dir.display()))
        .allowlist_function("av_.*")
        .allowlist_function("avio_.*")
        .allowlist_function("avcodec_.*")
        .allowlist_function("avsubtitle_.*")
        .allowlist_function("avformat_.*")
        .allowlist_function("swr_.*")
        .allowlist_type("AV.*")
        .allowlist_type("Swr.*")
        .allowlist_var("AV.*")
        .allowlist_var("FF_.*")
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

fn emit_ffmpeg_version_cfg(include_dir: &Path) -> Option<u32> {
    println!("cargo:rustc-check-cfg=cfg(erika_ffmpeg_legacy_channel_layout)");
    let version_header = include_dir.join("libavutil/version.h");
    println!("cargo:rerun-if-changed={}", version_header.display());
    let Ok(contents) = fs::read_to_string(&version_header) else {
        return None;
    };
    let major = contents.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        match (parts.next(), parts.next(), parts.next()) {
            (Some("#define"), Some("LIBAVUTIL_VERSION_MAJOR"), Some(value)) => {
                value.parse::<u32>().ok()
            }
            _ => None,
        }
    });
    if matches!(major, Some(value) if value < 57) {
        println!("cargo:rustc-cfg=erika_ffmpeg_legacy_channel_layout");
    }
    major
}

fn enforce_windows_ffmpeg_version(version_major: Option<u32>, include_dir: &Path) {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    if env::var("ERIKA_ALLOW_LEGACY_FFMPEG").as_deref() == Ok("1") {
        return;
    }
    if matches!(version_major, Some(major) if major >= 59) {
        return;
    }
    panic!(
        "Windows native core requires Erika's FFmpeg 7.x dependency bundle (libavutil >= 59), but found {:?} under {}. Run `{}` or set ERIKA_FFMPEG_DIR to that dist; set ERIKA_ALLOW_LEGACY_FFMPEG=1 only for local compatibility experiments.",
        version_major,
        include_dir.display(),
        xtask_build_hint()
    );
}

fn ensure_libclang_path() {
    if env::var_os("LIBCLANG_PATH").is_some() {
        return;
    }
    for path in [
        Path::new("C:/msys64/mingw64/bin"),
        Path::new("C:/Program Files/LLVM/bin"),
    ] {
        if path.join("libclang.dll").exists() {
            // Build scripts are single-process setup code; set this before bindgen
            // loads libclang so Windows source builds work without a developer shell.
            unsafe {
                env::set_var("LIBCLANG_PATH", path);
            }
            prepend_path_for_dlls(path);
            if path.starts_with("C:/msys64") {
                prepend_path_for_dlls(Path::new("C:/msys64/usr/bin"));
            }
            return;
        }
    }
}

fn prepend_path_for_dlls(path: &Path) {
    if !path.exists() {
        return;
    }
    let mut paths = vec![path.to_path_buf()];
    if let Some(current) = env::var_os("PATH") {
        paths.extend(env::split_paths(&current));
    }
    if let Ok(joined) = env::join_paths(paths) {
        unsafe {
            env::set_var("PATH", joined);
        }
    }
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
    } else if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_else(|_| "x86_64".to_string());
        dist = dist.join(format!("{arch}-pc-windows-msvc"));
    }
    dist.join(native_profile()).join("ffmpeg")
}

fn native_dep_dir(env_name: &str, name: &str) -> PathBuf {
    if let Ok(path) = env::var(env_name) {
        return PathBuf::from(path);
    }
    if let Ok(target) = env::var("ERIKA_NATIVE_TARGET") {
        return workspace_root()
            .join("third_party/dist")
            .join(target)
            .join(native_profile())
            .join(name);
    }
    let mut dist = workspace_root().join("third_party/dist");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("ios") {
        dist = dist.join("ios");
    } else if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_else(|_| "x86_64".to_string());
        dist = dist.join(format!("{arch}-pc-windows-msvc"));
    }
    dist.join(native_profile()).join(name)
}

fn native_profile() -> String {
    env::var("ERIKA_NATIVE_PROFILE").unwrap_or_else(|_| "lgpl".to_string())
}

fn xtask_build_hint() -> String {
    let profile = native_profile();
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_else(|_| "x86_64".to_string());
        format!(
            "cargo run -p xtask -- deps build --profile {profile} --target {arch}-pc-windows-msvc"
        )
    } else {
        format!("cargo run -p xtask -- deps build --profile {profile}")
    }
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
