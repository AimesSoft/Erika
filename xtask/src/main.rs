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

const FFMPEG_VERSION: &str = "7.1.1";
const LIBASS_VERSION: &str = "0.17.3";
const HARFBUZZ_VERSION: &str = "10.4.0";
const FREETYPE_VERSION: &str = "2.13.3";
const FRIBIDI_VERSION: &str = "1.0.16";
const ZLIB_VERSION: &str = "1.3.1";

const FFMPEG_ARCHIVE: &str = "ffmpeg-7.1.1.tar.xz";
const FFMPEG_DIR: &str = "ffmpeg-7.1.1";
const FFMPEG_URLS: &[&str] = &["https://ffmpeg.org/releases/ffmpeg-7.1.1.tar.xz"];

const LIBASS_ARCHIVE: &str = "libass-0.17.3.tar.xz";
const LIBASS_DIR: &str = "libass-0.17.3";
const LIBASS_URLS: &[&str] = &[
    "https://github.com/libass/libass/releases/download/0.17.3/libass-0.17.3.tar.xz",
    "https://codeload.github.com/libass/libass/tar.gz/refs/tags/0.17.3",
];

const HARFBUZZ_ARCHIVE: &str = "harfbuzz-10.4.0.tar.xz";
const HARFBUZZ_DIR: &str = "harfbuzz-10.4.0";
const HARFBUZZ_URLS: &[&str] = &[
    "https://github.com/harfbuzz/harfbuzz/releases/download/10.4.0/harfbuzz-10.4.0.tar.xz",
    "https://codeload.github.com/harfbuzz/harfbuzz/tar.gz/refs/tags/10.4.0",
];

const FREETYPE_ARCHIVE: &str = "freetype-2.13.3.tar.xz";
const FREETYPE_DIR: &str = "freetype-2.13.3";
const FREETYPE_URLS: &[&str] = &[
    "https://download.savannah.gnu.org/releases/freetype/freetype-2.13.3.tar.xz",
    "https://sourceforge.net/projects/freetype/files/freetype2/2.13.3/freetype-2.13.3.tar.xz/download",
];

const FRIBIDI_ARCHIVE: &str = "fribidi-1.0.16.tar.xz";
const FRIBIDI_DIR: &str = "fribidi-1.0.16";
const FRIBIDI_URLS: &[&str] = &[
    "https://github.com/fribidi/fribidi/releases/download/v1.0.16/fribidi-1.0.16.tar.xz",
    "https://codeload.github.com/fribidi/fribidi/tar.gz/refs/tags/v1.0.16",
];

const ZLIB_ARCHIVE: &str = "zlib-1.3.1.tar.gz";
const ZLIB_DIR: &str = "zlib-1.3.1";
const ZLIB_URLS: &[&str] = &[
    "https://zlib.net/fossils/zlib-1.3.1.tar.gz",
    "https://github.com/madler/zlib/archive/refs/tags/v1.3.1.tar.gz",
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
        bail!("missing deps subcommand: plan, fetch, status, or build");
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
                "--enable-decoder=hevc,h264,av1,vp9,mpeg1video,mpeg2video,mpeg4,vc1,mjpeg,flv,theora,aac,ac3,eac3,dca,truehd,mlp,opus,vorbis,flac,mp3,pcm_s16le,pcm_s24le,pcm_s32le,ass,srt,webvtt,pgssub,dvdsub,dvbsub",
                "--enable-videotoolbox",
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
                "--enable-decoder=hevc,h264,av1,vp9,mpeg1video,mpeg2video,mpeg4,vc1,mjpeg,flv,theora,aac,ac3,eac3,dca,truehd,mlp,opus,vorbis,flac,mp3,pcm_s16le,pcm_s24le,pcm_s32le,ass,srt,webvtt,pgssub,dvdsub,dvbsub",
                "--enable-videotoolbox",
            ],
        }
    }

    fn ffmpeg_configure_flags_for_target(self, target: AppleTarget) -> Vec<&'static str> {
        let mut flags = self.ffmpeg_configure_flags().to_vec();
        if target.is_windows() {
            flags.retain(|flag| *flag != "--enable-videotoolbox");
            flags.extend(["--enable-d3d11va", "--enable-dxva2"]);
        }
        flags
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppleTarget {
    Host,
    Aarch64Macos,
    X86_64Macos,
    Aarch64Ios,
    Aarch64IosSimulator,
    X86_64IosSimulator,
    X86_64WindowsMsvc,
}

