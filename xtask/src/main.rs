use std::collections::{HashMap, HashSet};
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{Context, Result, bail};

const FFMPEG_VERSION: &str = "8.1.2";
const DAV1D_VERSION: &str = "1.5.1";
const LIBASS_VERSION: &str = "0.17.5";
const HARFBUZZ_VERSION: &str = "14.2.1";
const FREETYPE_VERSION: &str = "2.14.3";
const FRIBIDI_VERSION: &str = "1.0.16";
const ZLIB_VERSION: &str = "1.3.2";
const DEFAULT_ANDROID_API_LEVEL: u32 = 26;

const FFMPEG_ARCHIVE: &str = "ffmpeg-8.1.2.tar.xz";
const FFMPEG_DIR: &str = "ffmpeg-8.1.2";
const FFMPEG_URLS: &[&str] = &["https://ffmpeg.org/releases/ffmpeg-8.1.2.tar.xz"];
const FFMPEG_PATCHSET_VERSION: &str = "erika-android-mediacodec-v3";
const FFMPEG_PATCHES: &[&str] =
    &["third_party/patches/ffmpeg-8.1.2/0001-erika-mediacodec-bounded-receive.patch"];

const DAV1D_ARCHIVE: &str = "dav1d-1.5.1.tar.gz";
const DAV1D_DIR: &str = "dav1d-1.5.1";
const DAV1D_URLS: &[&str] = &[
    "https://code.videolan.org/videolan/dav1d/-/archive/1.5.1/dav1d-1.5.1.tar.gz",
    "https://codeload.github.com/videolan/dav1d/tar.gz/refs/tags/1.5.1",
];

const LIBASS_ARCHIVE: &str = "libass-0.17.5.tar.xz";
const LIBASS_DIR: &str = "libass-0.17.5";
const LIBASS_URLS: &[&str] = &[
    "https://github.com/libass/libass/releases/download/0.17.5/libass-0.17.5.tar.xz",
    "https://codeload.github.com/libass/libass/tar.gz/refs/tags/0.17.5",
];

const HARFBUZZ_ARCHIVE: &str = "harfbuzz-14.2.1.tar.xz";
const HARFBUZZ_DIR: &str = "harfbuzz-14.2.1";
const HARFBUZZ_URLS: &[&str] = &[
    "https://github.com/harfbuzz/harfbuzz/releases/download/14.2.1/harfbuzz-14.2.1.tar.xz",
    "https://codeload.github.com/harfbuzz/harfbuzz/tar.gz/refs/tags/14.2.1",
];

const FREETYPE_ARCHIVE: &str = "freetype-2.14.3.tar.xz";
const FREETYPE_DIR: &str = "freetype-2.14.3";
const FREETYPE_URLS: &[&str] = &[
    "https://download.savannah.gnu.org/releases/freetype/freetype-2.14.3.tar.xz",
    "https://sourceforge.net/projects/freetype/files/freetype2/2.14.3/freetype-2.14.3.tar.xz/download",
];

const FRIBIDI_ARCHIVE: &str = "fribidi-1.0.16.tar.xz";
const FRIBIDI_DIR: &str = "fribidi-1.0.16";
const FRIBIDI_URLS: &[&str] = &[
    "https://github.com/fribidi/fribidi/releases/download/v1.0.16/fribidi-1.0.16.tar.xz",
    "https://codeload.github.com/fribidi/fribidi/tar.gz/refs/tags/v1.0.16",
];

const ZLIB_ARCHIVE: &str = "zlib-1.3.2.tar.gz";
const ZLIB_DIR: &str = "zlib-1.3.2";
const ZLIB_URLS: &[&str] = &[
    "https://zlib.net/zlib-1.3.2.tar.gz",
    "https://github.com/madler/zlib/archive/refs/tags/v1.3.2.tar.gz",
];

fn main() -> Result<()> {
    let mut args = env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        print_help();
        return Ok(());
    }

    match args.remove(0).as_str() {
        "deps" => deps(args),
        "pkg-config-shim" => pkg_config_shim(args),
        "check" => check(args),
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        command => bail!("unknown xtask command: {command}"),
    }
}

fn check(mut args: Vec<String>) -> Result<()> {
    if args.is_empty() {
        bail!("missing check subcommand: license");
    }
    match args.remove(0).as_str() {
        "license" => check_license_policy(),
        other => bail!("unknown check subcommand: {other}"),
    }
}

