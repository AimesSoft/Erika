use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=ERIKA_NATIVE_PROFILE");
    println!("cargo:rerun-if-env-changed=ERIKA_NATIVE_TARGET");
    println!("cargo:rerun-if-env-changed=ERIKA_FFMPEG_DIR");
    println!("cargo:rerun-if-env-changed=ERIKA_LIBASS_DIR");
    println!("cargo:rerun-if-env-changed=ERIKA_FREETYPE_DIR");
    println!("cargo:rerun-if-env-changed=ERIKA_HARFBUZZ_DIR");
    println!("cargo:rerun-if-env-changed=ERIKA_FRIBIDI_DIR");
    println!("cargo:rerun-if-env-changed=ERIKA_ALLOW_LEGACY_FFMPEG");

    let ffmpeg_version_major = emit_ffmpeg_version_cfg();
    enforce_windows_ffmpeg_version(ffmpeg_version_major);

    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("ios") {
        println!("cargo:rustc-link-lib=framework=AudioToolbox");
    }

    if env::var("CARGO_FEATURE_LIBASS").is_err() {
        return;
    }

    let libass = native_dep_dir("ERIKA_LIBASS_DIR", "libass");
    let freetype = native_dep_dir("ERIKA_FREETYPE_DIR", "freetype");
    let harfbuzz = native_dep_dir("ERIKA_HARFBUZZ_DIR", "harfbuzz");
    let fribidi = native_dep_dir("ERIKA_FRIBIDI_DIR", "fribidi");

    for dir in [&libass, &freetype, &harfbuzz, &fribidi] {
        if !dir.join("lib").exists() {
            panic!(
                "native dependency was not found at {}. Run `cargo run -p xtask -- deps build --all --profile {}` first, or set ERIKA_*_DIR.",
                dir.display(),
                native_profile()
            );
        }
        println!(
            "cargo:rustc-link-search=native={}",
            dir.join("lib").display()
        );
    }

    if !libass.join("include/ass/ass.h").exists() && !libass.join("include/ass.h").exists() {
        panic!(
            "libass headers were not found under {}. Run `cargo run -p xtask -- deps build --all --profile {}` first.",
            libass.display(),
            native_profile()
        );
    }

    println!("cargo:rustc-link-lib=static=ass");
    println!("cargo:rustc-link-lib=static=fribidi");
    println!("cargo:rustc-link-lib=static=harfbuzz");
    println!("cargo:rustc-link-lib=static=freetype");

    let target_os = env::var("CARGO_CFG_TARGET_OS").ok();
    if matches!(target_os.as_deref(), Some("ios" | "macos")) {
        if target_os.as_deref() == Some("macos") {
            println!("cargo:rustc-link-lib=framework=ApplicationServices");
        }
        println!("cargo:rustc-link-lib=framework=CoreText");
        println!("cargo:rustc-link-lib=framework=CoreFoundation");
        println!("cargo:rustc-link-lib=framework=CoreGraphics");
        if target_os.as_deref() == Some("macos") {
            println!("cargo:rustc-link-lib=iconv");
        }
    }
}

fn emit_ffmpeg_version_cfg() -> Option<u32> {
    println!("cargo:rustc-check-cfg=cfg(erika_ffmpeg_legacy_channel_layout)");
    let version_header = ffmpeg_dist_dir().join("include/libavutil/version.h");
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

fn enforce_windows_ffmpeg_version(version_major: Option<u32>) {
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
        "Windows native core requires Erika's FFmpeg 7.x dependency bundle (libavutil >= 59), but found {:?}. Run `cargo run -p xtask -- deps build --profile {} --target x86_64-pc-windows-msvc` or set ERIKA_FFMPEG_DIR to that dist.",
        version_major,
        native_profile()
    );
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

fn workspace_root() -> PathBuf {
    PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set"))
        .parent()
        .and_then(|path| path.parent())
        .expect("crates/erika has a workspace root")
        .to_path_buf()
}
