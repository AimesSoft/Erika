# Building Erika

> Translations: [中文](building.zh.md) · [日本語](building.ja.md)

Erika is a Rust workspace that links a set of **statically built native
dependencies** (FFmpeg and, optionally, the libass subtitle stack). Those native
libraries are not vendored — you build them once with the `xtask` orchestrator,
which stages them under `third_party/dist/`, and the Rust crates link against
that staging directory.

```
xtask deps build  ──▶  third_party/dist/<target>/<profile>/{ffmpeg,zlib,libass,…}
                                        │
                          erika_ffmpeg_sys/build.rs  (auto-discovers dist, runs bindgen)
                                        │
                                  cargo build -p erika
```

## Prerequisites

### Rust

- Rust **1.92+** (workspace edition 2024).
- For cross-targets, add the Rust std target, e.g.
  `rustup target add aarch64-apple-ios` or
  `rustup target add x86_64-pc-windows-msvc`.

### Build tools — macOS / Unix host

`tar`, `make`, `clang`, `cmake`, `pkg-config`, and `python3` (with `venv`) must
be on `PATH`. Building the full subtitle stack (`--all`) additionally needs
`meson` and `ninja` (and `nasm` for FFmpeg's x86 assembly on Intel hosts). On
macOS, install the Xcode Command Line Tools plus the above via Homebrew.

`erika_ffmpeg_sys` runs **bindgen**, which needs `libclang`. If it is not found
automatically, set `LIBCLANG_PATH`.

### Build tools — Windows (`x86_64-pc-windows-msvc`)

- **Visual Studio Build Tools** (MSVC) + Windows SDK, and the **CMake**
  component.
- A **POSIX shell** (Git for Windows or MSYS2) — FFmpeg's `configure` needs it.
- **GNU make** (MSYS2 `make` or MinGW `mingw32-make`).
- `nasm` for FFmpeg assembly.
- For `--all`: **Python** (with `venv`); `xtask` provisions a `pkg-config` shim
  automatically.

Run the commands from a shell where the MSVC environment is active (e.g. a
*"x64 Native Tools Command Prompt"*), so `xtask` can locate the toolchain.

## Native dependencies via `xtask`

`xtask` is a workspace member; invoke it with `cargo run -p xtask -- …`.

```sh
# Inspect what would be built (no side effects)
cargo run -p xtask -- deps plan
cargo run -p xtask -- deps status

# Build the minimal set (zlib + FFmpeg) — LGPL profile
cargo run -p xtask -- deps build --profile lgpl

# Build everything, including the libass subtitle stack
cargo run -p xtask -- deps build --all --profile lgpl
```

Subcommands: `plan` (print the plan), `fetch` (download sources only),
`status` (what is present/built), `build` (fetch + compile).

### Options

| Flag | Values | Default | Meaning |
|------|--------|---------|---------|
| `--profile` | `lgpl`, `gpl-full` | `lgpl` | FFmpeg license profile (see below). |
| `--target` | see targets table | `host` | Cross-compile target. |
| `--all` | — | off | Also build libass + FreeType + HarfBuzz + FriBidi (subtitle rendering). Without it, only zlib + FFmpeg are built. |
| `--force` | — | off | Rebuild even if up-to-date markers exist. |
| `--jobs N` | integer | auto | Parallelism for the native builds. |

### Targets

| `--target` | Triple | Notes |
|------------|--------|-------|
| `host` | current machine | Default. |
| `aarch64-apple-darwin` | Apple Silicon macOS | |
| `x86_64-apple-darwin` | Intel macOS | |
| `aarch64-apple-ios` | iOS device | |
| `aarch64-apple-ios-sim` | iOS sim (Apple Silicon) | |
| `x86_64-apple-ios` | iOS sim (Intel) | |
| `x86_64-pc-windows-msvc` (or `windows-x64`) | Windows | Swaps VideoToolbox for D3D11VA/DXVA2 in FFmpeg. |

Deployment minimums default to macOS `11.0` / iOS `13.0` and can be overridden
with `MACOSX_DEPLOYMENT_TARGET` / `IPHONEOS_DEPLOYMENT_TARGET`.

## License profiles

The native build is split into profiles so the license boundary is explicit:

- **`lgpl`** (default) — FFmpeg configured `--disable-gpl --enable-version3`,
  static, no network, file protocol only, a curated demuxer/decoder/parser set,
  zlib enabled, plus VideoToolbox (Apple) or D3D11VA/DXVA2 (Windows).
- **`gpl-full`** — the same set with `--enable-gpl`. Use only if you accept GPL
  terms for the resulting binary.

The Rust workspace itself is MPL-2.0 (see [`LICENSE`](../LICENSE)). Keep the
profile consistent across `xtask` and your `cargo build`. `cargo run -p xtask --
check license` validates the policy.

## The `dist` layout

After a build, libraries land under (per target + profile):

```
third_party/
  cache/                       downloaded archives
  src/                         extracted sources
  build/<target>/<profile>/    out-of-tree build trees
  dist/<target>/<profile>/     install prefixes the crates link:
    ffmpeg/{include,lib}
    zlib/    libass/    freetype/    harfbuzz/    fribidi/
```

For the `host` target the `<target>` path segment is omitted
(`third_party/dist/<profile>/…`).

## How the crates find `dist`

`erika_ffmpeg_sys/build.rs` discovers the FFmpeg prefix automatically:

1. `ERIKA_FFMPEG_DIR`, if set (explicit override).
2. else `third_party/dist/$ERIKA_NATIVE_TARGET/$ERIKA_NATIVE_PROFILE/ffmpeg`,
   if `ERIKA_NATIVE_TARGET` is set.
3. else `third_party/dist/<profile>/ffmpeg` under the workspace root (with an
   `ios/` segment when building for iOS).

Relevant environment variables: `ERIKA_NATIVE_PROFILE`, `ERIKA_NATIVE_TARGET`,
`ERIKA_FFMPEG_DIR`, `ERIKA_ZLIB_DIR`, `LIBCLANG_PATH`, and
`ERIKA_ALLOW_LEGACY_FFMPEG` (escape hatch). Erika requires FFmpeg **7.x**
(`libavutil >= 59`); the Windows native core enforces this. Set
`ERIKA_ALLOW_LEGACY_FFMPEG=1` only for local compatibility experiments.

## Compile and test

```sh
cargo build -p erika                 # core library
cargo build -p erika_capi            # C ABI (produces the dylib/staticlib/dll)
cargo test --workspace               # unit + integration tests
```

`erika_capi` produces the artifact native hosts link:

- macOS: `liberika_capi.dylib` (loaded via `dlopen` by the macOS Flutter plugin;
  override with `ERIKA_CAPI_DYLIB`).
- iOS: `liberika_capi.a` (static).
- Windows: `erika_capi.dll` (the Flutter Windows plugin builds it through
  `build_erika_runtime.cmake`).

## Verify the playback path

```sh
# macOS
export SAMPLE="/path/to/video.mp4"
cargo run -p macos_native_demo -- "$SAMPLE"
cargo run -p macos_native_demo -- --smoke-seconds 3 "$SAMPLE"

# Windows
cargo run -p windows_native_demo -- "%SAMPLE%"
cargo run -p windows_native_demo -- --smoke-seconds 3 --metrics-log out.jsonl "%SAMPLE%"
```

The demos print per-frame pipeline stats (decoded/rendered frames, zero-copy vs
CPU fallback, HDR10 activity, audio underflow) — a quick way to confirm hardware
decode and zero-copy interop are engaged.

## Troubleshooting

- **"FFmpeg headers were not found …"** — you haven't run `xtask deps build` for
  this target/profile, or `ERIKA_NATIVE_TARGET`/`ERIKA_NATIVE_PROFILE` don't
  match what you built. Run `deps status` to see what's present.
- **bindgen / libclang errors** — set `LIBCLANG_PATH` to your LLVM `lib`
  directory.
- **Windows: configure fails** — ensure a POSIX shell (Git Bash/MSYS2) and GNU
  make are on `PATH`, and that you launched from an MSVC environment.
- **Legacy FFmpeg rejected** — install/build the 7.x bundle; don't rely on a
  system FFmpeg.
- **License check fails** — your profile mixes GPL and LGPL artifacts; rebuild
  deps with a single `--profile`.

See [CONTRIBUTING.md](../CONTRIBUTING.md) for the development workflow and
[architecture.md](architecture.md) for how the pieces fit together.