fn deps(mut args: Vec<String>) -> Result<()> {
    if args.is_empty() {
        bail!("missing deps subcommand: plan, fetch, status, smoke-ffmpeg-make, or build");
    }
    let subcommand = args.remove(0);
    let options = DepsOptions::parse(&args)?;
    match subcommand.as_str() {
        "plan" => {
            print_dependency_plan(options.profile, options.target);
            Ok(())
        }
        "fetch" => {
            print_dependency_plan(options.profile, options.target);
            let layout = workspace_layout(options.profile, options.target)?;
            fetch_dependency_sources(&layout, options.all)?;
            write_profile_metadata(&layout, options.profile, options.target)
        }
        "status" => print_dependency_status(&workspace_layout(options.profile, options.target)?),
        "smoke-ffmpeg-make" => smoke_ffmpeg_make(options),
        "build" => {
            print_dependency_plan(options.profile, options.target);
            build_dependencies(options)
        }
        other => bail!("unknown deps subcommand: {other}"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NativeDependencyProfile {
    Lgpl,
    GplFull,
}

impl NativeDependencyProfile {
    fn ffmpeg_configure_flags(self) -> &'static [&'static str] {
        match self {
            Self::Lgpl => &[
                "--disable-gpl",
                "--enable-version3",
                "--enable-static",
                "--disable-shared",
                "--disable-programs",
                "--disable-doc",
                "--disable-network",
                "--disable-autodetect",
                "--enable-zlib",
                "--enable-protocol=file",
                "--enable-demuxer=mov,matroska,mpegts,mpegps,mpegvideo,avi,flv,h264,hevc,av1,ivf,mp3,aac,flac,wav,ogg,ac3,eac3,dts,truehd,mlp,mjpeg,vc1,ass,srt,webvtt",
                "--enable-parser=hevc,h264,av1,vp9,aac,ac3,dca,mlp,opus,vorbis,flac,mpegaudio,mpegvideo,mpeg4video,mjpeg,vc1,dvdsub,dvbsub",
                "--enable-decoder=hevc,h264,av1,vp8,vp9,mpeg1video,mpeg2video,mpeg4,vc1,mjpeg,flv,theora,aac,ac3,eac3,dca,truehd,mlp,opus,vorbis,flac,mp3,pcm_s16le,pcm_s24le,pcm_s32le,ass,srt,webvtt,pgssub,dvdsub,dvbsub",
            ],
            Self::GplFull => &[
                "--enable-gpl",
                "--enable-version3",
                "--enable-static",
                "--disable-shared",
                "--disable-programs",
                "--disable-doc",
                "--disable-network",
                "--disable-autodetect",
                "--enable-zlib",
                "--enable-protocol=file",
                "--enable-demuxer=mov,matroska,mpegts,mpegps,mpegvideo,avi,flv,h264,hevc,av1,ivf,mp3,aac,flac,wav,ogg,ac3,eac3,dts,truehd,mlp,mjpeg,vc1,ass,srt,webvtt",
                "--enable-parser=hevc,h264,av1,vp9,aac,ac3,dca,mlp,opus,vorbis,flac,mpegaudio,mpegvideo,mpeg4video,mjpeg,vc1,dvdsub,dvbsub",
                "--enable-decoder=hevc,h264,av1,vp8,vp9,mpeg1video,mpeg2video,mpeg4,vc1,mjpeg,flv,theora,aac,ac3,eac3,dca,truehd,mlp,opus,vorbis,flac,mp3,pcm_s16le,pcm_s24le,pcm_s32le,ass,srt,webvtt,pgssub,dvdsub,dvbsub",
            ],
        }
    }

    fn ffmpeg_configure_flags_for_target(self, target: NativeTarget) -> Vec<&'static str> {
        let mut flags = self.ffmpeg_configure_flags().to_vec();
        if target.is_windows() {
            flags.extend(["--enable-d3d11va", "--enable-dxva2"]);
        } else if target.is_android() {
            flags.extend([
                "--enable-jni",
                "--enable-mediacodec",
                "--enable-libdav1d",
                "--enable-decoder=h264_mediacodec,hevc_mediacodec,mpeg2_mediacodec,mpeg4_mediacodec,vp8_mediacodec,vp9_mediacodec,av1_mediacodec,libdav1d",
            ]);
            // FFmpeg's 32-bit external and inline x86 assembly still emits
            // absolute R_386_32 relocations even with CONFIG_PIC enabled.
            // Android has rejected text relocations since API 23, so keep the
            // complete C decoder set while disabling the incompatible asm
            // acceleration paths for this legacy ABI.
            if matches!(target, NativeTarget::I686Android) {
                flags.push("--disable-asm");
            }
        } else if target.is_apple() {
            flags.extend([
                "--enable-videotoolbox",
                "--enable-libdav1d",
                "--enable-decoder=libdav1d",
            ]);
        }
        if target.is_android() {
            assert_android_software_decoder_fallbacks(&flags);
        }
        flags
    }
}

fn assert_android_software_decoder_fallbacks(flags: &[&str]) {
    let enabled = flags
        .iter()
        .filter_map(|flag| flag.strip_prefix("--enable-decoder="))
        .flat_map(|decoders| decoders.split(','))
        .collect::<std::collections::HashSet<_>>();
    for (hardware, software) in [
        ("h264_mediacodec", "h264"),
        ("hevc_mediacodec", "hevc"),
        ("mpeg2_mediacodec", "mpeg2video"),
        ("mpeg4_mediacodec", "mpeg4"),
        ("vp8_mediacodec", "vp8"),
        ("vp9_mediacodec", "vp9"),
        ("av1_mediacodec", "libdav1d"),
    ] {
        assert!(
            !enabled.contains(hardware) || enabled.contains(software),
            "Android FFmpeg enables {hardware} without required software fallback {software}"
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NativeTarget {
    Host,
    Aarch64Macos,
    X86_64Macos,
    Aarch64Ios,
    Aarch64IosSimulator,
    X86_64IosSimulator,
    X86_64WindowsMsvc,
    Aarch64Android,
    Armv7Android,
    X86_64Android,
    I686Android,
}

impl NativeTarget {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "host" => Ok(Self::Host),
            "aarch64-apple-darwin" => Ok(Self::Aarch64Macos),
            "x86_64-apple-darwin" => Ok(Self::X86_64Macos),
            "aarch64-apple-ios" => Ok(Self::Aarch64Ios),
            "aarch64-apple-ios-sim" => Ok(Self::Aarch64IosSimulator),
            "x86_64-apple-ios" => Ok(Self::X86_64IosSimulator),
            "x86_64-pc-windows-msvc" | "windows-x64" => Ok(Self::X86_64WindowsMsvc),
            "aarch64-linux-android" | "arm64-v8a" => Ok(Self::Aarch64Android),
            "armv7-linux-androideabi" | "armeabi-v7a" => Ok(Self::Armv7Android),
            "x86_64-linux-android" | "android-x64" => Ok(Self::X86_64Android),
            "i686-linux-android" | "x86" => Ok(Self::I686Android),
            other => bail!("unknown native target: {other}"),
        }
    }

    fn triple(self) -> Option<&'static str> {
        match self {
            Self::Host => None,
            Self::Aarch64Macos => Some("aarch64-apple-darwin"),
            Self::X86_64Macos => Some("x86_64-apple-darwin"),
            Self::Aarch64Ios => Some("aarch64-apple-ios"),
            Self::Aarch64IosSimulator => Some("aarch64-apple-ios-sim"),
            Self::X86_64IosSimulator => Some("x86_64-apple-ios"),
            Self::X86_64WindowsMsvc => Some("x86_64-pc-windows-msvc"),
            Self::Aarch64Android => Some("aarch64-linux-android"),
            Self::Armv7Android => Some("armv7-linux-androideabi"),
            Self::X86_64Android => Some("x86_64-linux-android"),
            Self::I686Android => Some("i686-linux-android"),
        }
    }

    fn sdk(self) -> Option<&'static str> {
        match self {
            Self::Host => None,
            Self::Aarch64Macos | Self::X86_64Macos => Some("macosx"),
            Self::Aarch64Ios => Some("iphoneos"),
            Self::Aarch64IosSimulator | Self::X86_64IosSimulator => Some("iphonesimulator"),
            Self::X86_64WindowsMsvc
            | Self::Aarch64Android
            | Self::Armv7Android
            | Self::X86_64Android
            | Self::I686Android => None,
        }
    }

    fn ffmpeg_arch(self) -> Option<&'static str> {
        match self {
            Self::Host => None,
            Self::Aarch64Macos | Self::Aarch64Ios | Self::Aarch64IosSimulator => Some("arm64"),
            Self::X86_64Macos | Self::X86_64IosSimulator => Some("x86_64"),
            Self::X86_64WindowsMsvc => Some("x86_64"),
            Self::Aarch64Android => Some("aarch64"),
            Self::Armv7Android => Some("arm"),
            Self::X86_64Android => Some("x86_64"),
            Self::I686Android => Some("x86"),
        }
    }

    fn meson_cpu_family(self) -> Option<&'static str> {
        match self {
            Self::Host => None,
            Self::Aarch64Macos | Self::Aarch64Ios | Self::Aarch64IosSimulator => Some("aarch64"),
            Self::X86_64Macos | Self::X86_64IosSimulator => Some("x86_64"),
            Self::X86_64WindowsMsvc => Some("x86_64"),
            Self::Aarch64Android => Some("aarch64"),
            Self::Armv7Android => Some("arm"),
            Self::X86_64Android => Some("x86_64"),
            Self::I686Android => Some("x86"),
        }
    }

    fn meson_cpu(self) -> Option<&'static str> {
        match self {
            Self::Host => None,
            Self::Aarch64Macos | Self::Aarch64Ios | Self::Aarch64IosSimulator => Some("arm64"),
            Self::X86_64Macos | Self::X86_64IosSimulator => Some("x86_64"),
            Self::X86_64WindowsMsvc => Some("x86_64"),
            Self::Aarch64Android => Some("aarch64"),
            Self::Armv7Android => Some("armv7"),
            Self::X86_64Android => Some("x86_64"),
            Self::I686Android => Some("i686"),
        }
    }

    fn android_abi(self) -> Option<&'static str> {
        match self {
            Self::Aarch64Android => Some("arm64-v8a"),
            Self::Armv7Android => Some("armeabi-v7a"),
            Self::X86_64Android => Some("x86_64"),
            Self::I686Android => Some("x86"),
            _ => None,
        }
    }

    fn android_clang_triple(self) -> Option<&'static str> {
        match self {
            Self::Aarch64Android => Some("aarch64-linux-android"),
            Self::Armv7Android => Some("armv7a-linux-androideabi"),
            Self::X86_64Android => Some("x86_64-linux-android"),
            Self::I686Android => Some("i686-linux-android"),
            _ => None,
        }
    }

    fn is_ios(self) -> bool {
        matches!(
            self,
            Self::Aarch64Ios | Self::Aarch64IosSimulator | Self::X86_64IosSimulator
        )
    }

    fn is_windows(self) -> bool {
        matches!(self, Self::X86_64WindowsMsvc) || (matches!(self, Self::Host) && cfg!(windows))
    }

    fn is_apple(self) -> bool {
        matches!(
            self,
            Self::Aarch64Macos
                | Self::X86_64Macos
                | Self::Aarch64Ios
                | Self::Aarch64IosSimulator
                | Self::X86_64IosSimulator
        ) || (matches!(self, Self::Host) && cfg!(target_vendor = "apple"))
    }

    fn is_android(self) -> bool {
        matches!(
            self,
            Self::Aarch64Android | Self::Armv7Android | Self::X86_64Android | Self::I686Android
        ) || (matches!(self, Self::Host) && cfg!(target_os = "android"))
    }

    fn deployment_target(self) -> Option<(String, &'static str)> {
        match self {
            Self::Host => None,
            Self::Aarch64Macos | Self::X86_64Macos => Some((
                env::var("MACOSX_DEPLOYMENT_TARGET").unwrap_or_else(|_| "11.0".to_string()),
                "-mmacosx-version-min",
            )),
            Self::Aarch64Ios => Some((
                env::var("IPHONEOS_DEPLOYMENT_TARGET").unwrap_or_else(|_| "13.0".to_string()),
                "-miphoneos-version-min",
            )),
            Self::Aarch64IosSimulator | Self::X86_64IosSimulator => Some((
                env::var("IPHONEOS_DEPLOYMENT_TARGET").unwrap_or_else(|_| "13.0".to_string()),
                "-mios-simulator-version-min",
            )),
            Self::X86_64WindowsMsvc
            | Self::Aarch64Android
            | Self::Armv7Android
            | Self::X86_64Android
            | Self::I686Android => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct DepsOptions {
    profile: NativeDependencyProfile,
    target: NativeTarget,
    force: bool,
    all: bool,
    jobs: Option<usize>,
}

impl DepsOptions {
    fn parse(args: &[String]) -> Result<Self> {
        let mut options = Self {
            profile: NativeDependencyProfile::Lgpl,
            target: NativeTarget::Host,
            force: false,
            all: false,
            jobs: None,
        };
        let mut index = 0;
        while index < args.len() {
            match args[index].as_str() {
                "--profile" => {
                    let value = args.get(index + 1).context("--profile requires a value")?;
                    options.profile = match value.as_str() {
                        "lgpl" => NativeDependencyProfile::Lgpl,
                        "gpl-full" => NativeDependencyProfile::GplFull,
                        other => bail!("unknown dependency profile: {other}"),
                    };
                    index += 2;
                }
                "--target" => {
                    let value = args.get(index + 1).context("--target requires a value")?;
                    options.target = NativeTarget::parse(value)?;
                    index += 2;
                }
                "--force" => {
                    options.force = true;
                    index += 1;
                }
                "--all" => {
                    options.all = true;
                    index += 1;
                }
                "--jobs" => {
                    let value = args.get(index + 1).context("--jobs requires a value")?;
                    options.jobs =
                        Some(value.parse().context("--jobs must be a positive integer")?);
                    index += 2;
                }
                other => bail!("unknown deps option: {other}"),
            }
        }
        Ok(options)
    }
}

#[derive(Debug)]
struct WorkspaceLayout {
    root: PathBuf,
    target: NativeTarget,
    cache_dir: PathBuf,
    source_dir: PathBuf,
    build_dir: PathBuf,
    dist_dir: PathBuf,
    ffmpeg_source_dir: PathBuf,
    ffmpeg_build_dir: PathBuf,
    ffmpeg_build_marker: PathBuf,
    ffmpeg_prefix: PathBuf,
    dav1d_source_dir: PathBuf,
    dav1d_build_dir: PathBuf,
    dav1d_build_marker: PathBuf,
    dav1d_prefix: PathBuf,
    libass_source_dir: PathBuf,
    libass_build_dir: PathBuf,
    libass_build_marker: PathBuf,
    libass_prefix: PathBuf,
    harfbuzz_source_dir: PathBuf,
    harfbuzz_build_dir: PathBuf,
    harfbuzz_build_marker: PathBuf,
    harfbuzz_prefix: PathBuf,
    freetype_source_dir: PathBuf,
    freetype_build_dir: PathBuf,
    freetype_build_marker: PathBuf,
    freetype_prefix: PathBuf,
    fribidi_source_dir: PathBuf,
    fribidi_build_dir: PathBuf,
    fribidi_build_marker: PathBuf,
    fribidi_prefix: PathBuf,
    zlib_source_dir: PathBuf,
    zlib_build_dir: PathBuf,
    zlib_build_marker: PathBuf,
    zlib_prefix: PathBuf,
    python_tools_dir: PathBuf,
}

fn workspace_layout(
    profile: NativeDependencyProfile,
    target: NativeTarget,
) -> Result<WorkspaceLayout> {
    let root = workspace_root()?;
    let cache_dir = root.join("third_party/cache");
    let source_dir = root.join("third_party/src");
    let (build_dir, dist_dir) = if let Some(triple) = target.triple() {
        (
            root.join("third_party/build")
                .join(triple)
                .join(profile_name(profile)),
            root.join("third_party/dist")
                .join(triple)
                .join(profile_name(profile)),
        )
    } else {
        (
            root.join("third_party/build").join(profile_name(profile)),
            root.join("third_party/dist").join(profile_name(profile)),
        )
    };
    let ffmpeg_source_dir = source_dir.join(FFMPEG_DIR);
    let ffmpeg_build_dir = build_dir.join("ffmpeg");
    let ffmpeg_build_marker = ffmpeg_build_dir.join("ffmpeg-built.txt");
    let ffmpeg_prefix = dist_dir.join("ffmpeg");
    let dav1d_source_dir = source_dir.join(DAV1D_DIR);
    let dav1d_build_dir = build_dir.join("dav1d");
    let dav1d_build_marker = dav1d_build_dir.join("dav1d-built.txt");
    let dav1d_prefix = dist_dir.join("dav1d");
    let libass_source_dir = source_dir.join(LIBASS_DIR);
    let libass_build_dir = build_dir.join("libass");
    let libass_build_marker = libass_build_dir.join("libass-built.txt");
    let libass_prefix = dist_dir.join("libass");
    let harfbuzz_source_dir = source_dir.join(HARFBUZZ_DIR);
    let harfbuzz_build_dir = build_dir.join("harfbuzz");
    let harfbuzz_build_marker = harfbuzz_build_dir.join("harfbuzz-built.txt");
    let harfbuzz_prefix = dist_dir.join("harfbuzz");
    let freetype_source_dir = source_dir.join(FREETYPE_DIR);
    let freetype_build_dir = build_dir.join("freetype");
    let freetype_build_marker = freetype_build_dir.join("freetype-built.txt");
    let freetype_prefix = dist_dir.join("freetype");
    let fribidi_source_dir = source_dir.join(FRIBIDI_DIR);
    let fribidi_build_dir = build_dir.join("fribidi");
    let fribidi_build_marker = fribidi_build_dir.join("fribidi-built.txt");
    let fribidi_prefix = dist_dir.join("fribidi");
    let zlib_source_dir = source_dir.join(ZLIB_DIR);
    let zlib_build_dir = build_dir.join("zlib");
    let zlib_build_marker = zlib_build_dir.join("zlib-built.txt");
    let zlib_prefix = dist_dir.join("zlib");
    let python_tools_dir = build_dir.join("python-tools");
    Ok(WorkspaceLayout {
        root,
        target,
        cache_dir,
        source_dir,
        build_dir,
        dist_dir,
        ffmpeg_source_dir,
        ffmpeg_build_dir,
        ffmpeg_build_marker,
        ffmpeg_prefix,
        dav1d_source_dir,
        dav1d_build_dir,
        dav1d_build_marker,
        dav1d_prefix,
        libass_source_dir,
        libass_build_dir,
        libass_build_marker,
        libass_prefix,
        harfbuzz_source_dir,
        harfbuzz_build_dir,
        harfbuzz_build_marker,
        harfbuzz_prefix,
        freetype_source_dir,
        freetype_build_dir,
        freetype_build_marker,
        freetype_prefix,
        fribidi_source_dir,
        fribidi_build_dir,
        fribidi_build_marker,
        fribidi_prefix,
        zlib_source_dir,
        zlib_build_dir,
        zlib_build_marker,
        zlib_prefix,
        python_tools_dir,
    })
}

fn print_dependency_plan(profile: NativeDependencyProfile, target: NativeTarget) {
    println!("Erika native dependency plan");
    println!("profile: {}", profile_name(profile));
    println!("target: {}", target.triple().unwrap_or("host"));
    if let Some(abi) = target.android_abi() {
        println!("android ABI: {abi}");
        match android_api_level() {
            Ok(api_level) => println!("android API level: {api_level}"),
            Err(error) => println!("android API level: invalid ({error})"),
        }
    }
    println!("ffmpeg: {FFMPEG_VERSION} ({})", FFMPEG_URLS[0]);
    if target.is_android() {
        println!("ffmpeg patch set: {FFMPEG_PATCHSET_VERSION}");
        println!("dav1d: {DAV1D_VERSION} ({})", DAV1D_URLS[0]);
    }
    println!("libass: {LIBASS_VERSION} ({})", LIBASS_URLS[0]);
    println!("harfbuzz: {HARFBUZZ_VERSION} ({})", HARFBUZZ_URLS[0]);
    println!("freetype: {FREETYPE_VERSION} ({})", FREETYPE_URLS[0]);
    println!("fribidi: {FRIBIDI_VERSION} ({})", FRIBIDI_URLS[0]);
    println!("zlib: {ZLIB_VERSION} ({})", ZLIB_URLS[0]);
    println!("ffmpeg configure flags:");
    for flag in profile.ffmpeg_configure_flags_for_target(target) {
        println!("  {flag}");
    }
    println!(
        "text/subtitle dependencies are source-fetched in v0 and linked when libass rendering lands"
    );
}

fn fetch_dependency_sources(layout: &WorkspaceLayout, all: bool) -> Result<()> {
    fs::create_dir_all(&layout.cache_dir)
        .with_context(|| format!("create {}", layout.cache_dir.display()))?;
    fs::create_dir_all(&layout.source_dir)
        .with_context(|| format!("create {}", layout.source_dir.display()))?;

    fetch_and_extract(layout, FFMPEG_URLS, FFMPEG_ARCHIVE, FFMPEG_DIR)?;
    apply_ffmpeg_patches(layout)?;
    fetch_and_extract(layout, ZLIB_URLS, ZLIB_ARCHIVE, ZLIB_DIR)?;
    if layout.target.is_android() || layout.target.is_apple() {
        fetch_and_extract(layout, DAV1D_URLS, DAV1D_ARCHIVE, DAV1D_DIR)?;
    }
    if all {
        fetch_and_extract(layout, LIBASS_URLS, LIBASS_ARCHIVE, LIBASS_DIR)?;
        fetch_and_extract(layout, HARFBUZZ_URLS, HARFBUZZ_ARCHIVE, HARFBUZZ_DIR)?;
        fetch_and_extract(layout, FREETYPE_URLS, FREETYPE_ARCHIVE, FREETYPE_DIR)?;
        fetch_and_extract(layout, FRIBIDI_URLS, FRIBIDI_ARCHIVE, FRIBIDI_DIR)?;
    } else {
        println!(
            "skip text/subtitle source fetch; pass --all when preparing libass/HarfBuzz/FreeType work"
        );
    }
    Ok(())
}

fn build_dependencies(options: DepsOptions) -> Result<()> {
    let layout = workspace_layout(options.profile, options.target)?;
    ensure_required_tools(options, &layout)?;
    prepare_dependency_dirs(&layout)?;
    fetch_dependency_sources(&layout, options.all)?;
    build_zlib(&layout, options)?;
    if options.target.is_android() || options.target.is_apple() {
        build_dav1d(&layout, options)?;
    }
    build_ffmpeg(&layout, options)?;
    if options.all {
        build_text_dependencies(&layout, options)?;
    }
    write_profile_metadata(&layout, options.profile, options.target)?;
    println!(
        "\nNative dependencies are ready at {}",
        layout.dist_dir.display()
    );
    Ok(())
}

fn print_dependency_status(layout: &WorkspaceLayout) -> Result<()> {
    println!("Erika native dependency status");
    println!("workspace: {}", layout.root.display());
    println!("cache dir: {}", layout.cache_dir.display());
    println!("source dir: {}", layout.source_dir.display());
    println!("dist dir: {}", layout.dist_dir.display());
    println!(
        "ffmpeg source: {}",
        status_word(layout.ffmpeg_source_dir.exists())
    );
    println!(
        "ffmpeg dist: {}",
        status_word(native_static_lib_exists(&layout.ffmpeg_prefix, "avformat"))
    );
    if layout.target.is_android() {
        println!(
            "dav1d source: {}",
            status_word(layout.dav1d_source_dir.exists())
        );
        println!(
            "dav1d dist: {}",
            status_word(native_static_lib_exists(&layout.dav1d_prefix, "dav1d"))
        );
    }
    println!(
        "zlib source: {}",
        status_word(layout.zlib_source_dir.exists())
    );
    println!(
        "zlib dist: {}",
        status_word(
            native_static_lib_exists(&layout.zlib_prefix, "z")
                || native_static_lib_exists(&layout.zlib_prefix, "zlib")
        )
    );
    println!(
        "libass source: {}",
        status_word(layout.libass_source_dir.exists())
    );
    println!(
        "harfbuzz source: {}",
        status_word(layout.harfbuzz_source_dir.exists())
    );
    println!(
        "freetype source: {}",
        status_word(layout.freetype_source_dir.exists())
    );
    println!(
        "fribidi source: {}",
        status_word(layout.fribidi_source_dir.exists())
    );
    println!(
        "freetype dist: {}",
        status_word(native_static_lib_exists(
            &layout.freetype_prefix,
            "freetype"
        ))
    );
    println!(
        "harfbuzz dist: {}",
        status_word(native_static_lib_exists(
            &layout.harfbuzz_prefix,
            "harfbuzz"
        ))
    );
    println!(
        "fribidi dist: {}",
        status_word(native_static_lib_exists(&layout.fribidi_prefix, "fribidi"))
    );
    println!(
        "libass dist: {}",
        status_word(native_static_lib_exists(&layout.libass_prefix, "ass"))
    );
    if layout.dist_dir.join("erika-native-deps.txt").exists() {
        println!(
            "metadata: {}",
            layout.dist_dir.join("erika-native-deps.txt").display()
        );
    } else {
        println!("metadata: missing");
    }
    Ok(())
}

fn prepare_dependency_dirs(layout: &WorkspaceLayout) -> Result<()> {
    fs::create_dir_all(&layout.build_dir)
        .with_context(|| format!("create {}", layout.build_dir.display()))?;
    fs::create_dir_all(&layout.ffmpeg_build_dir)
        .with_context(|| format!("create {}", layout.ffmpeg_build_dir.display()))?;
    fs::create_dir_all(&layout.dist_dir)
        .with_context(|| format!("create {}", layout.dist_dir.display()))?;
    println!("workspace: {}", layout.root.display());
    println!("cache dir: {}", layout.cache_dir.display());
    println!("source dir: {}", layout.source_dir.display());
    println!("build dir: {}", layout.build_dir.display());
    println!("dist dir: {}", layout.dist_dir.display());
    Ok(())
}

fn ensure_required_tools(options: DepsOptions, layout: &WorkspaceLayout) -> Result<()> {
    for tool in ["tar"] {
        if which(tool).is_none() {
            bail!("required build tool `{tool}` was not found in PATH");
        }
    }

    if options.target.is_windows() {
        let _ = windows_msvc_environment()?;
        if posix_shell().is_none() {
            bail!(
                "required POSIX shell was not found; install Git for Windows or MSYS2 so FFmpeg configure can run"
            );
        }
        if gnu_make().is_none() {
            bail!("required GNU make was not found; install MSYS2 make or MinGW mingw32-make");
        }
        if cmake_tool().is_none() {
            bail!("required CMake was not found; install the Visual Studio CMake component");
        }
        if options.all {
            if python_tool().is_none() {
                bail!("required Python with venv support was not found in PATH");
            }
            let _ = ensure_pkg_config_shim(layout)?;
        }
        return Ok(());
    }

    if options.target.is_android() {
        let toolchain = android_toolchain(options.target)?
            .context("an explicit Android target requires an Android NDK toolchain")?;
        println!("Android NDK: {}", toolchain.ndk_root.display());
        println!("Android toolchain: {}", toolchain.bin_dir.display());
        if cfg!(windows) && posix_shell().is_none() {
            bail!(
                "required POSIX shell was not found; install Git for Windows so FFmpeg configure can run"
            );
        }
        if cfg!(windows) {
            let _ = windows_msvc_environment()?;
        }
        if gnu_make().is_none() {
            bail!(
                "required GNU make was not found; install make or use an NDK distribution that includes prebuilt make"
            );
        }
        if matches!(options.target, NativeTarget::X86_64Android)
            && (!ffmpeg_build_marker_is_current(layout, options)
                || !dav1d_build_marker_is_current(layout, options))
            && which("nasm").is_none()
        {
            bail!("required build tool `nasm` was not found for Android x86_64 FFmpeg/dav1d");
        }
        if cmake_tool().is_none() {
            bail!("required CMake was not found; install CMake or Android SDK CMake");
        }
        if python_tool().is_none() && (which("meson").is_none() || which("ninja").is_none()) {
            bail!(
                "required Python with venv support was not found; Android dav1d needs Meson/Ninja and xtask cannot provision them"
            );
        }
        let _ = ensure_meson_tools(layout)?;
        let _ = ensure_pkg_config_shim(layout)?;
        let _ = host_c_compiler()?;
        let _ = host_cxx_compiler()?;
        return Ok(());
    }

    let compiler = "clang";
    for tool in ["make", compiler, "cmake", "pkg-config"] {
        if which(tool).is_none() {
            bail!("required build tool `{tool}` was not found in PATH");
        }
    }
    if python_tool().is_none() {
        bail!("required Python with venv support was not found in PATH");
    }
    Ok(())
}

fn build_text_dependencies(layout: &WorkspaceLayout, options: DepsOptions) -> Result<()> {
    build_freetype(layout, options)?;
    build_harfbuzz(layout, options)?;
    build_fribidi(layout, options)?;
    build_libass(layout, options)?;
    Ok(())
}

fn build_zlib(layout: &WorkspaceLayout, options: DepsOptions) -> Result<()> {
    if layout.zlib_build_marker.exists() && !options.force {
        println!(
            "reuse zlib build marker {}",
            layout.zlib_build_marker.display()
        );
        ensure_windows_link_aliases(
            options.target,
            &layout.zlib_prefix,
            &[("zlibs.lib", "z.lib")],
        )?;
        ensure_windows_zlib_static_alias(options.target, &layout.zlib_prefix)?;
        ensure_windows_zlib_header_compat(options.target, &layout.zlib_prefix)?;
        return Ok(());
    }
    clean_build_and_prefix(options, &layout.zlib_build_dir, &layout.zlib_prefix)?;
    fs::create_dir_all(&layout.zlib_build_dir)
        .with_context(|| format!("create {}", layout.zlib_build_dir.display()))?;
    fs::create_dir_all(&layout.zlib_prefix)
        .with_context(|| format!("create {}", layout.zlib_prefix.display()))?;

    println!("configure zlib");
    let mut configure = cmake_command(options.target)?;
    configure
        .arg("-S")
        .arg(&layout.zlib_source_dir)
        .arg("-B")
        .arg(&layout.zlib_build_dir)
        .arg("-DCMAKE_BUILD_TYPE=Release")
        .arg("-DBUILD_SHARED_LIBS=OFF")
        .arg("-DZLIB_BUILD_EXAMPLES=OFF")
        .arg(format!(
            "-DCMAKE_INSTALL_PREFIX={}",
            layout.zlib_prefix.display()
        ));
    apply_cmake_target(&mut configure, options.target)?;
    run(&mut configure)?;
    cmake_build_install(&layout.zlib_build_dir, options.jobs, options.target)?;
    ensure_windows_link_aliases(
        options.target,
        &layout.zlib_prefix,
        &[("zlibs.lib", "z.lib")],
    )?;
    ensure_windows_zlib_static_alias(options.target, &layout.zlib_prefix)?;
    ensure_windows_zlib_header_compat(options.target, &layout.zlib_prefix)?;
    write_marker(
        &layout.zlib_build_marker,
        "zlib",
        ZLIB_VERSION,
        &layout.zlib_prefix,
    )
}

fn build_dav1d(layout: &WorkspaceLayout, options: DepsOptions) -> Result<()> {
    if !options.target.is_android() && !options.target.is_apple() {
        return Ok(());
    }
    if dav1d_build_marker_is_current(layout, options) && !options.force {
        println!(
            "reuse dav1d build marker {}",
            layout.dav1d_build_marker.display()
        );
        return Ok(());
    }

    // The marker includes the target API and assembly policy. Never mix an
    // older cross configuration with the current build.
    for path in [&layout.dav1d_build_dir, &layout.dav1d_prefix] {
        if path.exists() {
            fs::remove_dir_all(path).with_context(|| format!("remove {}", path.display()))?;
        }
    }
    fs::create_dir_all(&layout.dav1d_prefix)
        .with_context(|| format!("create {}", layout.dav1d_prefix.display()))?;

    let meson = ensure_meson_tools(layout)?;
    let asm_enabled = dav1d_asm_enabled(options.target);
    println!(
        "configure dav1d for {} (asm={asm_enabled})",
        options.target.triple().unwrap_or("host")
    );
    let mut setup = meson_command(&meson);
    setup
        .arg("setup")
        .arg(&layout.dav1d_build_dir)
        .arg(&layout.dav1d_source_dir)
        .arg(format!("--prefix={}", layout.dav1d_prefix.display()))
        .arg("--libdir=lib")
        .arg("--default-library=static")
        .arg("--buildtype=release")
        .arg("-Dbitdepths=8,16")
        .arg(format!("-Denable_asm={asm_enabled}"))
        .arg("-Denable_tools=false")
        .arg("-Denable_examples=false")
        .arg("-Denable_tests=false")
        .arg("-Denable_docs=false")
        .arg("-Dlogging=true");
    apply_meson_target(&mut setup, layout, options.target, "dav1d")?;
    apply_windows_target_env(&mut setup, options.target)?;
    run(&mut setup)?;
    meson_compile_install(
        &meson,
        &layout.dav1d_build_dir,
        options.jobs,
        options.target,
    )?;

    let archive = layout.dav1d_prefix.join("lib/libdav1d.a");
    let pkg_config = layout.dav1d_prefix.join("lib/pkgconfig/dav1d.pc");
    for path in [&archive, &pkg_config] {
        if !path.is_file() {
            bail!("dav1d install did not produce {}", path.display());
        }
    }
    fs::write(
        &layout.dav1d_build_marker,
        format!(
            "dav1d={DAV1D_VERSION}\ntarget={}\nandroid_api={}\nasm={asm_enabled}\nprefix={}\n",
            options.target.triple().unwrap_or("host"),
            if options.target.is_android() {
                android_api_level()?.to_string()
            } else {
                "n/a".to_string()
            },
            layout.dav1d_prefix.display(),
        ),
    )
    .with_context(|| format!("write {}", layout.dav1d_build_marker.display()))
}

fn dav1d_asm_enabled(target: NativeTarget) -> bool {
    // Match the FFmpeg policy for 32-bit Android x86: omit assembly that can
    // introduce text relocations into the final shared library.
    !matches!(target, NativeTarget::I686Android)
}

fn build_freetype(layout: &WorkspaceLayout, options: DepsOptions) -> Result<()> {
    if layout.freetype_build_marker.exists() && !options.force {
        println!(
            "reuse FreeType build marker {}",
            layout.freetype_build_marker.display()
        );
        return Ok(());
    }
    clean_build_and_prefix(options, &layout.freetype_build_dir, &layout.freetype_prefix)?;
    fs::create_dir_all(&layout.freetype_build_dir)
        .with_context(|| format!("create {}", layout.freetype_build_dir.display()))?;
    fs::create_dir_all(&layout.freetype_prefix)
        .with_context(|| format!("create {}", layout.freetype_prefix.display()))?;

    println!("configure FreeType");
    let mut configure = cmake_command(options.target)?;
    configure
        .arg("-S")
        .arg(&layout.freetype_source_dir)
        .arg("-B")
        .arg(&layout.freetype_build_dir)
        .arg("-DCMAKE_BUILD_TYPE=Release")
        .arg("-DBUILD_SHARED_LIBS=OFF")
        .arg(format!(
            "-DCMAKE_INSTALL_PREFIX={}",
            layout.freetype_prefix.display()
        ))
        .arg("-DFT_DISABLE_ZLIB=TRUE")
        .arg("-DFT_DISABLE_BZIP2=TRUE")
        .arg("-DFT_DISABLE_PNG=TRUE")
        .arg("-DFT_DISABLE_HARFBUZZ=TRUE")
        .arg("-DFT_DISABLE_BROTLI=TRUE");
    apply_cmake_target(&mut configure, options.target)?;
    run(&mut configure)?;
    cmake_build_install(&layout.freetype_build_dir, options.jobs, options.target)?;
    write_marker(
        &layout.freetype_build_marker,
        "freetype",
        FREETYPE_VERSION,
        &layout.freetype_prefix,
    )
}

fn build_harfbuzz(layout: &WorkspaceLayout, options: DepsOptions) -> Result<()> {
    if layout.harfbuzz_build_marker.exists() && !options.force {
        println!(
            "reuse HarfBuzz build marker {}",
            layout.harfbuzz_build_marker.display()
        );
        return Ok(());
    }
    clean_build_and_prefix(options, &layout.harfbuzz_build_dir, &layout.harfbuzz_prefix)?;
    fs::create_dir_all(&layout.harfbuzz_build_dir)
        .with_context(|| format!("create {}", layout.harfbuzz_build_dir.display()))?;
    fs::create_dir_all(&layout.harfbuzz_prefix)
        .with_context(|| format!("create {}", layout.harfbuzz_prefix.display()))?;

    println!("configure HarfBuzz");
    let mut configure = cmake_command(options.target)?;
    configure
        .arg("-S")
        .arg(&layout.harfbuzz_source_dir)
        .arg("-B")
        .arg(&layout.harfbuzz_build_dir)
        .arg("-DCMAKE_BUILD_TYPE=Release")
        .arg("-DBUILD_SHARED_LIBS=OFF")
        .arg(format!(
            "-DCMAKE_INSTALL_PREFIX={}",
            layout.harfbuzz_prefix.display()
        ))
        .arg("-DHB_HAVE_FREETYPE=OFF")
        .arg("-DHB_HAVE_GLIB=OFF")
        .arg("-DHB_HAVE_GOBJECT=OFF")
        .arg("-DHB_HAVE_ICU=OFF")
        .arg("-DHB_HAVE_CAIRO=OFF")
        .arg("-DHB_BUILD_UTILS=OFF")
        .arg("-DHB_BUILD_SUBSET=OFF");
    if options.target.is_windows() {
        configure
            .arg("-DHB_HAVE_CORETEXT=OFF")
            .arg("-DHB_HAVE_DIRECTWRITE=ON");
    } else if options.target.is_apple() {
        configure
            .arg("-DHB_HAVE_CORETEXT=ON")
            .arg("-DHB_HAVE_DIRECTWRITE=OFF");
    } else {
        configure
            .arg("-DHB_HAVE_CORETEXT=OFF")
            .arg("-DHB_HAVE_DIRECTWRITE=OFF");
    }
    apply_cmake_target(&mut configure, options.target)?;
    run(&mut configure)?;
    cmake_build_install(&layout.harfbuzz_build_dir, options.jobs, options.target)?;
    write_marker(
        &layout.harfbuzz_build_marker,
        "harfbuzz",
        HARFBUZZ_VERSION,
        &layout.harfbuzz_prefix,
    )
}

fn build_fribidi(layout: &WorkspaceLayout, options: DepsOptions) -> Result<()> {
    if layout.fribidi_build_marker.exists() && !options.force {
        println!(
            "reuse FriBidi build marker {}",
            layout.fribidi_build_marker.display()
        );
        ensure_windows_link_aliases(
            options.target,
            &layout.fribidi_prefix,
            &[("libfribidi.a", "fribidi.lib")],
        )?;
        return Ok(());
    }
    patch_fribidi_meson_native_compiler(layout)?;
    if layout.fribidi_build_dir.exists() && !layout.fribidi_build_marker.exists() {
        fs::remove_dir_all(&layout.fribidi_build_dir)
            .with_context(|| format!("remove stale {}", layout.fribidi_build_dir.display()))?;
    }
    let meson = ensure_meson_tools(layout)?;
    clean_build_and_prefix(options, &layout.fribidi_build_dir, &layout.fribidi_prefix)?;
    fs::create_dir_all(&layout.fribidi_prefix)
        .with_context(|| format!("create {}", layout.fribidi_prefix.display()))?;
    println!("configure FriBidi");
    let mut setup = meson_command(&meson);
    setup
        .arg("setup")
        .arg(&layout.fribidi_build_dir)
        .arg(&layout.fribidi_source_dir)
        .arg(format!("--prefix={}", layout.fribidi_prefix.display()))
        .arg("--default-library=static")
        .arg("--buildtype=release")
        .arg("-Ddocs=false")
        .arg("-Dtests=false");
    apply_meson_target(&mut setup, layout, options.target, "fribidi")?;
    apply_windows_target_env(&mut setup, options.target)?;
    run(&mut setup)?;
    meson_compile_install(
        &meson,
        &layout.fribidi_build_dir,
        options.jobs,
        options.target,
    )?;
    ensure_windows_link_aliases(
        options.target,
        &layout.fribidi_prefix,
        &[("libfribidi.a", "fribidi.lib")],
    )?;
    write_marker(
        &layout.fribidi_build_marker,
        "fribidi",
        FRIBIDI_VERSION,
        &layout.fribidi_prefix,
    )
}

fn patch_fribidi_meson_native_compiler(layout: &WorkspaceLayout) -> Result<()> {
    let path = layout.fribidi_source_dir.join("gen.tab/meson.build");
    let contents = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let original = "native_cc = meson.get_compiler('c')";
    let replacement = "native_cc = meson.get_compiler('c', native: true)";
    if contents.contains(replacement) {
        return Ok(());
    }
    if !contents.contains(original) {
        bail!(
            "FriBidi native compiler declaration was not found in {}; update the Erika patch for this FriBidi version",
            path.display()
        );
    }
    fs::write(&path, contents.replacen(original, replacement, 1))
        .with_context(|| format!("patch {}", path.display()))?;
    println!("patched FriBidi Meson generators to probe headers with the build-machine compiler");
    Ok(())
}

fn build_libass(layout: &WorkspaceLayout, options: DepsOptions) -> Result<()> {
    if layout.libass_build_marker.exists() && !options.force {
        println!(
            "reuse libass build marker {}",
            layout.libass_build_marker.display()
        );
        ensure_windows_link_aliases(
            options.target,
            &layout.libass_prefix,
            &[("libass.a", "ass.lib")],
        )?;
        return Ok(());
    }
    if layout.libass_build_dir.exists() && !layout.libass_build_marker.exists() {
        fs::remove_dir_all(&layout.libass_build_dir)
            .with_context(|| format!("remove stale {}", layout.libass_build_dir.display()))?;
    }
    let meson = ensure_meson_tools(layout)?;
    clean_build_and_prefix(options, &layout.libass_build_dir, &layout.libass_prefix)?;
    fs::create_dir_all(&layout.libass_prefix)
        .with_context(|| format!("create {}", layout.libass_prefix.display()))?;

    let pkg_config_path = pkg_config_path([
        &layout.freetype_prefix,
        &layout.harfbuzz_prefix,
        &layout.fribidi_prefix,
    ]);
    let pkg_config = ensure_pkg_config_shim(layout)?;
    println!("configure libass");
    let mut setup = meson_command(&meson);
    setup
        .arg("setup")
        .arg(&layout.libass_build_dir)
        .arg(&layout.libass_source_dir)
        .arg(format!("--prefix={}", layout.libass_prefix.display()))
        .arg("--default-library=static")
        .arg("--buildtype=release")
        .arg("-Dtest=disabled")
        .arg("-Dprofile=disabled")
        .arg("-Dfontconfig=disabled")
        .arg("-Dasm=disabled")
        .arg("-Dlibunibreak=disabled")
        .env("PKG_CONFIG_PATH", &pkg_config_path)
        .env("PKG_CONFIG", &pkg_config)
        .env("ERIKA_PKG_CONFIG_RELATIVE_BASE", &layout.libass_build_dir);
    if options.target.is_windows() {
        setup
            .arg("-Dcoretext=disabled")
            .arg("-Ddirectwrite=enabled");
    } else if options.target.is_apple() {
        setup
            .arg("-Dcoretext=enabled")
            .arg("-Ddirectwrite=disabled");
    } else {
        setup
            .arg("-Dcoretext=disabled")
            .arg("-Ddirectwrite=disabled")
            .arg("-Drequire-system-font-provider=false");
    }
    apply_meson_target(&mut setup, layout, options.target, "libass")?;
    apply_windows_target_env(&mut setup, options.target)?;
    run(&mut setup)?;

    let mut compile = meson_command(&meson);
    compile
        .arg("compile")
        .arg("-C")
        .arg(&layout.libass_build_dir)
        .env("PKG_CONFIG_PATH", &pkg_config_path)
        .env("PKG_CONFIG", &pkg_config)
        .env("ERIKA_PKG_CONFIG_RELATIVE_BASE", &layout.libass_build_dir);
    if let Some(jobs) = options.jobs {
        compile.arg(format!("-j{jobs}"));
    }
    apply_windows_target_env(&mut compile, options.target)?;
    apply_android_host_env(&mut compile, options.target)?;
    run(&mut compile)?;
    let mut install = meson_command(&meson);
    install
        .arg("install")
        .arg("-C")
        .arg(&layout.libass_build_dir)
        .env("PKG_CONFIG_PATH", &pkg_config_path)
        .env("PKG_CONFIG", &pkg_config)
        .env("ERIKA_PKG_CONFIG_RELATIVE_BASE", &layout.libass_build_dir);
    apply_windows_target_env(&mut install, options.target)?;
    apply_android_host_env(&mut install, options.target)?;
    run(&mut install)?;
    ensure_windows_link_aliases(
        options.target,
        &layout.libass_prefix,
        &[("libass.a", "ass.lib")],
    )?;

    write_marker(
        &layout.libass_build_marker,
        "libass",
        LIBASS_VERSION,
        &layout.libass_prefix,
    )
}

fn cmake_build_install(
    build_dir: &std::path::Path,
    jobs: Option<usize>,
    target: NativeTarget,
) -> Result<()> {
    let mut build = cmake_command(target)?;
    build
        .arg("--build")
        .arg(build_dir)
        .arg("--config")
        .arg("Release");
    if let Some(jobs) = jobs {
        build.arg("--parallel").arg(jobs.to_string());
    }
    apply_windows_target_env(&mut build, target)?;
    run(&mut build)?;
    let mut install = cmake_command(target)?;
    install
        .arg("--install")
        .arg(build_dir)
        .arg("--config")
        .arg("Release");
    apply_windows_target_env(&mut install, target)?;
    run(&mut install)
}

#[derive(Debug, Clone)]
struct MesonTools {
    meson: PathBuf,
    bin_dir: PathBuf,
}

fn ensure_meson_tools(layout: &WorkspaceLayout) -> Result<MesonTools> {
    if let Some(meson) = which("meson") {
        if which("ninja").is_some() {
            let bin_dir = meson.parent().unwrap_or(Path::new("")).to_path_buf();
            return Ok(MesonTools { meson, bin_dir });
        }
    }

    let venv = layout.python_tools_dir.join("venv");
    let bin_dir = venv_bin_dir(&venv);
    let meson = executable_in_dir(&bin_dir, "meson");
    let ninja = executable_in_dir(&bin_dir, "ninja");
    if meson.exists() && ninja.exists() {
        return Ok(MesonTools { meson, bin_dir });
    }

    fs::create_dir_all(&layout.python_tools_dir)
        .with_context(|| format!("create {}", layout.python_tools_dir.display()))?;
    let python = python_tool().context("required Python was not found in PATH")?;
    println!("bootstrap local meson/ninja tools");
    run(Command::new(python).arg("-m").arg("venv").arg(&venv))?;
    run(Command::new(executable_in_dir(&bin_dir, "python"))
        .arg("-m")
        .arg("pip")
        .arg("install")
        .arg("--upgrade")
        .arg("pip")
        .arg("meson==1.8.5")
        .arg("ninja==1.13.0"))?;
    Ok(MesonTools { meson, bin_dir })
}

fn meson_command(meson: &MesonTools) -> Command {
    let mut command = Command::new(&meson.meson);
    prepend_path(&mut command, &meson.bin_dir);
    command
}

fn cmake_command(target: NativeTarget) -> Result<Command> {
    let _ = target;
    let cmake = cmake_tool().context("required CMake was not found")?;
    Ok(Command::new(cmake))
}

fn apply_cmake_target(command: &mut Command, target: NativeTarget) -> Result<()> {
    apply_cmake_apple_target(command, target)?;
    if let Some(config) = android_toolchain(target)? {
        command
            .arg(format!(
                "-DCMAKE_TOOLCHAIN_FILE={}",
                config.cmake_toolchain_file.display()
            ))
            .arg(format!("-DANDROID_ABI={}", config.abi))
            .arg(format!("-DANDROID_PLATFORM=android-{}", config.api_level))
            .arg("-DANDROID_STL=c++_shared")
            .arg("-DCMAKE_POSITION_INDEPENDENT_CODE=ON");
        if let Some(ninja) = ninja_tool() {
            command
                .arg("-G")
                .arg("Ninja")
                .arg(format!("-DCMAKE_MAKE_PROGRAM={}", ninja.display()));
        }
    }
    if target.is_windows() {
        if let Some(ninja) = ninja_tool() {
            command
                .arg("-G")
                .arg("Ninja")
                .arg(format!("-DCMAKE_MAKE_PROGRAM={}", ninja.display()));
        }
        apply_windows_target_env(command, target)?;
    }
    Ok(())
}

fn apply_cmake_apple_target(command: &mut Command, target: NativeTarget) -> Result<()> {
    let Some(config) = apple_toolchain(target)? else {
        return Ok(());
    };
    command
        .arg(format!("-DCMAKE_C_COMPILER={}", config.clang.display()))
        .arg(format!("-DCMAKE_CXX_COMPILER={}", config.clangxx.display()))
        .arg(format!("-DCMAKE_AR={}", config.ar.display()))
        .arg(format!("-DCMAKE_RANLIB={}", config.ranlib.display()))
        .arg(format!("-DCMAKE_OSX_SYSROOT={}", config.sdk_root.display()))
        .arg(format!("-DCMAKE_OSX_ARCHITECTURES={}", config.arch))
        .arg(format!("-DCMAKE_SYSTEM_PROCESSOR={}", config.arch))
        .arg(format!(
            "-DCMAKE_OSX_DEPLOYMENT_TARGET={}",
            config.deployment_target
        ));
    if target.is_ios() {
        command.arg("-DCMAKE_SYSTEM_NAME=iOS");
    }
    apply_apple_target_env(command, target)
}

fn apply_meson_target(
    command: &mut Command,
    layout: &WorkspaceLayout,
    target: NativeTarget,
    name: &str,
) -> Result<()> {
    let Some(cross_file) = meson_cross_file(layout, target, name)? else {
        return Ok(());
    };
    command.arg("--cross-file").arg(cross_file);
    // Cross builds (e.g. iOS) compile native generator tools such as FriBidi's
    // gen.tab on the build machine. Provide an explicit build-machine compiler
    // pinned to the macOS SDK so the iOS SDKROOT we export below does not make
    // those native tools target iOS and fail to run.
    let native_file = meson_native_file(layout, target, name)?;
    command.arg("--native-file").arg(native_file);
    apply_apple_target_env(command, target)?;
    apply_android_host_env(command, target)
}

fn meson_native_file(
    layout: &WorkspaceLayout,
    target: NativeTarget,
    name: &str,
) -> Result<PathBuf> {
    let path = layout.build_dir.join(format!("{name}-meson-native.ini"));
    let content = if target.is_apple() {
        let sdk_root = xcrun("macosx", &["--show-sdk-path"])?;
        let clang = xcrun("macosx", &["-f", "clang"])?;
        let clangxx = xcrun("macosx", &["-f", "clang++"])?;
        // SDKROOT belongs to the target build. Pin native generators back to macOS.
        let arch = match env::consts::ARCH {
            "aarch64" => "arm64",
            other => other,
        };
        let host_target = format!("{arch}-apple-macos");
        format!(
            "[binaries]\nc = [{}, '-target', {}, '-isysroot', {}]\ncpp = [{}, '-target', {}, '-isysroot', {}]\n",
            meson_string(&clang),
            meson_string(&host_target),
            meson_string(&sdk_root),
            meson_string(&clangxx),
            meson_string(&host_target),
            meson_string(&sdk_root),
        )
    } else {
        format!(
            "[binaries]\nc = {}\ncpp = {}\n",
            meson_string(&host_c_compiler()?.display().to_string()),
            meson_string(&host_cxx_compiler()?.display().to_string()),
        )
    };
    fs::write(&path, content).with_context(|| format!("write {}", path.display()))?;
    Ok(path)
}

fn meson_cross_file(
    layout: &WorkspaceLayout,
    target: NativeTarget,
    name: &str,
) -> Result<Option<PathBuf>> {
    let path = layout.build_dir.join(format!("{name}-meson-cross.ini"));
    let content = if let Some(config) = apple_toolchain(target)? {
        let pkg_config = which("pkg-config").unwrap_or_else(|| PathBuf::from("pkg-config"));
        let arch_flags = apple_arch_flags(&config);
        format!(
            "[binaries]\nc = {}\ncpp = {}\nar = {}\nstrip = {}\npkg-config = {}\n\n[built-in options]\nc_args = {}\ncpp_args = {}\nc_link_args = {}\ncpp_link_args = {}\n\n[host_machine]\nsystem = 'darwin'\ncpu_family = {}\ncpu = {}\nendian = 'little'\n",
            meson_string(&config.clang.display().to_string()),
            meson_string(&config.clangxx.display().to_string()),
            meson_string(&config.ar.display().to_string()),
            meson_string(&config.strip.display().to_string()),
            meson_string(&pkg_config.display().to_string()),
            meson_array(&arch_flags),
            meson_array(&arch_flags),
            meson_array(&arch_flags),
            meson_array(&arch_flags),
            meson_string(
                target
                    .meson_cpu_family()
                    .context("explicit Apple target must have a Meson CPU family")?,
            ),
            meson_string(
                target
                    .meson_cpu()
                    .context("explicit Apple target must have a Meson CPU")?,
            ),
        )
    } else if let Some(config) = android_toolchain(target)? {
        let pkg_config = ensure_pkg_config_shim(layout)?;
        let pic_flags = vec!["-fPIC".to_string()];
        format!(
            "[binaries]\nc = {}\ncpp = {}\nar = {}\nstrip = {}\npkg-config = {}\n\n[built-in options]\nc_args = {}\ncpp_args = {}\nc_link_args = {}\ncpp_link_args = {}\n\n[properties]\nneeds_exe_wrapper = true\n\n[host_machine]\nsystem = 'android'\ncpu_family = {}\ncpu = {}\nendian = 'little'\n",
            meson_string(&config.clang.display().to_string()),
            meson_string(&config.clangxx.display().to_string()),
            meson_string(&config.ar.display().to_string()),
            meson_string(&config.strip.display().to_string()),
            meson_string(&pkg_config.display().to_string()),
            meson_array(&pic_flags),
            meson_array(&pic_flags),
            meson_array(&pic_flags),
            meson_array(&pic_flags),
            meson_string(
                target
                    .meson_cpu_family()
                    .context("explicit Android target must have a Meson CPU family")?,
            ),
            meson_string(
                target
                    .meson_cpu()
                    .context("explicit Android target must have a Meson CPU")?,
            ),
        )
    } else {
        return Ok(None);
    };
    fs::write(&path, content).with_context(|| format!("write {}", path.display()))?;
    Ok(Some(path))
}

fn apply_apple_target_env(command: &mut Command, target: NativeTarget) -> Result<()> {
    let Some(config) = apple_toolchain(target)? else {
        return Ok(());
    };
    command.env("SDKROOT", &config.sdk_root);
    if target.is_ios() {
        command.env("IPHONEOS_DEPLOYMENT_TARGET", &config.deployment_target);
    } else {
        command.env("MACOSX_DEPLOYMENT_TARGET", &config.deployment_target);
    }
    Ok(())
}

fn apple_arch_flags(config: &AppleToolchain) -> Vec<String> {
    vec![
        "-arch".to_string(),
        config.arch.to_string(),
        "-isysroot".to_string(),
        config.sdk_root.display().to_string(),
        format!("{}={}", config.deployment_flag, config.deployment_target),
    ]
}

fn meson_array(values: &[String]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| meson_string(value))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn meson_string(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

fn prepend_path(command: &mut Command, dir: &Path) {
    let mut paths = vec![dir.to_path_buf()];
    if let Some(path) = env::var_os("PATH") {
        paths.extend(env::split_paths(&path));
    }
    command.env(
        "PATH",
        env::join_paths(paths).expect("PATH entries are valid"),
    );
}

fn meson_compile_install(
    meson: &MesonTools,
    build_dir: &std::path::Path,
    jobs: Option<usize>,
    target: NativeTarget,
) -> Result<()> {
    let mut compile = meson_command(meson);
    compile.arg("compile").arg("-C").arg(build_dir);
    if let Some(jobs) = jobs {
        compile.arg(format!("-j{jobs}"));
    }
    apply_windows_target_env(&mut compile, target)?;
    apply_android_host_env(&mut compile, target)?;
    run(&mut compile)?;
    let mut install = meson_command(meson);
    install.arg("install").arg("-C").arg(build_dir);
    apply_windows_target_env(&mut install, target)?;
    apply_android_host_env(&mut install, target)?;
    run(&mut install)
}

fn clean_build_and_prefix(
    options: DepsOptions,
    build_dir: &std::path::Path,
    prefix: &std::path::Path,
) -> Result<()> {
    if options.force && prefix.exists() {
        fs::remove_dir_all(prefix).with_context(|| format!("remove {}", prefix.display()))?;
    }
    if options.force && build_dir.exists() {
        fs::remove_dir_all(build_dir).with_context(|| format!("remove {}", build_dir.display()))?;
    }
    Ok(())
}

fn write_marker(
    path: &std::path::Path,
    name: &str,
    version: &str,
    prefix: &std::path::Path,
) -> Result<()> {
    fs::write(
        path,
        format!("{name}={version}\nprefix={}\n", prefix.display()),
    )
    .with_context(|| format!("write {}", path.display()))
}

fn pkg_config_path<'a>(prefixes: impl IntoIterator<Item = &'a PathBuf>) -> String {
    env::join_paths(
        prefixes
            .into_iter()
            .map(|prefix| prefix.join("lib/pkgconfig")),
    )
    .expect("pkg-config path entries are valid")
    .to_string_lossy()
    .into_owned()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UnifiedPatchHunk {
    old_start: usize,
    old_lines: Vec<String>,
    new_lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UnifiedPatchFile {
    path: PathBuf,
    hunks: Vec<UnifiedPatchHunk>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PatchApplication {
    Applied,
    AlreadyApplied,
}

fn apply_ffmpeg_patches(layout: &WorkspaceLayout) -> Result<()> {
    validate_generated_ffmpeg_source_path(layout)?;
    let patchset = ffmpeg_patchset_id(&layout.root)?;
    let application = match apply_ffmpeg_patch_files(layout) {
        Ok(application) => application,
        Err(first_error) => {
            // third_party/src is generated from the pinned archive. If an older
            // Erika patch set or an interrupted partial application left it in
            // an unknown state, refresh it automatically instead of requiring
            // users to repair the vendored source tree by hand.
            println!(
                "FFmpeg source does not match patch set {patchset}; refresh the pinned source ({first_error:#})"
            );
            refresh_ffmpeg_source(layout)?;
            apply_ffmpeg_patch_files(layout).with_context(|| {
                format!(
                    "apply FFmpeg patch set {patchset} after refreshing {}",
                    layout.ffmpeg_source_dir.display()
                )
            })?
        }
    };
    fs::write(
        layout.ffmpeg_source_dir.join(".erika-patchset"),
        format!("{patchset}\n"),
    )
    .with_context(|| {
        format!(
            "write FFmpeg patch stamp in {}",
            layout.ffmpeg_source_dir.display()
        )
    })?;
    match application {
        PatchApplication::Applied => println!("applied FFmpeg patch set {patchset}"),
        PatchApplication::AlreadyApplied => println!("reuse FFmpeg patch set {patchset}"),
    }
    Ok(())
}

fn apply_ffmpeg_patch_files(layout: &WorkspaceLayout) -> Result<PatchApplication> {
    let mut application = PatchApplication::AlreadyApplied;
    for relative_path in FFMPEG_PATCHES {
        let patch_path = layout.root.join(relative_path);
        let patch = fs::read_to_string(&patch_path)
            .with_context(|| format!("read FFmpeg patch {}", patch_path.display()))?;
        if apply_unified_patch(&layout.ffmpeg_source_dir, &patch)
            .with_context(|| format!("apply FFmpeg patch {}", patch_path.display()))?
            == PatchApplication::Applied
        {
            application = PatchApplication::Applied;
        }
    }
    Ok(application)
}

fn refresh_ffmpeg_source(layout: &WorkspaceLayout) -> Result<()> {
    validate_generated_ffmpeg_source_path(layout)?;
    if layout.ffmpeg_source_dir.exists() {
        fs::remove_dir_all(&layout.ffmpeg_source_dir)
            .with_context(|| format!("remove {}", layout.ffmpeg_source_dir.display()))?;
    }
    extract_archive(&layout.cache_dir.join(FFMPEG_ARCHIVE), &layout.source_dir)
}

fn validate_generated_ffmpeg_source_path(layout: &WorkspaceLayout) -> Result<()> {
    let expected_source = layout.source_dir.join(FFMPEG_DIR);
    if layout.ffmpeg_source_dir != expected_source {
        bail!(
            "refuse to modify unexpected FFmpeg source path {}",
            layout.ffmpeg_source_dir.display()
        );
    }
    let canonical_root = fs::canonicalize(&layout.root)
        .with_context(|| format!("resolve workspace root {}", layout.root.display()))?;
    let canonical_source_dir = fs::canonicalize(&layout.source_dir)
        .with_context(|| format!("resolve source root {}", layout.source_dir.display()))?;
    if !canonical_source_dir.starts_with(&canonical_root) {
        bail!(
            "refuse to modify FFmpeg source root outside the workspace: {}",
            canonical_source_dir.display()
        );
    }
    if layout.ffmpeg_source_dir.exists() {
        let canonical_ffmpeg = fs::canonicalize(&layout.ffmpeg_source_dir).with_context(|| {
            format!(
                "resolve FFmpeg source path {}",
                layout.ffmpeg_source_dir.display()
            )
        })?;
        let canonical_expected = canonical_source_dir.join(FFMPEG_DIR);
        if canonical_ffmpeg != canonical_expected {
            bail!(
                "refuse to modify redirected FFmpeg source path: {}",
                canonical_ffmpeg.display()
            );
        }
    }
    Ok(())
}

fn ffmpeg_patchset_id(root: &Path) -> Result<String> {
    let mut hash = 0xcbf29ce484222325_u64;
    fn update(hash: &mut u64, bytes: &[u8]) {
        for byte in bytes {
            *hash ^= u64::from(*byte);
            *hash = hash.wrapping_mul(0x100000001b3);
        }
    }

    update(&mut hash, FFMPEG_PATCHSET_VERSION.as_bytes());
    for relative_path in FFMPEG_PATCHES {
        update(&mut hash, &[0]);
        update(&mut hash, relative_path.as_bytes());
        update(&mut hash, &[0]);
        let patch_path = root.join(relative_path);
        let patch = fs::read(&patch_path)
            .with_context(|| format!("read FFmpeg patch {}", patch_path.display()))?;
        update(&mut hash, &patch);
    }
    Ok(format!("{FFMPEG_PATCHSET_VERSION}-{hash:016x}"))
}

fn apply_unified_patch(source_root: &Path, patch: &str) -> Result<PatchApplication> {
    let files = parse_unified_patch(patch)?;
    let mut pending_writes = Vec::new();
    for file in files {
        if file.path.is_absolute()
            || file
                .path
                .components()
                .any(|component| !matches!(component, std::path::Component::Normal(_)))
        {
            bail!("unsafe path in unified patch: {}", file.path.display());
        }
        let path = source_root.join(&file.path);
        let contents = fs::read_to_string(&path)
            .with_context(|| format!("read patch target {}", path.display()))?;
        let (updated, changed) = apply_patch_hunks(&contents, &file.hunks)
            .with_context(|| format!("patch {}", path.display()))?;
        if changed {
            pending_writes.push((path, updated));
        }
    }

    for (path, contents) in pending_writes.iter() {
        fs::write(path, contents).with_context(|| format!("write {}", path.display()))?;
    }
    Ok(if pending_writes.is_empty() {
        PatchApplication::AlreadyApplied
    } else {
        PatchApplication::Applied
    })
}

fn parse_unified_patch(patch: &str) -> Result<Vec<UnifiedPatchFile>> {
    let lines = patch.lines().collect::<Vec<_>>();
    let mut files = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let Some(path) = lines[index].strip_prefix("+++ ") else {
            index += 1;
            continue;
        };
        let path = path
            .split_whitespace()
            .next()
            .context("unified patch +++ line has no path")?;
        let path = path
            .strip_prefix("b/")
            .context("unified patch target must use a b/ path")?;
        let mut file = UnifiedPatchFile {
            path: PathBuf::from(path),
            hunks: Vec::new(),
        };
        index += 1;

        while index < lines.len() && !lines[index].starts_with("diff --git ") {
            if !lines[index].starts_with("@@ ") {
                index += 1;
                continue;
            }
            let (old_start, old_count, new_count) = parse_unified_hunk_header(lines[index])?;
            index += 1;
            let mut old_lines = Vec::with_capacity(old_count);
            let mut new_lines = Vec::with_capacity(new_count);
            while old_lines.len() < old_count || new_lines.len() < new_count {
                let line = lines.get(index).with_context(|| {
                    format!("truncated unified patch hunk at old line {old_start}")
                })?;
                index += 1;
                if line.starts_with('\\') {
                    continue;
                }
                let (prefix, text) = line.split_at(1);
                match prefix {
                    " " => {
                        old_lines.push(text.to_string());
                        new_lines.push(text.to_string());
                    }
                    "-" => old_lines.push(text.to_string()),
                    "+" => new_lines.push(text.to_string()),
                    _ => bail!("invalid unified patch hunk line: {line}"),
                }
                if old_lines.len() > old_count || new_lines.len() > new_count {
                    bail!("unified patch hunk contains more lines than its header declares");
                }
            }
            file.hunks.push(UnifiedPatchHunk {
                old_start,
                old_lines,
                new_lines,
            });
        }
        if file.hunks.is_empty() {
            bail!(
                "unified patch contains no hunks for {}",
                file.path.display()
            );
        }
        files.push(file);
    }
    if files.is_empty() {
        bail!("unified patch contains no target files");
    }
    Ok(files)
}

fn parse_unified_hunk_header(header: &str) -> Result<(usize, usize, usize)> {
    let ranges = header
        .strip_prefix("@@")
        .and_then(|rest| rest.split_once("@@").map(|(ranges, _)| ranges.trim()))
        .with_context(|| format!("invalid unified patch hunk header: {header}"))?;
    let mut ranges = ranges.split_whitespace();
    let (old_start, old_count) = parse_unified_range(
        ranges
            .next()
            .context("unified patch hunk has no old range")?,
        '-',
    )?;
    let (_, new_count) = parse_unified_range(
        ranges
            .next()
            .context("unified patch hunk has no new range")?,
        '+',
    )?;
    Ok((old_start, old_count, new_count))
}

fn parse_unified_range(range: &str, prefix: char) -> Result<(usize, usize)> {
    let range = range
        .strip_prefix(prefix)
        .with_context(|| format!("unified patch range must start with {prefix}: {range}"))?;
    let (start, count) = range.split_once(',').unwrap_or((range, "1"));
    let start = start
        .parse::<usize>()
        .with_context(|| format!("invalid unified patch range start: {range}"))?;
    let count = count
        .parse::<usize>()
        .with_context(|| format!("invalid unified patch range count: {range}"))?;
    if start == 0 || count == 0 {
        bail!("zero-length unified patch ranges are not supported: {range}");
    }
    Ok((start, count))
}

fn apply_patch_hunks(contents: &str, hunks: &[UnifiedPatchHunk]) -> Result<(String, bool)> {
    let trailing_newline = contents.ends_with('\n');
    let mut lines = contents.lines().map(str::to_string).collect::<Vec<_>>();
    let mut line_shift = 0_isize;
    let mut changed = false;
    for hunk in hunks {
        let expected = isize::try_from(hunk.old_start - 1)
            .context("unified patch line number does not fit isize")?
            + line_shift;
        if expected < 0 {
            bail!("unified patch hunk resolves before the start of the file");
        }
        let expected = usize::try_from(expected).expect("non-negative patch line index");
        if patch_lines_match(&lines, expected, &hunk.old_lines) {
            lines.splice(
                expected..expected + hunk.old_lines.len(),
                hunk.new_lines.iter().cloned(),
            );
            changed = true;
        } else if !patch_lines_match(&lines, expected, &hunk.new_lines) {
            bail!(
                "hunk at source line {} matches neither the pinned FFmpeg source nor the already-patched source",
                hunk.old_start
            );
        }
        line_shift += hunk.new_lines.len() as isize - hunk.old_lines.len() as isize;
    }

    let mut updated = lines.join("\n");
    if trailing_newline {
        updated.push('\n');
    }
    Ok((updated, changed))
}

fn patch_lines_match(lines: &[String], start: usize, expected: &[String]) -> bool {
    start
        .checked_add(expected.len())
        .filter(|end| *end <= lines.len())
        .is_some_and(|end| lines[start..end] == *expected)
}

fn fetch_and_extract(
    layout: &WorkspaceLayout,
    urls: &[&str],
    archive_name: &str,
    source_dir_name: &str,
) -> Result<()> {
    let archive_path = layout.cache_dir.join(archive_name);
    let partial_path = layout.cache_dir.join(format!("{archive_name}.part"));
    if !archive_path.exists() {
        download_archive(urls, &partial_path, &archive_path)?;
    } else {
        println!("reuse {}", archive_path.display());
    }

    let source_path = layout.source_dir.join(source_dir_name);
    if !source_path.exists() {
        println!("extract {}", archive_path.display());
        extract_archive(&archive_path, &layout.source_dir)?;
    } else {
        println!("reuse {}", source_path.display());
    }
    Ok(())
}

fn extract_archive(archive_path: &Path, destination: &Path) -> Result<()> {
    run(Command::new("tar")
        .arg("-xf")
        .arg(archive_path)
        .arg("-C")
        .arg(destination))
}

fn download_archive(urls: &[&str], partial_path: &PathBuf, archive_path: &PathBuf) -> Result<()> {
    let mut last_error = None;
    let agent = download_agent();
    for url in urls {
        println!("download {url}");
        if partial_path.exists() {
            fs::remove_file(partial_path)
                .with_context(|| format!("remove {}", partial_path.display()))?;
        }
        match download_url(&agent, url, partial_path) {
            Ok(()) => {
                fs::rename(partial_path, archive_path).with_context(|| {
                    format!(
                        "rename {} to {}",
                        partial_path.display(),
                        archive_path.display()
                    )
                })?;
                return Ok(());
            }
            Err(error) => {
                last_error = Some(error);
                let _ = fs::remove_file(partial_path);
                println!("download failed, trying next source if available");
            }
        }
    }
    match last_error {
        Some(error) => Err(error).context("all download sources failed"),
        None => bail!(
            "no download sources configured for {}",
            archive_path.display()
        ),
    }
}

fn download_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(20)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .timeout_recv_body(Some(Duration::from_secs(300)))
        .max_redirects(10)
        .build()
        .into()
}

fn download_url(agent: &ureq::Agent, url: &str, partial_path: &Path) -> Result<()> {
    let mut response = agent
        .get(url)
        .header("User-Agent", "erika-xtask")
        .call()
        .with_context(|| format!("download {url}"))?;
    let mut reader = response.body_mut().as_reader();
    let mut output =
        File::create(partial_path).with_context(|| format!("create {}", partial_path.display()))?;
    io::copy(&mut reader, &mut output)
        .with_context(|| format!("write {}", partial_path.display()))?;
    Ok(())
}

fn build_ffmpeg(layout: &WorkspaceLayout, options: DepsOptions) -> Result<()> {
    let marker_is_current = ffmpeg_build_marker_is_current(layout, options);
    if marker_is_current && !options.force {
        println!(
            "reuse FFmpeg build marker {}",
            layout.ffmpeg_build_marker.display()
        );
        ensure_windows_link_aliases(
            options.target,
            &layout.ffmpeg_prefix,
            &[
                ("libavdevice.a", "avdevice.lib"),
                ("libavfilter.a", "avfilter.lib"),
                ("libavformat.a", "avformat.lib"),
                ("libavcodec.a", "avcodec.lib"),
                ("libswresample.a", "swresample.lib"),
                ("libswscale.a", "swscale.lib"),
                ("libavutil.a", "avutil.lib"),
            ],
        )?;
        return Ok(());
    }

    // A changed target/configuration marker must never reuse objects or static
    // archive members produced by the previous FFmpeg configuration.
    if layout.ffmpeg_prefix.exists() {
        fs::remove_dir_all(&layout.ffmpeg_prefix)
            .with_context(|| format!("remove {}", layout.ffmpeg_prefix.display()))?;
    }
    if layout.ffmpeg_build_dir.exists() {
        fs::remove_dir_all(&layout.ffmpeg_build_dir)
            .with_context(|| format!("remove {}", layout.ffmpeg_build_dir.display()))?;
    }
    fs::create_dir_all(&layout.ffmpeg_build_dir)
        .with_context(|| format!("create {}", layout.ffmpeg_build_dir.display()))?;
    fs::create_dir_all(&layout.ffmpeg_prefix)
        .with_context(|| format!("create {}", layout.ffmpeg_prefix.display()))?;
    if uses_windows_posix_ffmpeg(options.target)
        && !layout.ffmpeg_build_dir.join("configure").exists()
    {
        println!("copy FFmpeg source for Windows-hosted in-tree build");
        copy_dir_all(&layout.ffmpeg_source_dir, &layout.ffmpeg_build_dir)?;
    }

    let mut configure = if uses_windows_posix_ffmpeg(options.target) {
        let mut command = Command::new(
            posix_shell().context("required POSIX shell was not found for FFmpeg configure")?,
        );
        command.arg("configure");
        command
    } else {
        Command::new(layout.ffmpeg_source_dir.join("configure"))
    };
    configure.current_dir(&layout.ffmpeg_build_dir);
    configure.arg(format!(
        "--prefix={}",
        path_to_forward_slashes(&layout.ffmpeg_prefix)
    ));
    if options.target.is_android() || options.target.is_apple() {
        let pkg_config = ensure_pkg_config_shim(layout)?;
        let dav1d_pkg_config_dir = layout.dav1d_prefix.join("lib/pkgconfig");
        configure
            .arg(format!(
                "--pkg-config={}",
                ffmpeg_flag_path_arg(&pkg_config)
            ))
            .arg("--pkg-config-flags=--static")
            .env("PKG_CONFIG_PATH", &dav1d_pkg_config_dir)
            .env("PKG_CONFIG_LIBDIR", &dav1d_pkg_config_dir)
            .env("ERIKA_PKG_CONFIG_RELATIVE_BASE", &layout.ffmpeg_build_dir);
    } else {
        configure.arg("--pkg-config=false");
    }
    if !options.target.is_android() {
        configure.arg("--disable-x86asm");
    }
    let mut extra_cflags = if options.target.is_windows() {
        Vec::new()
    } else {
        vec!["-fPIC".to_string()]
    };
    let mut extra_ldflags = Vec::new();
    if let Some(config) = apple_toolchain(options.target)? {
        configure.arg(format!("--cc={}", config.clang.display()));
        configure.arg(format!("--ar={}", config.ar.display()));
        configure.arg(format!("--ranlib={}", config.ranlib.display()));
        configure.arg(format!("--strip={}", config.strip.display()));
        configure.arg("--target-os=darwin");
        configure.arg("--enable-cross-compile");
        configure.arg(format!("--arch={}", config.arch));
        configure.arg(format!("--sysroot={}", config.sdk_root.display()));
        extra_cflags.push(format!("-arch {}", config.arch));
        extra_cflags.push(format!("-isysroot {}", config.sdk_root.display()));
        extra_cflags.push(format!(
            "{}={}",
            config.deployment_flag, config.deployment_target
        ));
        extra_ldflags.push(format!("-arch {}", config.arch));
        extra_ldflags.push(format!("-isysroot {}", config.sdk_root.display()));
        extra_ldflags.push(format!(
            "{}={}",
            config.deployment_flag, config.deployment_target
        ));
        extra_cflags.push(format!(
            "-I{}",
            ffmpeg_flag_path_arg(&layout.zlib_prefix.join("include"))
        ));
        extra_ldflags.push(format!(
            "-L{}",
            ffmpeg_flag_path_arg(&layout.zlib_prefix.join("lib"))
        ));
        if options.target.is_apple() {
            extra_cflags.push(format!(
                "-I{}",
                ffmpeg_flag_path_arg(&layout.dav1d_prefix.join("include"))
            ));
            extra_ldflags.push(format!(
                "-L{}",
                ffmpeg_flag_path_arg(&layout.dav1d_prefix.join("lib"))
            ));
        }
        configure.env("SDKROOT", &config.sdk_root);
        match options.target {
            NativeTarget::Aarch64Macos | NativeTarget::X86_64Macos => {
                configure.env("MACOSX_DEPLOYMENT_TARGET", &config.deployment_target);
            }
            NativeTarget::Aarch64Ios
            | NativeTarget::Aarch64IosSimulator
            | NativeTarget::X86_64IosSimulator => {
                configure.env("IPHONEOS_DEPLOYMENT_TARGET", &config.deployment_target);
            }
            NativeTarget::Host
            | NativeTarget::X86_64WindowsMsvc
            | NativeTarget::Aarch64Android
            | NativeTarget::Armv7Android
            | NativeTarget::X86_64Android
            | NativeTarget::I686Android => {}
        }
    } else if let Some(config) = android_toolchain(options.target)? {
        configure.arg(format!("--cc={}", ffmpeg_flag_path_arg(&config.clang)));
        configure.arg(format!("--cxx={}", ffmpeg_flag_path_arg(&config.clangxx)));
        configure.arg(format!("--ar={}", ffmpeg_flag_path_arg(&config.ar)));
        configure.arg(format!("--ranlib={}", ffmpeg_flag_path_arg(&config.ranlib)));
        configure.arg(format!("--strip={}", ffmpeg_flag_path_arg(&config.strip)));
        configure.arg(format!("--nm={}", ffmpeg_flag_path_arg(&config.nm)));
        configure.arg("--target-os=android");
        configure.arg("--enable-cross-compile");
        configure.arg(format!("--arch={}", config.arch));
        configure.arg(format!(
            "--sysroot={}",
            path_to_forward_slashes(&config.sysroot)
        ));
        if let Some(host_cc) = ffmpeg_android_host_cc(&config)? {
            configure.arg(format!("--host-cc={host_cc}"));
        }
        extra_cflags.push(format!(
            "-I{}",
            ffmpeg_flag_path_arg(&layout.zlib_prefix.join("include"))
        ));
        extra_cflags.push(format!(
            "-I{}",
            ffmpeg_flag_path_arg(&layout.dav1d_prefix.join("include"))
        ));
        extra_ldflags.push(format!(
            "-L{}",
            ffmpeg_flag_path_arg(&layout.zlib_prefix.join("lib"))
        ));
        extra_ldflags.push(format!(
            "-L{}",
            ffmpeg_flag_path_arg(&layout.dav1d_prefix.join("lib"))
        ));
        configure.env("ANDROID_NDK_HOME", &config.ndk_root);
    } else if options.target.is_windows() {
        configure.arg("--target-os=win64");
        configure.arg("--arch=x86_64");
        configure.arg("--toolchain=msvc");
        extra_cflags.push(format!(
            "-I{}",
            ffmpeg_flag_path_arg(&layout.zlib_prefix.join("include"))
        ));
        extra_ldflags.push(format!(
            "-libpath:{}",
            ffmpeg_flag_path_arg(&layout.zlib_prefix.join("lib"))
        ));
        apply_windows_target_env(&mut configure, options.target)?;
    } else {
        configure.arg("--cc=clang");
        extra_cflags.push(format!(
            "-I{}",
            ffmpeg_flag_path_arg(&layout.zlib_prefix.join("include"))
        ));
        extra_ldflags.push(format!(
            "-L{}",
            ffmpeg_flag_path_arg(&layout.zlib_prefix.join("lib"))
        ));
    }
    apply_windows_posix_shell(&mut configure, options.target);
    append_windows_posix_paths(&mut configure);
    apply_android_host_env(&mut configure, options.target)?;
    if !extra_cflags.is_empty() {
        configure.arg(format!("--extra-cflags={}", extra_cflags.join(" ")));
    }
    if !extra_ldflags.is_empty() {
        configure.arg(format!("--extra-ldflags={}", extra_ldflags.join(" ")));
    }
    for flag in options
        .profile
        .ffmpeg_configure_flags_for_target(options.target)
    {
        configure.arg(flag);
    }

    println!("configure FFmpeg");
    run(&mut configure)?;
    if cfg!(windows) && options.target.is_android() {
        enable_ffmpeg_archive_response_files(&layout.ffmpeg_build_dir)?;
    }

    let jobs = options.jobs.unwrap_or_else(default_job_count);
    println!("build FFmpeg with {jobs} jobs");
    let make = gnu_make().context("required GNU make was not found")?;
    let build_args = [format!("-j{jobs}")];
    let mut build =
        ffmpeg_make_command(&make, &layout.ffmpeg_build_dir, &build_args, options.target)?;
    apply_windows_target_env(&mut build, options.target)?;
    apply_android_host_env(&mut build, options.target)?;
    apply_windows_posix_shell(&mut build, options.target);
    append_windows_posix_paths(&mut build);
    run(&mut build)?;
    let install_args = ["install".to_string()];
    let mut install = ffmpeg_make_command(
        &make,
        &layout.ffmpeg_build_dir,
        &install_args,
        options.target,
    )?;
    apply_windows_target_env(&mut install, options.target)?;
    apply_android_host_env(&mut install, options.target)?;
    apply_windows_posix_shell(&mut install, options.target);
    append_windows_posix_paths(&mut install);
    run(&mut install)?;
    ensure_windows_link_aliases(
        options.target,
        &layout.ffmpeg_prefix,
        &[
            ("libavdevice.a", "avdevice.lib"),
            ("libavfilter.a", "avfilter.lib"),
            ("libavformat.a", "avformat.lib"),
            ("libavcodec.a", "avcodec.lib"),
            ("libswresample.a", "swresample.lib"),
            ("libswscale.a", "swscale.lib"),
            ("libavutil.a", "avutil.lib"),
        ],
    )?;

    let ffmpeg_patchset = ffmpeg_patchset_id(&layout.root)?;
    fs::write(
        &layout.ffmpeg_build_marker,
        format!(
            "ffmpeg={FFMPEG_VERSION}\npatchset={}\nzlib={ZLIB_VERSION}\ndav1d={}\nprofile={}\ntarget={}\nandroid_api={}\nprefix={}\nflags={}\n",
            ffmpeg_patchset,
            if options.target.is_android() || options.target.is_apple() {
                DAV1D_VERSION
            } else {
                "n/a"
            },
            profile_name(options.profile),
            options.target.triple().unwrap_or("host"),
            if options.target.is_android() {
                android_api_level()?.to_string()
            } else {
                "n/a".to_string()
            },
            layout.ffmpeg_prefix.display(),
            options
                .profile
                .ffmpeg_configure_flags_for_target(options.target)
                .join(" ")
        ),
    )
    .with_context(|| format!("write {}", layout.ffmpeg_build_marker.display()))?;
    Ok(())
}

fn enable_ffmpeg_archive_response_files(build_dir: &Path) -> Result<()> {
    let makefile = build_dir.join("ffbuild/library.mak");
    let contents =
        fs::read_to_string(&makefile).with_context(|| format!("read {}", makefile.display()))?;
    let original = "\t$(AR) $(ARFLAGS) $(AR_O) $^";
    let replacement = concat!(
        "\t$(file >$@.rsp,$(ARFLAGS) $(AR_O) $^)\n",
        "\t$(AR) @$@.rsp\n",
        "\t$(RM) $@.rsp"
    );
    if contents.contains(replacement) {
        return Ok(());
    }
    if !contents.contains(original) {
        bail!(
            "FFmpeg archive recipe was not found in {}; cannot enable Windows response files",
            makefile.display()
        );
    }
    let patched = contents.replacen(original, replacement, 1);
    fs::write(&makefile, patched).with_context(|| {
        format!(
            "write response-file archive recipe to {}",
            makefile.display()
        )
    })?;
    println!("patched FFmpeg static archive recipe to use llvm-ar response files on Windows");
    Ok(())
}

struct AppleToolchain {
    clang: PathBuf,
    clangxx: PathBuf,
    ar: PathBuf,
    ranlib: PathBuf,
    strip: PathBuf,
    sdk_root: PathBuf,
    arch: &'static str,
    deployment_flag: &'static str,
    deployment_target: String,
}

#[derive(Debug, Clone)]
struct AndroidToolchain {
    ndk_root: PathBuf,
    bin_dir: PathBuf,
    sysroot: PathBuf,
    cmake_toolchain_file: PathBuf,
    clang: PathBuf,
    clangxx: PathBuf,
    ar: PathBuf,
    ranlib: PathBuf,
    strip: PathBuf,
    nm: PathBuf,
    arch: &'static str,
    abi: &'static str,
    api_level: u32,
}

fn apple_toolchain(target: NativeTarget) -> Result<Option<AppleToolchain>> {
    let Some(sdk) = target.sdk() else {
        return Ok(None);
    };
    let sdk_root = PathBuf::from(xcrun(sdk, &["--show-sdk-path"])?);
    let (deployment_target, deployment_flag) = target
        .deployment_target()
        .context("explicit Apple target must have a deployment target")?;
    Ok(Some(AppleToolchain {
        clang: PathBuf::from(xcrun(sdk, &["-f", "clang"])?),
        clangxx: PathBuf::from(xcrun(sdk, &["-f", "clang++"])?),
        ar: PathBuf::from(xcrun(sdk, &["-f", "ar"])?),
        ranlib: PathBuf::from(xcrun(sdk, &["-f", "ranlib"])?),
        strip: PathBuf::from(xcrun(sdk, &["-f", "strip"])?),
        sdk_root,
        arch: target
            .ffmpeg_arch()
            .context("explicit Apple target must have an FFmpeg arch")?,
        deployment_flag,
        deployment_target,
    }))
}

fn android_toolchain(target: NativeTarget) -> Result<Option<AndroidToolchain>> {
    if !target.is_android() || matches!(target, NativeTarget::Host) {
        return Ok(None);
    }
    let ndk_root = android_ndk_root()?;
    let host_tag = android_ndk_host_tag(&ndk_root)?;
    let bin_dir = ndk_root
        .join("toolchains/llvm/prebuilt")
        .join(&host_tag)
        .join("bin");
    let sysroot = ndk_root
        .join("toolchains/llvm/prebuilt")
        .join(&host_tag)
        .join("sysroot");
    let cmake_toolchain_file = ndk_root.join("build/cmake/android.toolchain.cmake");
    let api_level = android_api_level()?;
    let clang_triple = target
        .android_clang_triple()
        .context("explicit Android target must have a Clang triple")?;
    let clang = required_executable_in_dir(
        &bin_dir,
        &format!("{clang_triple}{api_level}-clang"),
        "Android NDK C compiler",
    )?;
    let clangxx = required_executable_in_dir(
        &bin_dir,
        &format!("{clang_triple}{api_level}-clang++"),
        "Android NDK C++ compiler",
    )?;
    let ar = required_executable_in_dir(&bin_dir, "llvm-ar", "Android NDK archiver")?;
    let ranlib = required_executable_in_dir(&bin_dir, "llvm-ranlib", "Android NDK ranlib")?;
    let strip = required_executable_in_dir(&bin_dir, "llvm-strip", "Android NDK strip")?;
    let nm = required_executable_in_dir(&bin_dir, "llvm-nm", "Android NDK nm")?;
    if !sysroot.is_dir() || !cmake_toolchain_file.is_file() {
        bail!(
            "Android NDK at {} is incomplete (missing sysroot or android.toolchain.cmake)",
            ndk_root.display()
        );
    }
    Ok(Some(AndroidToolchain {
        ndk_root,
        bin_dir,
        sysroot,
        cmake_toolchain_file,
        clang,
        clangxx,
        ar,
        ranlib,
        strip,
        nm,
        arch: target
            .ffmpeg_arch()
            .context("explicit Android target must have an FFmpeg arch")?,
        abi: target
            .android_abi()
            .context("explicit Android target must have an ABI")?,
        api_level,
    }))
}

fn ffmpeg_android_host_cc(config: &AndroidToolchain) -> Result<Option<String>> {
    if !cfg!(windows) {
        return Ok(None);
    }
    let clang =
        required_executable_in_dir(&config.bin_dir, "clang", "Android NDK host Clang compiler")?;
    Ok(Some(format!(
        "{} --target=x86_64-pc-windows-msvc -fuse-ld=link",
        ffmpeg_flag_path_arg(&clang)
    )))
}

fn android_api_level() -> Result<u32> {
    let value = env::var("ANDROID_API_LEVEL")
        .ok()
        .map(|value| {
            value
                .trim_start_matches("android-")
                .parse::<u32>()
                .with_context(|| format!("ANDROID_API_LEVEL must be an integer, got `{value}`"))
        })
        .transpose()?
        .unwrap_or(DEFAULT_ANDROID_API_LEVEL);
    if value < DEFAULT_ANDROID_API_LEVEL {
        bail!(
            "Android API level {value} is unsupported; Erika requires API {} or newer",
            DEFAULT_ANDROID_API_LEVEL
        );
    }
    Ok(value)
}

fn android_ndk_root() -> Result<PathBuf> {
    let mut candidates = Vec::new();
    for variable in ["ANDROID_NDK_HOME", "ANDROID_NDK_ROOT"] {
        if let Some(path) = env::var_os(variable) {
            candidates.push(PathBuf::from(path));
        }
    }
    for variable in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Some(path) = env::var_os(variable) {
            add_android_sdk_ndk_candidates(&mut candidates, &PathBuf::from(path));
        }
    }
    if cfg!(windows) {
        if let Some(local_app_data) = env::var_os("LOCALAPPDATA") {
            add_android_sdk_ndk_candidates(
                &mut candidates,
                &PathBuf::from(local_app_data).join("Android/Sdk"),
            );
        }
    }
    if let Some(path) = candidates.into_iter().find(|path| {
        path.join("build/cmake/android.toolchain.cmake").is_file()
            && path.join("toolchains/llvm/prebuilt").is_dir()
    }) {
        return Ok(path);
    }
    bail!(
        "Android NDK was not found. Set ANDROID_NDK_HOME/ANDROID_NDK_ROOT, or install a side-by-side NDK under ANDROID_HOME/ndk"
    )
}

fn add_android_sdk_ndk_candidates(candidates: &mut Vec<PathBuf>, sdk_root: &Path) {
    let ndk_dir = sdk_root.join("ndk");
    if let Ok(entries) = fs::read_dir(&ndk_dir) {
        let mut versions = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect::<Vec<_>>();
        versions.sort_by_key(|path| {
            path.file_name()
                .map(|name| android_ndk_version_key(&name.to_string_lossy()))
                .unwrap_or_default()
        });
        versions.reverse();
        candidates.extend(versions);
    }
    candidates.push(sdk_root.join("ndk-bundle"));
}

fn android_ndk_version_key(value: &str) -> Vec<u32> {
    value
        .split(|ch: char| !ch.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse().ok())
        .collect()
}

fn android_ndk_host_tag(ndk_root: &Path) -> Result<String> {
    let prebuilt = ndk_root.join("toolchains/llvm/prebuilt");
    let preferred: &[&str] = if cfg!(windows) {
        &["windows-x86_64"]
    } else if cfg!(target_os = "macos") {
        &["darwin-aarch64", "darwin-x86_64"]
    } else {
        &["linux-x86_64"]
    };
    for tag in preferred {
        if prebuilt.join(tag).is_dir() {
            return Ok((*tag).to_string());
        }
    }
    let available = fs::read_dir(&prebuilt)
        .with_context(|| format!("read Android NDK prebuilt directory {}", prebuilt.display()))?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect::<Vec<_>>();
    bail!(
        "Android NDK at {} has no toolchain for this host (available: {})",
        ndk_root.display(),
        available.join(", ")
    )
}

fn required_executable_in_dir(dir: &Path, name: &str, description: &str) -> Result<PathBuf> {
    let executable = executable_in_dir(dir, name);
    if executable.is_file() {
        Ok(executable)
    } else {
        bail!("{description} was not found under {}", dir.display())
    }
}

fn ffmpeg_flag_path_arg(path: &Path) -> String {
    shell_escape(&path.to_string_lossy().replace('\\', "/"))
}

fn ffmpeg_build_marker_is_current(layout: &WorkspaceLayout, options: DepsOptions) -> bool {
    let Ok(marker) = fs::read_to_string(&layout.ffmpeg_build_marker) else {
        return false;
    };
    let android_api_is_current = if options.target.is_android() {
        android_api_level().is_ok_and(|api| marker.contains(&format!("android_api={api}\n")))
    } else {
        marker.contains("android_api=n/a\n")
    };
    let patchset_is_current = ffmpeg_patchset_id(&layout.root).is_ok_and(|patchset| {
        ffmpeg_build_marker_has_current_patchset(&marker, options.target, &patchset)
    });
    marker.contains(&format!("ffmpeg={FFMPEG_VERSION}\n"))
        && marker.contains(&format!("profile={}\n", profile_name(options.profile)))
        && marker.contains(&format!(
            "target={}\n",
            options.target.triple().unwrap_or("host")
        ))
        && marker.contains(&format!("zlib={ZLIB_VERSION}\n"))
        && marker.contains(&format!(
            "dav1d={}\n",
            if options.target.is_android() || options.target.is_apple() {
                DAV1D_VERSION
            } else {
                "n/a"
            }
        ))
        && ffmpeg_build_marker_has_current_flags(&marker, options.profile, options.target)
        && patchset_is_current
        && android_api_is_current
}

fn dav1d_build_marker_is_current(layout: &WorkspaceLayout, options: DepsOptions) -> bool {
    if !options.target.is_android() && !options.target.is_apple() {
        return true;
    }
    let Ok(marker) = fs::read_to_string(&layout.dav1d_build_marker) else {
        return false;
    };
    let api_is_current = if options.target.is_android() {
        android_api_level().is_ok_and(|api| marker.contains(&format!("android_api={api}\n")))
    } else {
        marker.contains("android_api=n/a\n")
    };
    api_is_current && {
        marker.contains(&format!("dav1d={DAV1D_VERSION}\n"))
            && marker.contains(&format!(
                "target={}\n",
                options.target.triple().unwrap_or("host")
            ))
            && marker.contains(&format!("asm={}\n", dav1d_asm_enabled(options.target)))
    }
}

fn ffmpeg_build_marker_has_current_flags(
    marker: &str,
    profile: NativeDependencyProfile,
    target: NativeTarget,
) -> bool {
    let expected = profile.ffmpeg_configure_flags_for_target(target).join(" ");
    marker
        .lines()
        .find_map(|line| line.strip_prefix("flags="))
        .is_some_and(|flags| flags == expected)
}

fn ffmpeg_build_marker_has_current_patchset(
    marker: &str,
    target: NativeTarget,
    expected: &str,
) -> bool {
    !target.is_android()
        || marker
            .lines()
            .find_map(|line| line.strip_prefix("patchset="))
            .is_some_and(|patchset| patchset == expected)
}

fn shell_escape(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | ':' | '.' | '_' | '-'))
    {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn write_profile_metadata(
    layout: &WorkspaceLayout,
    profile: NativeDependencyProfile,
    target: NativeTarget,
) -> Result<()> {
    fs::create_dir_all(&layout.dist_dir)
        .with_context(|| format!("create {}", layout.dist_dir.display()))?;
    let ffmpeg_patchset = ffmpeg_patchset_id(&layout.root)?;
    fs::write(
        layout.dist_dir.join("erika-native-deps.txt"),
        format!(
            "profile={}\ntarget={}\nandroid_api={}\nffmpeg={}\nffmpeg_patchset={}\nffmpeg_dist={}\ndav1d={}\ndav1d_dist={}\nzlib={}\nzlib_dist={}\nlibass={}\nlibass_source={}\nharfbuzz={}\nharfbuzz_source={}\nfreetype={}\nfreetype_source={}\nfribidi={}\nfribidi_source={}\n",
            profile_name(profile),
            target.triple().unwrap_or("host"),
            if target.is_android() {
                android_api_level()?.to_string()
            } else {
                "n/a".to_string()
            },
            FFMPEG_VERSION,
            ffmpeg_patchset,
            layout.ffmpeg_prefix.display(),
            if target.is_android() || target.is_apple() {
                DAV1D_VERSION
            } else {
                "n/a"
            },
            if target.is_android() || target.is_apple() {
                layout.dav1d_prefix.display().to_string()
            } else {
                "n/a".to_string()
            },
            ZLIB_VERSION,
            layout.zlib_prefix.display(),
            LIBASS_VERSION,
            source_state(&layout.libass_source_dir),
            HARFBUZZ_VERSION,
            source_state(&layout.harfbuzz_source_dir),
            FREETYPE_VERSION,
            source_state(&layout.freetype_source_dir),
            FRIBIDI_VERSION,
            source_state(&layout.fribidi_source_dir)
        ),
    )
    .with_context(|| format!("write metadata in {}", layout.dist_dir.display()))?;
    Ok(())
}

fn check_license_policy() -> Result<()> {
    let root = workspace_root()?;
    let manifest = fs::read_to_string(root.join("crates/erika_ffmpeg_sys/Cargo.toml"))
        .context("read erika_ffmpeg_sys manifest")?;
    if !manifest.contains("default = [\"lgpl\"]") {
        bail!("erika_ffmpeg_sys default feature must be exactly lgpl");
    }
    if !NativeDependencyProfile::Lgpl
        .ffmpeg_configure_flags()
        .contains(&"--disable-gpl")
    {
        bail!("LGPL profile must pass --disable-gpl");
    }
    if NativeDependencyProfile::Lgpl
        .ffmpeg_configure_flags()
        .contains(&"--enable-gpl")
    {
        bail!("LGPL profile must not pass --enable-gpl");
    }
    if !NativeDependencyProfile::GplFull
        .ffmpeg_configure_flags()
        .contains(&"--enable-gpl")
    {
        bail!("gpl-full profile must explicitly pass --enable-gpl");
    }
    let notices = fs::read_to_string(root.join("packaging/THIRD_PARTY_NOTICES.md"))
        .context("read third-party notices")?;
    if !notices.contains("| dav1d | 1.5.x | BSD 2-Clause |")
        || !root.join("packaging/LICENSE.dav1d").is_file()
    {
        bail!("dav1d BSD 2-Clause attribution must ship with release bundles");
    }
    println!("license policy ok: default=lgpl, gpl-full is opt-in, dav1d BSD notice present");
    Ok(())
}

fn workspace_root() -> Result<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(PathBuf::from)
        .context("xtask manifest has no parent")
}

fn profile_name(profile: NativeDependencyProfile) -> &'static str {
    match profile {
        NativeDependencyProfile::Lgpl => "lgpl",
        NativeDependencyProfile::GplFull => "gpl-full",
    }
}

