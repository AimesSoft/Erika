use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

const FFMPEG_VERSION: &str = "7.1.1";
const LIBASS_VERSION: &str = "0.17.3";
const HARFBUZZ_VERSION: &str = "10.4.0";
const FREETYPE_VERSION: &str = "2.13.3";
const FRIBIDI_VERSION: &str = "1.0.16";

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

fn main() -> Result<()> {
    let mut args = env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        print_help();
        return Ok(());
    }

    match args.remove(0).as_str() {
        "deps" => deps(args),
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
                "--enable-protocol=file",
                "--enable-demuxer=mov,matroska,mpegts,mpegps,mpegvideo,avi,flv,h264,hevc,av1,ivf,mp3,aac,flac,wav,ogg,ac3,eac3,dts,truehd,mlp,mjpeg,vc1,ass,srt,webvtt",
                "--enable-parser=hevc,h264,av1,vp9,aac,ac3,dca,mlp,opus,vorbis,flac,mpegaudio,mpegvideo,mpeg4video,mjpeg,vc1,dvdsub,dvbsub",
                "--enable-decoder=hevc,h264,av1,vp9,mpeg1video,mpeg2video,mpeg4,vc1,mjpeg,flv,theora,aac,ac3,eac3,dca,truehd,mlp,opus,vorbis,flac,mp3,pcm_s16le,pcm_s24le,pcm_s32le,ass,srt,webvtt,pgssub,dvdsub,dvbsub",
                "--enable-videotoolbox",
            ],
        }
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
            other => bail!("unknown Apple target: {other}"),
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
        }
    }

    fn sdk(self) -> Option<&'static str> {
        match self {
            Self::Host => None,
            Self::Aarch64Macos | Self::X86_64Macos => Some("macosx"),
            Self::Aarch64Ios => Some("iphoneos"),
            Self::Aarch64IosSimulator | Self::X86_64IosSimulator => Some("iphonesimulator"),
        }
    }

    fn ffmpeg_arch(self) -> Option<&'static str> {
        match self {
            Self::Host => None,
            Self::Aarch64Macos | Self::Aarch64Ios | Self::Aarch64IosSimulator => Some("arm64"),
            Self::X86_64Macos | Self::X86_64IosSimulator => Some("x86_64"),
        }
    }

    fn meson_cpu_family(self) -> Option<&'static str> {
        match self {
            Self::Host => None,
            Self::Aarch64Macos | Self::Aarch64Ios | Self::Aarch64IosSimulator => Some("aarch64"),
            Self::X86_64Macos | Self::X86_64IosSimulator => Some("x86_64"),
        }
    }

    fn meson_cpu(self) -> Option<&'static str> {
        match self {
            Self::Host => None,
            Self::Aarch64Macos | Self::Aarch64Ios | Self::Aarch64IosSimulator => Some("arm64"),
            Self::X86_64Macos | Self::X86_64IosSimulator => Some("x86_64"),
        }
    }

    fn is_ios(self) -> bool {
        matches!(
            self,
            Self::Aarch64Ios | Self::Aarch64IosSimulator | Self::X86_64IosSimulator
        )
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
    println!("ffmpeg configure flags:");
    for flag in profile.ffmpeg_configure_flags() {
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
    ensure_required_tools()?;
    let layout = workspace_layout(options.profile, options.target)?;
    prepare_dependency_dirs(&layout)?;
    fetch_dependency_sources(&layout, options.all)?;
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
        status_word(layout.ffmpeg_prefix.join("lib/libavformat.a").exists())
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
        status_word(layout.freetype_prefix.join("lib/libfreetype.a").exists())
    );
    println!(
        "harfbuzz dist: {}",
        status_word(layout.harfbuzz_prefix.join("lib/libharfbuzz.a").exists())
    );
    println!(
        "fribidi dist: {}",
        status_word(layout.fribidi_prefix.join("lib/libfribidi.a").exists())
    );
    println!(
        "libass dist: {}",
        status_word(layout.libass_prefix.join("lib/libass.a").exists())
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

fn ensure_required_tools() -> Result<()> {
    for tool in [
        "curl",
        "tar",
        "xz",
        "make",
        "clang",
        "cmake",
        "python3",
        "pkg-config",
    ] {
        if which(tool).is_none() {
            bail!("required build tool `{tool}` was not found in PATH");
        }
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
    let mut configure = Command::new("cmake");
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
    apply_cmake_apple_target(&mut configure, options.target)?;
    run(&mut configure)?;
    cmake_build_install(&layout.freetype_build_dir, options.jobs)?;
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
    let mut configure = Command::new("cmake");
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
        .arg("-DHB_HAVE_CORETEXT=ON")
        .arg("-DHB_BUILD_UTILS=OFF")
        .arg("-DHB_BUILD_SUBSET=OFF");
    apply_cmake_apple_target(&mut configure, options.target)?;
    run(&mut configure)?;
    cmake_build_install(&layout.harfbuzz_build_dir, options.jobs)?;
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
    run(&mut setup)?;
    meson_compile_install(&meson, &layout.fribidi_build_dir, options.jobs)?;
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
        return Ok(());
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
        .arg("-Dcoretext=enabled")
        .arg("-Dasm=disabled")
        .arg("-Dlibunibreak=disabled")
        .env("PKG_CONFIG_PATH", &pkg_config_path);
    apply_meson_apple_target(&mut setup, layout, options.target, "libass")?;
    run(&mut setup)?;

    let mut compile = meson_command(&meson);
    compile
        .arg("compile")
        .arg("-C")
        .arg(&layout.libass_build_dir)
        .env("PKG_CONFIG_PATH", &pkg_config_path);
    if let Some(jobs) = options.jobs {
        compile.arg(format!("-j{jobs}"));
    }
    run(&mut compile)?;
    let mut install = meson_command(&meson);
    install
        .arg("install")
        .arg("-C")
        .arg(&layout.libass_build_dir)
        .env("PKG_CONFIG_PATH", &pkg_config_path);
    run(&mut install)?;

    write_marker(
        &layout.libass_build_marker,
        "libass",
        LIBASS_VERSION,
        &layout.libass_prefix,
    )
}

fn cmake_build_install(build_dir: &std::path::Path, jobs: Option<usize>) -> Result<()> {
    let mut build = Command::new("cmake");
    build
        .arg("--build")
        .arg(build_dir)
        .arg("--config")
        .arg("Release");
    if let Some(jobs) = jobs {
        build.arg("--parallel").arg(jobs.to_string());
    }
    run(&mut build)?;
    run(Command::new("cmake")
        .arg("--install")
        .arg(build_dir)
        .arg("--config")
        .arg("Release"))
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
    let meson = venv.join("bin/meson");
    let ninja = venv.join("bin/ninja");
    if meson.exists() && ninja.exists() {
        return Ok(MesonTools {
            meson,
            bin_dir: venv.join("bin"),
        });
    }

    fs::create_dir_all(&layout.python_tools_dir)
        .with_context(|| format!("create {}", layout.python_tools_dir.display()))?;
    println!("bootstrap local meson/ninja tools");
    run(Command::new("python3").arg("-m").arg("venv").arg(&venv))?;
    run(Command::new(venv.join("bin/python"))
        .arg("-m")
        .arg("pip")
        .arg("install")
        .arg("--upgrade")
        .arg("pip")
        .arg("meson==1.8.5")
        .arg("ninja==1.13.0"))?;
    Ok(MesonTools {
        meson,
        bin_dir: venv.join("bin"),
    })
}

fn meson_command(meson: &MesonTools) -> Command {
    let mut command = Command::new(&meson.meson);
    prepend_path(&mut command, &meson.bin_dir);
    command
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
) -> Result<()> {
    let mut compile = meson_command(meson);
    compile.arg("compile").arg("-C").arg(build_dir);
    if let Some(jobs) = jobs {
        compile.arg(format!("-j{jobs}"));
    }
    run(&mut compile)?;
    let mut install = meson_command(meson);
    install.arg("install").arg("-C").arg(build_dir);
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
    for url in urls {
        println!("download {url}");
        if partial_path.exists() {
            fs::remove_file(partial_path)
                .with_context(|| format!("remove {}", partial_path.display()))?;
        }
        let mut curl = Command::new("curl");
        curl.arg("-L")
            .arg("--fail")
            .arg("--show-error")
            .arg("--connect-timeout")
            .arg("20")
            .arg("--max-time")
            .arg("300")
            .arg("--speed-limit")
            .arg("1")
            .arg("--speed-time")
            .arg("20")
            .arg("--retry")
            .arg("2")
            .arg("--retry-delay")
            .arg("2")
            .arg("--output")
            .arg(partial_path)
            .arg(url);
        match run(&mut curl) {
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

fn build_ffmpeg(layout: &WorkspaceLayout, options: DepsOptions) -> Result<()> {
    if layout.ffmpeg_build_marker.exists() && !options.force {
        println!(
            "reuse FFmpeg build marker {}",
            layout.ffmpeg_build_marker.display()
        );
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

    let mut configure = Command::new(layout.ffmpeg_source_dir.join("configure"));
    configure.current_dir(&layout.ffmpeg_build_dir);
    configure.arg(format!("--prefix={}", layout.ffmpeg_prefix.display()));
    configure.arg("--pkg-config=false");
    configure.arg("--disable-x86asm");
    let mut extra_cflags = vec!["-fPIC".to_string()];
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
            AppleTarget::Host => {}
        }
    } else {
        configure.arg("--cc=clang");
    }
    configure.arg(format!("--extra-cflags={}", extra_cflags.join(" ")));
    if !extra_ldflags.is_empty() {
        configure.arg(format!("--extra-ldflags={}", extra_ldflags.join(" ")));
    }
    for flag in options.profile.ffmpeg_configure_flags() {
        configure.arg(flag);
    }

    println!("configure FFmpeg");
    run(&mut configure)?;

    let jobs = options.jobs.unwrap_or_else(default_job_count);
    println!("build FFmpeg with {jobs} jobs");
    run(Command::new("make")
        .current_dir(&layout.ffmpeg_build_dir)
        .arg(format!("-j{jobs}")))?;
    run(Command::new("make")
        .current_dir(&layout.ffmpeg_build_dir)
        .arg("install"))?;

    fs::write(
        &layout.ffmpeg_build_marker,
        format!(
            "ffmpeg={FFMPEG_VERSION}\nprofile={}\ntarget={}\nprefix={}\n",
            profile_name(options.profile),
            options.target.triple().unwrap_or("host"),
            layout.ffmpeg_prefix.display()
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

fn write_profile_metadata(
    layout: &WorkspaceLayout,
    profile: NativeDependencyProfile,
    target: AppleTarget,
) -> Result<()> {
    fs::write(
        layout.dist_dir.join("erika-native-deps.txt"),
        format!(
            "profile={}\ntarget={}\nffmpeg={}\nffmpeg_dist={}\nlibass={}\nlibass_source={}\nharfbuzz={}\nharfbuzz_source={}\nfreetype={}\nfreetype_source={}\n",
            profile_name(profile),
            target.triple().unwrap_or("host"),
            FFMPEG_VERSION,
            layout.ffmpeg_prefix.display(),
            LIBASS_VERSION,
            source_state(&layout.libass_source_dir),
            HARFBUZZ_VERSION,
            source_state(&layout.harfbuzz_source_dir),
            FREETYPE_VERSION,
            source_state(&layout.freetype_source_dir)
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

fn which(tool: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|dir| dir.join(tool))
        .find(|candidate| candidate.is_file())
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

fn print_help() {
    println!("Erika xtask");
    println!("  cargo run -p xtask -- deps plan --profile lgpl");
    println!("  cargo run -p xtask -- deps fetch --profile lgpl [--all]");
    println!("  cargo run -p xtask -- deps status --profile lgpl");
    println!(
        "  cargo run -p xtask -- deps build --profile lgpl [--target host|aarch64-apple-darwin|x86_64-apple-darwin|aarch64-apple-ios|aarch64-apple-ios-sim|x86_64-apple-ios] [--force] [--jobs N]"
    );
    println!("  cargo run -p xtask -- check license");
}
