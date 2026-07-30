use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

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
    println!("cargo:rerun-if-env-changed=ANDROID_API_LEVEL");
    println!("cargo:rerun-if-env-changed=ANDROID_NDK_HOME");
    println!("cargo:rerun-if-env-changed=ANDROID_NDK_ROOT");
    println!("cargo:rerun-if-changed=src/renderer/ohos_native_buffer.vert");
    println!("cargo:rerun-if-changed=src/renderer/ohos_native_buffer.frag");
    println!("cargo:rerun-if-changed=src/renderer/ohos_native_buffer.vert.spv");
    println!("cargo:rerun-if-changed=src/renderer/ohos_native_buffer.frag.spv");

    let ffmpeg_version_major = emit_ffmpeg_version_cfg();
    enforce_bundled_ffmpeg_version(ffmpeg_version_major);

    let target_os = env::var("CARGO_CFG_TARGET_OS").ok();
    if target_os.as_deref() == Some("ios") {
        println!("cargo:rustc-link-lib=framework=AudioToolbox");
    } else if target_os.as_deref() == Some("android") {
        for lib in [
            "android",
            "log",
            "dl",
            "m",
            "atomic",
            "aaudio",
            "camera2ndk",
            "mediandk",
        ] {
            println!("cargo:rustc-link-lib={lib}");
        }
        // Erika always includes C++ code through bundled SoundTouch, and the
        // libass/HarfBuzz path adds more C++ when that feature is enabled.
        // Keep the Android runtime dependency independent of Cargo features.
        println!("cargo:rustc-link-lib=dylib=c++_shared");
        if env::var_os("CARGO_FEATURE_WGPU").is_some() {
            compile_android_vulkan_shaders();
        }
    }

    if env::var("CARGO_FEATURE_LIBASS").is_err() {
        return;
    }

    println!("cargo:rerun-if-changed=src/libass_log_bridge.c");
    cc::Build::new()
        .file("src/libass_log_bridge.c")
        .warnings(true)
        .compile("erika_libass_log_bridge");

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

    if target_os.as_deref() == Some("android") {
        for (dir, archive) in [
            (&libass, "libass.a"),
            (&fribidi, "libfribidi.a"),
            (&harfbuzz, "libharfbuzz.a"),
            (&freetype, "libfreetype.a"),
        ] {
            let archive = dir.join("lib").join(archive);
            println!("cargo:rerun-if-changed={}", archive.display());
            if !archive.is_file() {
                panic!(
                    "Android native dependency archive was not found at {}. Run `cargo run -p xtask -- deps build --all --profile {} --target {}` first.",
                    archive.display(),
                    native_profile(),
                    inferred_native_target().unwrap_or_else(|| "android-target".to_string())
                );
            }
        }
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

    if target_os.as_deref() == Some("windows") {
        println!("cargo:rustc-link-lib=dwrite");
    } else if matches!(target_os.as_deref(), Some("ios" | "macos")) {
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

fn compile_android_vulkan_shaders() {
    let shaders = [
        (
            "src/renderer/android_ahb.vert",
            "android_ahb.vert.spv",
            "vertex",
        ),
        (
            "src/renderer/android_ahb.frag",
            "android_ahb.frag.spv",
            "fragment",
        ),
    ];
    let glslc = android_glslc();
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo did not set OUT_DIR"));
    for (source, output, stage) in shaders {
        println!("cargo:rerun-if-changed={source}");
        let output = out_dir.join(output);
        let status = Command::new(&glslc)
            .arg(format!("-fshader-stage={stage}"))
            .arg("--target-env=vulkan1.0")
            .arg("-O")
            .arg(source)
            .arg("-o")
            .arg(&output)
            .status()
            .unwrap_or_else(|error| {
                panic!(
                    "failed to launch Android NDK glslc at {}: {error}",
                    glslc.display()
                )
            });
        if !status.success() {
            panic!("Android Vulkan shader compilation failed for {source} with {status}");
        }
    }
}

fn android_glslc() -> PathBuf {
    let ndk = env::var_os("ANDROID_NDK_HOME")
        .or_else(|| env::var_os("ANDROID_NDK_ROOT"))
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            panic!(
                "ANDROID_NDK_HOME or ANDROID_NDK_ROOT is required to compile Android Vulkan shaders"
            )
        });
    let host_tag = match (env::consts::OS, env::consts::ARCH) {
        ("windows", _) => "windows-x86_64",
        ("linux", "aarch64") => "linux-aarch64",
        ("linux", _) => "linux-x86_64",
        ("macos", "aarch64") => "darwin-arm64",
        ("macos", _) => "darwin-x86_64",
        (os, arch) => panic!("unsupported Android shader compiler host {os}/{arch}"),
    };
    let executable = if cfg!(windows) { "glslc.exe" } else { "glslc" };
    let path = ndk.join("shader-tools").join(host_tag).join(executable);
    if !path.is_file() {
        panic!(
            "Android NDK glslc was not found at {}; install a complete side-by-side NDK",
            path.display()
        );
    }
    path
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

fn enforce_bundled_ffmpeg_version(version_major: Option<u32>) {
    let target_os = env::var("CARGO_CFG_TARGET_OS").ok();
    if !matches!(target_os.as_deref(), Some("windows" | "android")) {
        return;
    }
    if env::var("ERIKA_ALLOW_LEGACY_FFMPEG").as_deref() == Ok("1") {
        return;
    }
    if matches!(version_major, Some(major) if major >= 60) {
        return;
    }
    panic!(
        "{} native core requires Erika's FFmpeg 8.x dependency bundle (libavutil >= 60), but found {:?}. Run `cargo run -p xtask -- deps build --profile {} --target {}` or set ERIKA_FFMPEG_DIR to that dist.",
        target_os.as_deref().unwrap_or("target"),
        version_major,
        native_profile(),
        inferred_native_target().unwrap_or_else(|| "host".to_string())
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
    if let Some(target) = inferred_native_target() {
        dist = dist.join(target);
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
    if let Some(target) = inferred_native_target() {
        dist = dist.join(target);
    }
    dist.join(native_profile()).join(name)
}

fn native_profile() -> String {
    env::var("ERIKA_NATIVE_PROFILE").unwrap_or_else(|_| "lgpl".to_string())
}

fn inferred_native_target() -> Option<String> {
    let os = env::var("CARGO_CFG_TARGET_OS").ok()?;
    let arch = env::var("CARGO_CFG_TARGET_ARCH").ok()?;
    match (os.as_str(), arch.as_str()) {
        ("windows", "x86_64") => Some("x86_64-pc-windows-msvc".to_string()),
        ("windows", "aarch64") => Some("aarch64-pc-windows-msvc".to_string()),
        ("android", "aarch64") => Some("aarch64-linux-android".to_string()),
        ("android", "arm") => Some("armv7-linux-androideabi".to_string()),
        ("android", "x86_64") => Some("x86_64-linux-android".to_string()),
        ("android", "x86") => Some("i686-linux-android".to_string()),
        ("ios", _) => Some("ios".to_string()),
        _ => None,
    }
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set"))
        .parent()
        .and_then(|path| path.parent())
        .expect("crates/erika has a workspace root")
        .to_path_buf()
}