fn default_job_count() -> usize {
    std::thread::available_parallelism()
        .map_or(4, usize::from)
        .max(1)
}

fn status_word(ok: bool) -> &'static str {
    if ok { "ready" } else { "missing" }
}

fn source_state(path: &std::path::Path) -> &'static str {
    status_word(path.exists())
}

fn native_static_lib_exists(prefix: &Path, name: &str) -> bool {
    let lib_dir = prefix.join("lib");
    [
        format!("lib{name}.a"),
        format!("{name}.lib"),
        format!("lib{name}.lib"),
    ]
    .into_iter()
    .any(|file| lib_dir.join(file).exists())
}

fn smoke_ffmpeg_make(options: DepsOptions) -> Result<()> {
    if !options.target.is_windows() {
        bail!("deps smoke-ffmpeg-make only applies to the Windows FFmpeg/MSYS make path");
    }

    let make = gnu_make().context("required GNU make was not found")?;
    let smoke_dir = env::temp_dir().join(format!("erika-ffmpeg-make-smoke-{}", std::process::id()));
    if smoke_dir.exists() {
        fs::remove_dir_all(&smoke_dir)
            .with_context(|| format!("remove {}", smoke_dir.display()))?;
    }
    fs::create_dir_all(&smoke_dir).with_context(|| format!("create {}", smoke_dir.display()))?;
    let root = workspace_root()?;
    fs::write(
        smoke_dir.join("header_smoke.cpp"),
        "#include \"crates/erika_capi/include/erika.h\"\nint main() { return 0; }\n",
    )
    .with_context(|| format!("write {}", smoke_dir.join("header_smoke.cpp").display()))?;
    fs::write(
        smoke_dir.join("Makefile"),
        format!(
            "all:\n\t@echo cwd=$(CURDIR)\n\t@test -f Makefile\n\t@command -v cl.exe >/dev/null\n\t@awk 'BEGIN {{ s = \"C:\\\\tmp\\\\file\"; gsub(/\\\\/, \"/\", s); if (s != \"C:/tmp/file\") exit 42 }}'\n\t@cl.exe /nologo /TP /std:c++17 /I\"{}\" /c header_smoke.cpp\n\t@echo erika-ffmpeg-make-smoke-ok\n",
            path_to_forward_slashes(&root)
        ),
    )
    .with_context(|| format!("write {}", smoke_dir.join("Makefile").display()))?;

    let args = ["all".to_string()];
    let mut command = ffmpeg_make_command(&make, &smoke_dir, &args, options.target)?;
    apply_windows_target_env(&mut command, options.target)?;
    apply_windows_posix_shell(&mut command, options.target);
    append_windows_posix_paths(&mut command);
    let result = run(&mut command);
    let _ = fs::remove_dir_all(&smoke_dir);
    result
}