impl AppleTarget {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "host" => Ok(Self::Host),
            "aarch64-apple-darwin" => Ok(Self::Aarch64Macos),
            "x86_64-apple-darwin" => Ok(Self::X86_64Macos),
            "aarch64-apple-ios" => Ok(Self::Aarch64Ios),
            "aarch64-apple-ios-sim" => Ok(Self::Aarch64IosSimulator),
            "x86_64-apple-ios" => Ok(Self::X86_64IosSimulator),
            "x86_64-pc-windows-msvc" | "windows-x64" => Ok(Self::X86_64WindowsMsvc),
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
        }
    }

    fn sdk(self) -> Option<&'static str> {
        match self {
            Self::Host => None,
            Self::Aarch64Macos | Self::X86_64Macos => Some("macosx"),
            Self::Aarch64Ios => Some("iphoneos"),
            Self::Aarch64IosSimulator | Self::X86_64IosSimulator => Some("iphonesimulator"),
            Self::X86_64WindowsMsvc => None,
        }
    }

    fn ffmpeg_arch(self) -> Option<&'static str> {
        match self {
            Self::Host => None,
            Self::Aarch64Macos | Self::Aarch64Ios | Self::Aarch64IosSimulator => Some("arm64"),
            Self::X86_64Macos | Self::X86_64IosSimulator => Some("x86_64"),
            Self::X86_64WindowsMsvc => Some("x86_64"),
        }
    }

    fn meson_cpu_family(self) -> Option<&'static str> {
        match self {
            Self::Host => None,
            Self::Aarch64Macos | Self::Aarch64Ios | Self::Aarch64IosSimulator => Some("aarch64"),
            Self::X86_64Macos | Self::X86_64IosSimulator => Some("x86_64"),
            Self::X86_64WindowsMsvc => Some("x86_64"),
        }
    }

    fn meson_cpu(self) -> Option<&'static str> {
        match self {
            Self::Host => None,
            Self::Aarch64Macos | Self::Aarch64Ios | Self::Aarch64IosSimulator => Some("arm64"),
            Self::X86_64Macos | Self::X86_64IosSimulator => Some("x86_64"),
            Self::X86_64WindowsMsvc => Some("x86_64"),
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
            Self::X86_64WindowsMsvc => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct DepsOptions {
    profile: NativeDependencyProfile,
    target: AppleTarget,
    force: bool,
    all: bool,
    jobs: Option<usize>,
}

impl DepsOptions {
    fn parse(args: &[String]) -> Result<Self> {
        let mut options = Self {
            profile: NativeDependencyProfile::Lgpl,
            target: AppleTarget::Host,
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
                    options.target = AppleTarget::parse(value)?;
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
    cache_dir: PathBuf,
    source_dir: PathBuf,
    build_dir: PathBuf,
    dist_dir: PathBuf,
    ffmpeg_source_dir: PathBuf,
    ffmpeg_build_dir: PathBuf,
    ffmpeg_build_marker: PathBuf,
    ffmpeg_prefix: PathBuf,
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
    target: AppleTarget,
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
        cache_dir,
        source_dir,
        build_dir,
        dist_dir,
        ffmpeg_source_dir,
        ffmpeg_build_dir,
        ffmpeg_build_marker,
        ffmpeg_prefix,
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

fn print_dependency_plan(profile: NativeDependencyProfile, target: AppleTarget) {
    println!("Erika native dependency plan");
    println!("profile: {}", profile_name(profile));
    println!("target: {}", target.triple().unwrap_or("host"));
    println!("ffmpeg: {FFMPEG_VERSION} ({})", FFMPEG_URLS[0]);
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
    fetch_and_extract(layout, ZLIB_URLS, ZLIB_ARCHIVE, ZLIB_DIR)?;
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
            &[("zlibstatic.lib", "z.lib")],
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
        &[("zlibstatic.lib", "z.lib")],
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
    } else {
        configure
            .arg("-DHB_HAVE_CORETEXT=ON")
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
    apply_meson_apple_target(&mut setup, layout, options.target, "fribidi")?;
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
        .arg("-Dtest=false")
        .arg("-Dprofile=false")
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
    } else {
        setup
            .arg("-Dcoretext=enabled")
            .arg("-Ddirectwrite=disabled");
    }
    apply_meson_apple_target(&mut setup, layout, options.target, "libass")?;
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
    target: AppleTarget,
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

fn cmake_command(target: AppleTarget) -> Result<Command> {
    if target.is_windows() {
        let cmake = cmake_tool().context("required CMake was not found")?;
        Ok(Command::new(cmake))
    } else {
        Ok(Command::new("cmake"))
    }
}

fn apply_cmake_target(command: &mut Command, target: AppleTarget) -> Result<()> {
    apply_cmake_apple_target(command, target)?;
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

fn apply_cmake_apple_target(command: &mut Command, target: AppleTarget) -> Result<()> {
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

fn apply_meson_apple_target(
    command: &mut Command,
    layout: &WorkspaceLayout,
    target: AppleTarget,
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
    let native_file = meson_native_file(layout, name)?;
    command.arg("--native-file").arg(native_file);
    apply_apple_target_env(command, target)
}

fn meson_native_file(layout: &WorkspaceLayout, name: &str) -> Result<PathBuf> {
    let sdk_root = xcrun("macosx", &["--show-sdk-path"])?;
    let clang = xcrun("macosx", &["-f", "clang"])?;
    let clangxx = xcrun("macosx", &["-f", "clang++"])?;
    // The iOS SDKROOT we export for the cross build otherwise makes clang target
    // iOS even with a macOS -isysroot, producing native tools that cannot run on
    // the build machine. Pin the target triple to macOS to override it.
    let arch = match env::consts::ARCH {
        "aarch64" => "arm64",
        other => other,
    };
    let target = format!("{arch}-apple-macos");
    let path = layout.build_dir.join(format!("{name}-meson-native.ini"));
    let content = format!(
        "[binaries]\nc = [{}, '-target', {}, '-isysroot', {}]\ncpp = [{}, '-target', {}, '-isysroot', {}]\n",
        meson_string(&clang),
        meson_string(&target),
        meson_string(&sdk_root),
        meson_string(&clangxx),
        meson_string(&target),
        meson_string(&sdk_root),
    );
    fs::write(&path, content).with_context(|| format!("write {}", path.display()))?;
    Ok(path)
}

fn meson_cross_file(
    layout: &WorkspaceLayout,
    target: AppleTarget,
    name: &str,
) -> Result<Option<PathBuf>> {
    let Some(config) = apple_toolchain(target)? else {
        return Ok(None);
    };
    let pkg_config = which("pkg-config").unwrap_or_else(|| PathBuf::from("pkg-config"));
    let arch_flags = apple_arch_flags(&config);
    let path = layout.build_dir.join(format!("{name}-meson-cross.ini"));
    let content = format!(
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
    );
    fs::write(&path, content).with_context(|| format!("write {}", path.display()))?;
    Ok(Some(path))
}

fn apply_apple_target_env(command: &mut Command, target: AppleTarget) -> Result<()> {
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
    target: AppleTarget,
) -> Result<()> {
    let mut compile = meson_command(meson);
    compile.arg("compile").arg("-C").arg(build_dir);
    if let Some(jobs) = jobs {
        compile.arg(format!("-j{jobs}"));
    }
    apply_windows_target_env(&mut compile, target)?;
    run(&mut compile)?;
    let mut install = meson_command(meson);
    install.arg("install").arg("-C").arg(build_dir);
    apply_windows_target_env(&mut install, target)?;
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
        run(Command::new("tar")
            .arg("-xf")
            .arg(&archive_path)
            .arg("-C")
            .arg(&layout.source_dir))?;
    } else {
        println!("reuse {}", source_path.display());
    }
    Ok(())
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
    if ffmpeg_build_marker_is_current(layout, options) && !options.force {
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

    if options.force && layout.ffmpeg_prefix.exists() {
        fs::remove_dir_all(&layout.ffmpeg_prefix)
            .with_context(|| format!("remove {}", layout.ffmpeg_prefix.display()))?;
    }
    if options.force && layout.ffmpeg_build_dir.exists() {
        fs::remove_dir_all(&layout.ffmpeg_build_dir)
            .with_context(|| format!("remove {}", layout.ffmpeg_build_dir.display()))?;
    }
    fs::create_dir_all(&layout.ffmpeg_build_dir)
        .with_context(|| format!("create {}", layout.ffmpeg_build_dir.display()))?;
    fs::create_dir_all(&layout.ffmpeg_prefix)
        .with_context(|| format!("create {}", layout.ffmpeg_prefix.display()))?;
    if options.target.is_windows() && !layout.ffmpeg_build_dir.join("configure").exists() {
        println!("copy FFmpeg source for Windows in-tree build");
        copy_dir_all(&layout.ffmpeg_source_dir, &layout.ffmpeg_build_dir)?;
    }

    let mut configure = if options.target.is_windows() {
        let mut command = Command::new(
            posix_shell().context("required POSIX shell was not found for FFmpeg configure")?,
        );
        command.arg("configure");
        command
    } else {
        Command::new(layout.ffmpeg_source_dir.join("configure"))
    };
    configure.current_dir(&layout.ffmpeg_build_dir);
    configure.arg(format!("--prefix={}", layout.ffmpeg_prefix.display()));
    configure.arg("--pkg-config=false");
    configure.arg("--disable-x86asm");
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
        configure.env("SDKROOT", &config.sdk_root);
        match options.target {
            AppleTarget::Aarch64Macos | AppleTarget::X86_64Macos => {
                configure.env("MACOSX_DEPLOYMENT_TARGET", &config.deployment_target);
            }
            AppleTarget::Aarch64Ios
            | AppleTarget::Aarch64IosSimulator
            | AppleTarget::X86_64IosSimulator => {
                configure.env("IPHONEOS_DEPLOYMENT_TARGET", &config.deployment_target);
            }
            AppleTarget::Host | AppleTarget::X86_64WindowsMsvc => {}
        }
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
        append_windows_posix_paths(&mut configure);
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

    let jobs = options.jobs.unwrap_or_else(default_job_count);
    println!("build FFmpeg with {jobs} jobs");
    let make = gnu_make().context("required GNU make was not found")?;
    let mut build = Command::new(&make);
    build
        .current_dir(&layout.ffmpeg_build_dir)
        .arg(format!("-j{jobs}"));
    apply_windows_target_env(&mut build, options.target)?;
    append_windows_posix_paths(&mut build);
    run(&mut build)?;
    let mut install = Command::new(make);
    install.current_dir(&layout.ffmpeg_build_dir).arg("install");
    apply_windows_target_env(&mut install, options.target)?;
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

    fs::write(
        &layout.ffmpeg_build_marker,
        format!(
            "ffmpeg={FFMPEG_VERSION}\nzlib={ZLIB_VERSION}\nprofile={}\ntarget={}\nprefix={}\nflags={}\n",
            profile_name(options.profile),
            options.target.triple().unwrap_or("host"),
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

fn apple_toolchain(target: AppleTarget) -> Result<Option<AppleToolchain>> {
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

fn ffmpeg_flag_path_arg(path: &Path) -> String {
    shell_escape(&path.to_string_lossy().replace('\\', "/"))
}

fn ffmpeg_build_marker_is_current(layout: &WorkspaceLayout, options: DepsOptions) -> bool {
    let Ok(marker) = fs::read_to_string(&layout.ffmpeg_build_marker) else {
        return false;
    };
    marker.contains(&format!("ffmpeg={FFMPEG_VERSION}\n"))
        && marker.contains(&format!("profile={}\n", profile_name(options.profile)))
        && marker.contains(&format!(
            "target={}\n",
            options.target.triple().unwrap_or("host")
        ))
        && marker.contains(&format!("zlib={ZLIB_VERSION}\n"))
        && marker.contains("--enable-zlib")
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
    target: AppleTarget,
) -> Result<()> {
    fs::create_dir_all(&layout.dist_dir)
        .with_context(|| format!("create {}", layout.dist_dir.display()))?;
    fs::write(
        layout.dist_dir.join("erika-native-deps.txt"),
        format!(
            "profile={}\ntarget={}\nffmpeg={}\nffmpeg_dist={}\nzlib={}\nzlib_dist={}\nlibass={}\nlibass_source={}\nharfbuzz={}\nharfbuzz_source={}\nfreetype={}\nfreetype_source={}\nfribidi={}\nfribidi_source={}\n",
            profile_name(profile),
            target.triple().unwrap_or("host"),
            FFMPEG_VERSION,
            layout.ffmpeg_prefix.display(),
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
    println!("license policy ok: default=lgpl, gpl-full is opt-in");
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

fn ensure_windows_link_aliases(
    target: AppleTarget,
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

fn ensure_windows_zlib_static_alias(target: AppleTarget, prefix: &Path) -> Result<()> {
    if !target.is_windows() {
        return Ok(());
    }
    let lib_dir = prefix.join("lib");
    let static_lib = lib_dir.join("zlibstatic.lib");
    let import_lib = lib_dir.join("zlib.lib");
    if static_lib.exists() {
        fs::copy(&static_lib, &import_lib).with_context(|| {
            format!("copy {} to {}", static_lib.display(), import_lib.display())
        })?;
    }
    Ok(())
}

fn ensure_windows_zlib_header_compat(target: AppleTarget, prefix: &Path) -> Result<()> {
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
             set \"ERIKA_PKG_CONFIG_PATH=%ERIKA_DIST_DIR%\\ffmpeg\\lib\\pkgconfig;%ERIKA_DIST_DIR%\\freetype\\lib\\pkgconfig;%ERIKA_DIST_DIR%\\harfbuzz\\lib\\pkgconfig;%ERIKA_DIST_DIR%\\fribidi\\lib\\pkgconfig;%ERIKA_DIST_DIR%\\libass\\lib\\pkgconfig\"\r\n\
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

fn apply_windows_target_env(command: &mut Command, target: AppleTarget) -> Result<()> {
    if !target.is_windows() {
        return Ok(());
    }
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
    which("cmake").or_else(|| {
        existing_path(
            "C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/Common7/IDE/CommonExtensions/Microsoft/CMake/CMake/bin/cmake.exe",
        )
    })
}

fn ninja_tool() -> Option<PathBuf> {
    which("ninja").or_else(|| {
        existing_path(
            "C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/Common7/IDE/CommonExtensions/Microsoft/CMake/Ninja/ninja.exe",
        )
    })
}

fn posix_shell() -> Option<PathBuf> {
    existing_path("C:/msys64/usr/bin/sh.exe")
        .or_else(|| which("sh"))
        .or_else(|| which("bash"))
        .or_else(|| existing_path("C:/Program Files/Git/usr/bin/sh.exe"))
        .or_else(|| existing_path("C:/Program Files/Git/bin/bash.exe"))
}

fn gnu_make() -> Option<PathBuf> {
    existing_path("C:/msys64/usr/bin/make.exe")
        .or_else(|| which("make"))
        .or_else(|| which("gmake"))
        .or_else(|| which("mingw32-make"))
        .or_else(|| existing_path("C:/mingw64/bin/mingw32-make.exe"))
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
        "  cargo run -p xtask -- deps build --profile lgpl [--target host|aarch64-apple-darwin|x86_64-apple-darwin|aarch64-apple-ios|aarch64-apple-ios-sim|x86_64-apple-ios|x86_64-pc-windows-msvc|windows-x64] [--force] [--jobs N]"
    );
    println!("  cargo run -p xtask -- check license");
}