fn ensure_windows_link_aliases(
    target: NativeTarget,
    prefix: &Path,
    aliases: &[(&str, &str)],
) -> Result<()> {
    if !target.is_windows() {
        return Ok(());
    }
    let lib_dir = prefix.join("lib");
    for (source, alias) in aliases {
        let source = lib_dir.join(source);
        let alias = lib_dir.join(alias);
        if alias.exists() || !source.exists() {
            continue;
        }
        fs::copy(&source, &alias)
            .with_context(|| format!("copy {} to {}", source.display(), alias.display()))?;
    }
    Ok(())
}

fn ensure_windows_zlib_static_alias(target: NativeTarget, prefix: &Path) -> Result<()> {
    if !target.is_windows() {
        return Ok(());
    }
    let lib_dir = prefix.join("lib");
    let import_lib = lib_dir.join("zlib.lib");
    if import_lib.exists() {
        return Ok(());
    }
    for source in ["zlibs.lib", "zlibstatic.lib", "z.lib"] {
        let static_lib = lib_dir.join(source);
        if static_lib.exists() {
            fs::copy(&static_lib, &import_lib).with_context(|| {
                format!("copy {} to {}", static_lib.display(), import_lib.display())
            })?;
            break;
        }
    }
    Ok(())
}

fn ensure_windows_zlib_header_compat(target: NativeTarget, prefix: &Path) -> Result<()> {
    if !target.is_windows() {
        return Ok(());
    }
    let header = prefix.join("include").join("zconf.h");
    if !header.exists() {
        return Ok(());
    }
    let content =
        fs::read_to_string(&header).with_context(|| format!("read {}", header.display()))?;
    if content.contains("#if defined(HAVE_UNISTD_H) && !defined(_WIN32)") {
        return Ok(());
    }

    let updated = content.replace(
        "#ifdef HAVE_UNISTD_H    /* may be set to #if 1 by ./configure */",
        "#if defined(HAVE_UNISTD_H) && !defined(_WIN32)    /* may be set to #if 1 by ./configure */",
    );
    if updated == content {
        println!(
            "warning: zlib header compatibility patch not applied to {}",
            header.display()
        );
        return Ok(());
    }
    fs::write(&header, updated).with_context(|| format!("write {}", header.display()))?;
    Ok(())
}

fn ensure_pkg_config_shim(layout: &WorkspaceLayout) -> Result<PathBuf> {
    if !cfg!(windows) {
        return which("pkg-config").context("required pkg-config was not found in PATH");
    }
    let dir = layout.build_dir.join("pkg-config-shim");
    fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let exe = env::current_exe().context("resolve current xtask executable")?;
    let shim = dir.join("pkg-config.cmd");
    let root_from_shim = windows_cmd_parent_traversal(&layout.root, &dir)?;
    let dist_from_root = windows_cmd_path_under_root(&layout.root, &layout.dist_dir)?;
    let exe_command = if let Ok(exe_from_root) = exe.strip_prefix(&layout.root) {
        format!("\"%ERIKA_ROOT%\\{}\"", windows_cmd_path(exe_from_root))
    } else {
        format!("\"{}\"", exe.display())
    };
    fs::write(
        &shim,
        format!(
            "@echo off\r\n\
             setlocal\r\n\
             for %%I in (\"%~dp0{}\") do set \"ERIKA_ROOT=%%~fI\"\r\n\
             set \"ERIKA_DIST_DIR=%ERIKA_ROOT%\\{}\"\r\n\
             set \"ERIKA_PKG_CONFIG_PATH=%ERIKA_DIST_DIR%\\dav1d\\lib\\pkgconfig;%ERIKA_DIST_DIR%\\ffmpeg\\lib\\pkgconfig;%ERIKA_DIST_DIR%\\freetype\\lib\\pkgconfig;%ERIKA_DIST_DIR%\\harfbuzz\\lib\\pkgconfig;%ERIKA_DIST_DIR%\\fribidi\\lib\\pkgconfig;%ERIKA_DIST_DIR%\\libass\\lib\\pkgconfig\"\r\n\
             if defined PKG_CONFIG_PATH (\r\n\
             \tset \"PKG_CONFIG_PATH=%ERIKA_PKG_CONFIG_PATH%;%PKG_CONFIG_PATH%\"\r\n\
             ) else (\r\n\
             \tset \"PKG_CONFIG_PATH=%ERIKA_PKG_CONFIG_PATH%\"\r\n\
             )\r\n\
             {} pkg-config-shim %*\r\n\
             exit /b %ERRORLEVEL%\r\n",
            root_from_shim, dist_from_root, exe_command
        ),
    )
    .with_context(|| format!("write {}", shim.display()))?;
    Ok(shim)
}

fn windows_cmd_parent_traversal(root: &Path, dir: &Path) -> Result<String> {
    let rel = dir
        .strip_prefix(root)
        .with_context(|| format!("{} is not under {}", dir.display(), root.display()))?;
    let depth = rel.components().count();
    if depth == 0 {
        Ok(".".to_string())
    } else {
        Ok(std::iter::repeat_n("..", depth)
            .collect::<Vec<_>>()
            .join("\\"))
    }
}

fn windows_cmd_path_under_root(root: &Path, path: &Path) -> Result<String> {
    let rel = path
        .strip_prefix(root)
        .with_context(|| format!("{} is not under {}", path.display(), root.display()))?;
    Ok(windows_cmd_path(rel))
}

fn windows_cmd_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("\\")
}

fn pkg_config_shim(args: Vec<String>) -> Result<()> {
    let query = PkgConfigQuery::parse(args);
    if query.version {
        println!("2.0.0-erika");
        return Ok(());
    }
    if query.packages.is_empty() {
        return Ok(());
    }

    let mut visited = HashSet::new();
    let mut output = Vec::new();
    for package in &query.packages {
        let pc = load_pc_file(package)?;
        if query.exists {
            continue;
        }
        if query.modversion {
            output.push(pc.value("Version"));
        }
        if let Some(variable) = &query.variable {
            output.push(pc.variable(variable));
        }
        if query.cflags {
            collect_pc_flags(
                &pc,
                PkgFlagKind::Cflags,
                query.static_link,
                query.msvc_syntax,
                &mut visited,
                &mut output,
            )?;
        }
        if query.libs {
            collect_pc_flags(
                &pc,
                PkgFlagKind::Libs,
                query.static_link,
                query.msvc_syntax,
                &mut visited,
                &mut output,
            )?;
        }
    }

    if !output.is_empty() {
        println!("{}", output.join(" "));
    }
    Ok(())
}

#[derive(Debug, Default)]
struct PkgConfigQuery {
    version: bool,
    exists: bool,
    modversion: bool,
    cflags: bool,
    libs: bool,
    static_link: bool,
    msvc_syntax: bool,
    variable: Option<String>,
    packages: Vec<String>,
}

impl PkgConfigQuery {
    fn parse(args: Vec<String>) -> Self {
        let mut query = Self::default();
        for arg in args {
            match arg.as_str() {
                "--version" => query.version = true,
                "--exists" => query.exists = true,
                "--modversion" => query.modversion = true,
                "--cflags" | "--cflags-only-I" | "--cflags-only-other" => query.cflags = true,
                "--libs" | "--libs-only-L" | "--libs-only-l" | "--libs-only-other" => {
                    query.libs = true
                }
                "--static" => query.static_link = true,
                "--msvc-syntax" => query.msvc_syntax = true,
                "--print-errors" | "--silence-errors" | "--short-errors" | "--errors-to-stdout" => {
                }
                _ if arg.starts_with("--variable=") => {
                    query.variable = Some(arg["--variable=".len()..].to_string());
                }
                _ if arg.starts_with("--") => {}
                ">" | ">=" | "=" | "<=" | "<" => {}
                value if looks_like_version(value) => {}
                value => query.packages.push(value.to_string()),
            }
        }
        if !query.exists
            && !query.modversion
            && !query.cflags
            && !query.libs
            && query.variable.is_none()
            && !query.packages.is_empty()
        {
            query.cflags = true;
            query.libs = true;
        }
        query
    }
}

#[derive(Debug, Clone)]
struct PcFile {
    name: String,
    variables: HashMap<String, String>,
    fields: HashMap<String, String>,
}

impl PcFile {
    fn value(&self, key: &str) -> String {
        self.fields
            .get(key)
            .map(|value| substitute_pc_vars(value, &self.variables))
            .unwrap_or_default()
    }

    fn variable(&self, key: &str) -> String {
        self.variables.get(key).cloned().unwrap_or_default()
    }

    fn flag_tokens(&self, key: &str) -> Vec<String> {
        self.fields
            .get(key)
            .into_iter()
            .flat_map(|field| split_pc_field_tokens(field))
            .map(|token| unescape_pc_whitespace(&substitute_pc_vars(&token, &self.variables)))
            .filter(|token| !token.is_empty())
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PkgFlagKind {
    Cflags,
    Libs,
}

fn collect_pc_flags(
    pc: &PcFile,
    kind: PkgFlagKind,
    static_link: bool,
    msvc_syntax: bool,
    visited: &mut HashSet<String>,
    output: &mut Vec<String>,
) -> Result<()> {
    let visit_key = format!("{}:{kind:?}", pc.name);
    if !visited.insert(visit_key) {
        return Ok(());
    }

    let mut fields = match kind {
        PkgFlagKind::Cflags => vec!["Cflags"],
        PkgFlagKind::Libs => vec!["Libs"],
    };
    if kind == PkgFlagKind::Libs && static_link {
        fields.push("Libs.private");
    }
    for field in fields {
        output.extend(
            pc.flag_tokens(field)
                .into_iter()
                .map(|token| format_pkg_config_token(token, msvc_syntax)),
        );
    }

    for required in pc_requirements(pc, static_link) {
        let required_pc = load_pc_file(&required)?;
        collect_pc_flags(
            &required_pc,
            kind,
            static_link,
            msvc_syntax,
            visited,
            output,
        )?;
    }
    Ok(())
}

fn pc_requirements(pc: &PcFile, static_link: bool) -> Vec<String> {
    let mut fields = vec![pc.value("Requires")];
    if static_link {
        fields.push(pc.value("Requires.private"));
    }
    fields
        .into_iter()
        .flat_map(|field| {
            field
                .split(',')
                .filter_map(|entry| entry.split_whitespace().next().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .filter(|name| !name.is_empty())
        .collect()
}

fn load_pc_file(package: &str) -> Result<PcFile> {
    let pkg_config_path = env::var_os("PKG_CONFIG_PATH").context("PKG_CONFIG_PATH is not set")?;
    for dir in env::split_paths(&pkg_config_path) {
        let path = dir.join(format!("{package}.pc"));
        if path.exists() {
            return parse_pc_file(package, &path);
        }
    }
    bail!("pkg-config package `{package}` was not found")
}

fn parse_pc_file(package: &str, path: &Path) -> Result<PcFile> {
    let contents = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let mut variables = HashMap::new();
    let mut fields = HashMap::new();
    variables.insert(
        "pcfiledir".to_string(),
        path.parent().unwrap_or(Path::new("")).display().to_string(),
    );
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let equals = line.find('=');
        let colon = line.find(':');
        if let Some(index) = colon.filter(|index| equals.is_none_or(|equals| *index < equals)) {
            let (key, value) = line.split_at(index);
            fields.insert(key.trim().to_string(), value[1..].trim().to_string());
        } else if let Some((key, value)) = line.split_once('=') {
            let value = unescape_pc_whitespace(&substitute_pc_vars(value.trim(), &variables));
            variables.insert(key.trim().to_string(), value);
        }
    }
    Ok(PcFile {
        name: package.to_string(),
        variables,
        fields,
    })
}

fn substitute_pc_vars(value: &str, variables: &HashMap<String, String>) -> String {
    let mut output = value.to_string();
    for _ in 0..8 {
        let Some(start) = output.find("${") else {
            break;
        };
        let Some(end) = output[start + 2..].find('}') else {
            break;
        };
        let end = start + 2 + end;
        let name = &output[start + 2..end];
        let replacement = variables.get(name).cloned().unwrap_or_default();
        output.replace_range(start..=end, &replacement);
    }
    output
}

fn split_pc_field_tokens(value: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut escaped = false;
    for ch in value.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch.is_whitespace() {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
        } else {
            current.push(ch);
        }
    }
    if escaped {
        current.push('\\');
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn unescape_pc_whitespace(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut escaped = false;
    for ch in value.chars() {
        if escaped {
            output.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else {
            output.push(ch);
        }
    }
    if escaped {
        output.push('\\');
    }
    output
}

fn format_pkg_config_token(token: String, msvc_syntax: bool) -> String {
    let token = if let Some(path) = token.strip_prefix("-I") {
        let path = pkg_config_output_path(path);
        if msvc_syntax {
            format!("/I{path}")
        } else {
            format!("-I{path}")
        }
    } else if let Some(path) = token.strip_prefix("-L") {
        let path = pkg_config_output_path(path);
        if msvc_syntax {
            format!("/libpath:{path}")
        } else {
            format!("-L{path}")
        }
    } else if msvc_syntax {
        msvc_pkg_config_token(token)
    } else {
        token
    };
    escape_pkg_config_token(&token)
}

fn pkg_config_output_path(path: &str) -> String {
    let path = path.replace('\\', "/");
    if let Some(relative) = relative_pkg_config_path(&path) {
        return relative;
    }
    path
}

fn relative_pkg_config_path(path: &str) -> Option<String> {
    let base = env::var_os("ERIKA_PKG_CONFIG_RELATIVE_BASE")?;
    let path = PathBuf::from(path);
    if !path.is_absolute() {
        return None;
    }
    relative_path(Path::new(&base), &path).map(|path| path_to_forward_slashes(&path))
}

fn relative_path(base: &Path, path: &Path) -> Option<PathBuf> {
    let base_components = base.components().collect::<Vec<_>>();
    let path_components = path.components().collect::<Vec<_>>();
    let mut common = 0;
    while common < base_components.len()
        && common < path_components.len()
        && windows_component_eq(base_components[common], path_components[common])
    {
        common += 1;
    }
    if common == 0 {
        return None;
    }
    let mut relative = PathBuf::new();
    for component in &base_components[common..] {
        if matches!(component, std::path::Component::Normal(_)) {
            relative.push("..");
        }
    }
    for component in &path_components[common..] {
        relative.push(component.as_os_str());
    }
    Some(relative)
}

fn windows_component_eq(left: std::path::Component<'_>, right: std::path::Component<'_>) -> bool {
    left.as_os_str()
        .to_string_lossy()
        .eq_ignore_ascii_case(&right.as_os_str().to_string_lossy())
}

fn path_to_forward_slashes(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn escape_pkg_config_token(token: &str) -> String {
    token
        .chars()
        .flat_map(|ch| {
            if ch.is_whitespace() {
                vec!['\\', ch]
            } else {
                vec![ch]
            }
        })
        .collect()
}

fn msvc_pkg_config_token(token: String) -> String {
    if let Some(path) = token.strip_prefix("-I") {
        format!("/I{path}")
    } else if let Some(path) = token.strip_prefix("-L") {
        format!("/libpath:{path}")
    } else if let Some(name) = token.strip_prefix("-l") {
        format!("{name}.lib")
    } else {
        token
    }
}

fn looks_like_version(value: &str) -> bool {
    value.chars().next().is_some_and(|ch| ch.is_ascii_digit())
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
}

static WINDOWS_MSVC_ENV: OnceLock<std::result::Result<Vec<(OsString, OsString)>, String>> =
    OnceLock::new();

fn apply_windows_target_env(command: &mut Command, target: NativeTarget) -> Result<()> {
    if !target.is_windows() {
        return Ok(());
    }
    apply_msvc_environment(command)
}

fn apply_android_host_env(command: &mut Command, target: NativeTarget) -> Result<()> {
    if !cfg!(windows) || !target.is_android() {
        return Ok(());
    }
    apply_msvc_environment(command)
}

fn apply_msvc_environment(command: &mut Command) -> Result<()> {
    let existing_path = command_env_path(command);
    for (key, value) in windows_msvc_environment()? {
        command.env(key, value);
    }
    if let Some(existing_path) = existing_path {
        let existing_dirs = env::split_paths(&existing_path).collect::<Vec<_>>();
        // Keep VSDevCmd's PATH first so MSVC link.exe wins over POSIX tools such as MSYS link.exe.
        append_paths_to_command(command, existing_dirs.iter().map(PathBuf::as_path));
    }
    Ok(())
}

fn windows_msvc_environment() -> Result<&'static [(OsString, OsString)]> {
    match WINDOWS_MSVC_ENV
        .get_or_init(|| load_windows_msvc_environment().map_err(|e| e.to_string()))
    {
        Ok(values) => Ok(values.as_slice()),
        Err(message) => bail!("{message}"),
    }
}

fn load_windows_msvc_environment() -> Result<Vec<(OsString, OsString)>> {
    let devcmd = vs_dev_cmd().context("Visual Studio Developer Command Prompt was not found")?;
    let script_path = env::temp_dir().join("erika-vsdevcmd-env.cmd");
    fs::write(
        &script_path,
        format!(
            "@echo off\r\ncall \"{}\" -arch=x64 -host_arch=x64 >nul\r\nset\r\n",
            devcmd.display()
        ),
    )
    .with_context(|| format!("write {}", script_path.display()))?;
    let output = Command::new("cmd.exe")
        .arg("/d")
        .arg("/c")
        .arg(&script_path)
        .output()
        .context("spawn Visual Studio Developer Command Prompt")?;
    let _ = fs::remove_file(&script_path);
    if !output.status.success() {
        bail!(
            "Visual Studio Developer Command Prompt failed with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut values = Vec::new();
    for line in stdout.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        values.push((OsString::from(key), OsString::from(value)));
    }
    if !values.iter().any(|(key, _)| {
        key.to_string_lossy()
            .eq_ignore_ascii_case("VCToolsInstallDir")
    }) {
        bail!("Visual Studio C++ tools are not installed in the Build Tools instance");
    }
    Ok(values)
}

fn vs_dev_cmd() -> Option<PathBuf> {
    let vswhere = which("vswhere").or_else(|| {
        existing_path("C:/Program Files (x86)/Microsoft Visual Studio/Installer/vswhere.exe")
    })?;
    let output = Command::new(vswhere)
        .args([
            "-latest",
            "-products",
            "*",
            "-requires",
            "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
            "-property",
            "installationPath",
        ])
        .output()
        .ok()?;
    if output.status.success() {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !path.is_empty() {
            let devcmd = PathBuf::from(path).join("Common7/Tools/VsDevCmd.bat");
            if devcmd.exists() {
                return Some(devcmd);
            }
        }
    }
    existing_path(
        "C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/Common7/Tools/VsDevCmd.bat",
    )
}

fn cmake_tool() -> Option<PathBuf> {
    which("cmake")
        .or_else(|| android_sdk_cmake_tool("cmake"))
        .or_else(|| {
            existing_path(
            "C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/Common7/IDE/CommonExtensions/Microsoft/CMake/CMake/bin/cmake.exe",
        )
        })
}

fn ninja_tool() -> Option<PathBuf> {
    which("ninja")
        .or_else(|| android_sdk_cmake_tool("ninja"))
        .or_else(|| {
            existing_path(
            "C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/Common7/IDE/CommonExtensions/Microsoft/CMake/Ninja/ninja.exe",
        )
        })
}

fn android_sdk_cmake_tool(tool: &str) -> Option<PathBuf> {
    let mut sdk_roots = ["ANDROID_HOME", "ANDROID_SDK_ROOT"]
        .into_iter()
        .filter_map(env::var_os)
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    if cfg!(windows) {
        if let Some(local_app_data) = env::var_os("LOCALAPPDATA") {
            sdk_roots.push(PathBuf::from(local_app_data).join("Android/Sdk"));
        }
    }
    for sdk_root in sdk_roots {
        let Ok(entries) = fs::read_dir(sdk_root.join("cmake")) else {
            continue;
        };
        let mut versions = entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect::<Vec<_>>();
        versions.sort_by_key(|path| {
            path.file_name()
                .map(|name| android_ndk_version_key(&name.to_string_lossy()))
                .unwrap_or_default()
        });
        versions.reverse();
        for version in versions {
            let candidate = executable_in_dir(&version.join("bin"), tool);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn posix_shell() -> Option<PathBuf> {
    existing_path("C:/msys64/usr/bin/sh.exe")
        .or_else(|| existing_path("C:/Program Files/Git/usr/bin/sh.exe"))
        .or_else(|| existing_path("C:/Program Files/Git/bin/bash.exe"))
        .or_else(|| which("sh"))
        .or_else(|| {
            which("bash").filter(|path| {
                !cfg!(windows)
                    || !path
                        .to_string_lossy()
                        .eq_ignore_ascii_case("C:\\Windows\\System32\\bash.exe")
            })
        })
}

fn gnu_make() -> Option<PathBuf> {
    existing_path("C:/msys64/usr/bin/make.exe")
        .or_else(|| which("make"))
        .or_else(|| which("gmake"))
        .or_else(|| which("mingw32-make"))
        .or_else(|| existing_path("C:/mingw64/bin/mingw32-make.exe"))
        .or_else(android_ndk_make)
}

fn android_ndk_make() -> Option<PathBuf> {
    let ndk_root = android_ndk_root().ok()?;
    let host_tag = android_ndk_host_tag(&ndk_root).ok()?;
    for bin_dir in [
        ndk_root.join("prebuilt").join(&host_tag).join("bin"),
        ndk_root
            .join("toolchains/llvm/prebuilt")
            .join(&host_tag)
            .join("bin"),
    ] {
        let candidate = executable_in_dir(&bin_dir, "make");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn ffmpeg_make_command(
    make: &Path,
    build_dir: &Path,
    args: &[String],
    target: NativeTarget,
) -> Result<Command> {
    if uses_windows_posix_ffmpeg(target) {
        let shell = posix_shell().context("required POSIX shell was not found for FFmpeg make")?;
        let mut command = Command::new(shell);
        let make_line = std::iter::once(format!(
            "cd {}",
            shell_quote(&path_to_forward_slashes(build_dir))
        ))
        .chain(std::iter::once(
            std::iter::once(shell_quote(&path_to_forward_slashes(make)))
                .chain(args.iter().map(|arg| shell_quote(arg)))
                .collect::<Vec<_>>()
                .join(" "),
        ))
        .collect::<Vec<_>>()
        .join(" && ");
        command
            .arg("-lc")
            .arg(make_line)
            .env("MSYS2_PATH_TYPE", "inherit")
            .env("MSYS2_ARG_CONV_EXCL", "*");
        return Ok(command);
    }

    let mut command = Command::new(make);
    command.current_dir(build_dir).args(args);
    Ok(command)
}

fn python_tool() -> Option<PathBuf> {
    ["python3", "python", "py"]
        .into_iter()
        .filter_map(which)
        .find(|path| python_candidate_is_usable(path))
}

fn python_candidate_is_usable(path: &Path) -> bool {
    if cfg!(windows)
        && path
            .to_string_lossy()
            .to_ascii_lowercase()
            .contains("\\windowsapps\\")
    {
        return false;
    }
    Command::new(path)
        .arg("-c")
        .arg("import venv")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn append_windows_posix_paths(command: &mut Command) {
    if !cfg!(windows) {
        return;
    }
    let dirs = [
        Path::new("C:/msys64/usr/bin"),
        Path::new("C:/Program Files/Git/usr/bin"),
        Path::new("C:/mingw64/bin"),
    ];
    append_paths_to_command(command, dirs.into_iter().filter(|path| path.exists()));
}

fn apply_windows_posix_shell(command: &mut Command, target: NativeTarget) {
    if !uses_windows_posix_ffmpeg(target) {
        return;
    }
    let Some(shell) = posix_shell() else {
        return;
    };
    command.env("CONFIG_SHELL", &shell);
    command.env("SHELL", &shell);
}

fn uses_windows_posix_ffmpeg(target: NativeTarget) -> bool {
    cfg!(windows) && (target.is_windows() || target.is_android())
}

fn host_c_compiler() -> Result<PathBuf> {
    host_compiler("CC_FOR_BUILD", &["cc", "clang", "gcc", "cl"])
        .context("a host C compiler is required for Meson build-machine generators")
}

fn host_cxx_compiler() -> Result<PathBuf> {
    host_compiler("CXX_FOR_BUILD", &["c++", "clang++", "g++", "cl"])
        .context("a host C++ compiler is required for Meson build-machine generators")
}

fn host_compiler(variable: &str, candidates: &[&str]) -> Option<PathBuf> {
    env::var_os(variable)
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .or_else(|| {
            if cfg!(windows) && candidates.contains(&"cl") {
                windows_host_cl_compiler()
            } else {
                None
            }
        })
        .or_else(|| candidates.iter().find_map(|candidate| which(candidate)))
}

fn windows_host_cl_compiler() -> Option<PathBuf> {
    let tools = windows_msvc_environment().ok()?;
    let root = tools.iter().find_map(|(key, value)| {
        key.to_string_lossy()
            .eq_ignore_ascii_case("VCToolsInstallDir")
            .then(|| PathBuf::from(value))
    })?;
    [
        root.join("bin/Hostx64/x64/cl.exe"),
        root.join("bin/Hostx86/x86/cl.exe"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn append_paths_to_command<'a>(command: &mut Command, dirs: impl IntoIterator<Item = &'a Path>) {
    let mut paths = command_env_path(command)
        .or_else(|| env::var_os("PATH"))
        .map(|base_path| env::split_paths(&base_path).collect::<Vec<_>>())
        .unwrap_or_default();
    paths.extend(
        dirs.into_iter()
            .filter(|path| path.exists())
            .map(Path::to_path_buf),
    );
    if !paths.is_empty() {
        command.env(
            "PATH",
            env::join_paths(paths).expect("PATH entries are valid"),
        );
    }
}

fn command_env_path(command: &Command) -> Option<OsString> {
    command.get_envs().find_map(|(key, value)| {
        if key.to_string_lossy().eq_ignore_ascii_case("PATH") {
            value.map(OsString::from)
        } else {
            None
        }
    })
}

fn venv_bin_dir(venv: &Path) -> PathBuf {
    if cfg!(windows) {
        venv.join("Scripts")
    } else {
        venv.join("bin")
    }
}

fn executable_in_dir(dir: &Path, name: &str) -> PathBuf {
    if cfg!(windows) {
        for extension in ["exe", "cmd", "bat"] {
            let candidate = dir.join(format!("{name}.{extension}"));
            if candidate.exists() {
                return candidate;
            }
        }
    }
    dir.join(name)
}

fn existing_path(path: &str) -> Option<PathBuf> {
    let path = PathBuf::from(path);
    path.exists().then_some(path)
}

fn copy_dir_all(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination).with_context(|| format!("create {}", destination.display()))?;
    for entry in fs::read_dir(source).with_context(|| format!("read {}", source.display()))? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if source_path.is_dir() {
            copy_dir_all(&source_path, &destination_path)?;
        } else {
            fs::copy(&source_path, &destination_path).with_context(|| {
                format!(
                    "copy {} to {}",
                    source_path.display(),
                    destination_path.display()
                )
            })?;
        }
    }
    Ok(())
}

fn which(tool: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    for dir in env::split_paths(&path) {
        let candidate = dir.join(tool);
        if candidate.is_file() {
            return Some(candidate);
        }
        if cfg!(windows) && Path::new(tool).extension().is_none() {
            for extension in ["exe", "cmd", "bat"] {
                let candidate = dir.join(format!("{tool}.{extension}"));
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

fn run(command: &mut Command) -> Result<()> {
    let display = command_display(command);
    let status = command
        .stdin(Stdio::null())
        .status()
        .with_context(|| format!("spawn {display}"))?;
    if !status.success() {
        bail!("command failed ({status}): {display}");
    }
    Ok(())
}

fn xcrun(sdk: &str, args: &[&str]) -> Result<String> {
    let output = Command::new("xcrun")
        .arg("--sdk")
        .arg(sdk)
        .args(args)
        .output()
        .with_context(|| format!("spawn xcrun --sdk {sdk} {}", args.join(" ")))?;
    if !output.status.success() {
        bail!(
            "command failed ({}): xcrun --sdk {sdk} {}",
            output.status,
            args.join(" ")
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn command_display(command: &Command) -> String {
    let mut parts = Vec::new();
    parts.push(command.get_program().to_string_lossy().into_owned());
    parts.extend(
        command
            .get_args()
            .map(OsStr::to_string_lossy)
            .map(String::from),
    );
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn android_targets_map_to_rust_abi_and_clang() {
        let cases = [
            (
                "aarch64-linux-android",
                "arm64-v8a",
                "aarch64-linux-android",
            ),
            (
                "armv7-linux-androideabi",
                "armeabi-v7a",
                "armv7a-linux-androideabi",
            ),
            ("x86_64-linux-android", "x86_64", "x86_64-linux-android"),
            ("i686-linux-android", "x86", "i686-linux-android"),
        ];
        for (triple, abi, clang) in cases {
            let target = NativeTarget::parse(triple).unwrap();
            assert!(target.is_android());
            assert_eq!(target.triple(), Some(triple));
            assert_eq!(target.android_abi(), Some(abi));
            assert_eq!(target.android_clang_triple(), Some(clang));
        }
    }

    #[test]
    fn android_ffmpeg_plan_enables_mediacodec_without_videotoolbox() {
        for profile in [
            NativeDependencyProfile::Lgpl,
            NativeDependencyProfile::GplFull,
        ] {
            let flags = profile.ffmpeg_configure_flags_for_target(NativeTarget::X86_64Android);
            assert!(flags.contains(&"--enable-jni"));
            assert!(flags.contains(&"--enable-mediacodec"));
            assert!(flags.contains(&"--enable-libdav1d"));
            assert!(flags.iter().any(|flag| {
                flag.contains("h264_mediacodec")
                    && flag.contains("vp8_mediacodec")
                    && flag.contains("av1_mediacodec")
                    && flag.contains("libdav1d")
            }));
            assert!(flags.iter().any(|flag| {
                flag.strip_prefix("--enable-decoder=")
                    .is_some_and(|decoders| decoders.split(',').any(|decoder| decoder == "vp8"))
            }));
            assert!(!flags.contains(&"--enable-videotoolbox"));
        }
    }

    #[test]
    fn apple_ffmpeg_plan_enables_videotoolbox_with_dav1d_fallback() {
        for target in [NativeTarget::Aarch64Macos, NativeTarget::Aarch64Ios] {
            let flags = NativeDependencyProfile::Lgpl.ffmpeg_configure_flags_for_target(target);
            assert!(flags.contains(&"--enable-videotoolbox"));
            assert!(flags.contains(&"--enable-libdav1d"));
            assert!(flags.contains(&"--enable-decoder=libdav1d"));
        }
    }

    #[test]
    fn android_i686_ffmpeg_disables_non_pic_x86_assembly() {
        for profile in [
            NativeDependencyProfile::Lgpl,
            NativeDependencyProfile::GplFull,
        ] {
            let i686 = profile.ffmpeg_configure_flags_for_target(NativeTarget::I686Android);
            assert!(i686.contains(&"--disable-asm"));
            assert!(!dav1d_asm_enabled(NativeTarget::I686Android));

            let x86_64 = profile.ffmpeg_configure_flags_for_target(NativeTarget::X86_64Android);
            assert!(!x86_64.contains(&"--disable-asm"));
            assert!(dav1d_asm_enabled(NativeTarget::X86_64Android));
        }
    }

    #[test]
    fn ffmpeg_marker_invalidates_when_android_decoder_flags_change() {
        let profile = NativeDependencyProfile::Lgpl;
        let target = NativeTarget::X86_64Android;
        let current = format!(
            "flags={}\n",
            profile.ffmpeg_configure_flags_for_target(target).join(" ")
        );
        assert!(ffmpeg_build_marker_has_current_flags(
            &current, profile, target
        ));

        let stale = current.replace(",vp8,vp9", ",vp9");
        assert!(!ffmpeg_build_marker_has_current_flags(
            &stale, profile, target
        ));
    }

    #[test]
    fn android_ffmpeg_marker_requires_current_patchset_revision() {
        let target = NativeTarget::X86_64Android;
        let patchset = "erika-android-mediacodec-v1-deadbeef";
        assert!(!ffmpeg_build_marker_has_current_patchset(
            &format!("ffmpeg={FFMPEG_VERSION}\n"),
            target,
            patchset
        ));
        assert!(ffmpeg_build_marker_has_current_patchset(
            &format!("ffmpeg={FFMPEG_VERSION}\npatchset={patchset}\n"),
            target,
            patchset
        ));
        assert!(ffmpeg_build_marker_has_current_patchset(
            &format!("ffmpeg={FFMPEG_VERSION}\n"),
            NativeTarget::Host,
            patchset
        ));
    }

    #[test]
    fn unified_patch_application_is_idempotent() {
        let patch = concat!(
            "--- a/example.c\n",
            "+++ b/example.c\n",
            "@@ -2,2 +2,3 @@\n",
            " beta\n",
            "+bounded\n",
            " gamma\n",
        );
        let parsed = parse_unified_patch(patch).unwrap();
        assert_eq!(parsed.len(), 1);

        let original = "alpha\nbeta\ngamma\n";
        let (patched, changed) = apply_patch_hunks(original, &parsed[0].hunks).unwrap();
        assert!(changed);
        assert_eq!(patched, "alpha\nbeta\nbounded\ngamma\n");

        let (reapplied, changed) = apply_patch_hunks(&patched, &parsed[0].hunks).unwrap();
        assert!(!changed);
        assert_eq!(reapplied, patched);
    }

    #[test]
    fn ffmpeg_patch_is_opt_in_and_uses_zero_timeout_dequeues() {
        let root = workspace_root().unwrap();
        let patch = fs::read_to_string(root.join(FFMPEG_PATCHES[0])).unwrap();
        assert_eq!(parse_unified_patch(&patch).unwrap().len(), 3);
        assert!(patch.contains("\"erika_nonblocking\""));
        assert!(patch.contains("OFFSET(erika_nonblocking), AV_OPT_TYPE_BOOL, {.i64 = 0}"));
        assert!(patch.contains("s->ctx->erika_nonblocking = s->erika_nonblocking"));
        assert!(patch.contains("wait && !s->erika_nonblocking"));
        assert!(patch.contains("&null_pkt, !s->erika_nonblocking"));
        assert_eq!(patch.matches("frame, !s->erika_nonblocking").count(), 3);
        assert!(patch.contains("if (s->erika_nonblocking)"));
        assert!(patch.contains("output_dequeue_timeout_us = 0"));
        assert!(patch.contains("s->draining && !s->erika_nonblocking"));
        assert!(patch.contains("return AVERROR(EAGAIN);"));
        assert!(patch.contains("Erika nonblocking MediaCodec receive: yielding"));
        assert!(patch.contains("EAGAIN after zero-timeout input/output starvation"));
        assert!(patch.contains("Erika nonblocking MediaCodec drain: yielding EAGAIN"));
    }

    #[test]
    fn android_ffmpeg_flags_require_dav1d_for_av1_fallback() {
        for target in [
            NativeTarget::Aarch64Android,
            NativeTarget::Armv7Android,
            NativeTarget::X86_64Android,
            NativeTarget::I686Android,
        ] {
            let flags = NativeDependencyProfile::Lgpl.ffmpeg_configure_flags_for_target(target);
            let decoders = flags
                .iter()
                .filter_map(|flag| flag.strip_prefix("--enable-decoder="))
                .flat_map(|value| value.split(','))
                .collect::<HashSet<_>>();
            assert!(decoders.contains("av1_mediacodec"));
            assert!(decoders.contains("libdav1d"));
            assert!(flags.contains(&"--enable-libdav1d"));
        }
    }

    #[test]
    fn android_ndk_versions_sort_numerically() {
        let mut versions = ["9.0.0", "29.0.14206865", "27.2.12479018"];
        versions.sort_by_key(|value| android_ndk_version_key(value));
        assert_eq!(versions, ["9.0.0", "27.2.12479018", "29.0.14206865"]);
    }

    #[cfg(windows)]
    #[test]
    fn appending_paths_keeps_existing_command_path_first() {
        let temp = env::temp_dir().join("erika-xtask-path-order-test");
        let vs_bin = temp.join("VS/VC/bin");
        let system_bin = temp.join("Windows/System32");
        let msys_bin = temp.join("msys64/usr/bin");
        fs::create_dir_all(&vs_bin).unwrap();
        fs::create_dir_all(&system_bin).unwrap();
        fs::create_dir_all(&msys_bin).unwrap();

        let mut command = Command::new("tool");
        let vs_path = env::join_paths([&vs_bin, &system_bin]).unwrap();
        command.env("PATH", vs_path);

        append_paths_to_command(&mut command, [msys_bin.as_path()]);

        let merged = command_env_path(&command).unwrap();
        let paths = env::split_paths(&merged).collect::<Vec<_>>();
        assert_eq!(paths[0], vs_bin);
        assert_eq!(paths[1], system_bin);
        assert!(paths.iter().any(|path| path == &msys_bin));
    }
}

fn print_help() {
    println!("Erika xtask");
    println!("  cargo run -p xtask -- deps plan --profile lgpl");
    println!("  cargo run -p xtask -- deps fetch --profile lgpl [--all]");
    println!("  cargo run -p xtask -- deps status --profile lgpl");
    println!(
        "  cargo run -p xtask -- deps build --profile lgpl [--target host|aarch64-apple-darwin|x86_64-apple-darwin|aarch64-apple-ios|aarch64-apple-ios-sim|x86_64-apple-ios|x86_64-pc-windows-msvc|aarch64-linux-android|armv7-linux-androideabi|x86_64-linux-android|i686-linux-android] [--force] [--jobs N]"
    );
    println!("  cargo run -p xtask -- check license");
}
