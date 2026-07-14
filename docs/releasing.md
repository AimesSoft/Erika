# Releasing Erika

This describes how prebuilt `erika_capi` binaries are published so that other
projects can link Erika without building it from source.

> Translations: pending. Base language: English.

## What ships

Per-platform archives, each containing the C ABI library, the header, and
license files:

| Platform | Archive | Library artifacts |
|----------|---------|-------------------|
| macOS (universal) | `erika-capi-macos-universal.zip` | `liberika_capi.dylib`, `liberika_capi.a` (arm64 + x86_64) |
| Windows x64 | `erika-capi-windows-x64.zip` | `erika_capi.dll`, `erika_capi.dll.lib` (import), `erika_capi.lib` (static) |
| iOS | `erika-capi-ios.zip` | `erika_capi.xcframework` (device + simulator) |
| Android | `erika-capi-android.zip` | `liberika_capi.so`, `liberika_capi.a`, and `libc++_shared.so` for `arm64-v8a`, `armeabi-v7a`, `x86_64`, and `x86` |

The Android archive stores each ABI at `lib/android/<abi>/`. Flutter/Gradle
consumers package `liberika_capi.so` together with the matching NDK
`libc++_shared.so`; `liberika_capi.a` is included for native embedders that
prefer static linkage.

Every archive also includes `include/erika.h`, `LICENSE` (Erika, MPL-2.0),
`THIRD_PARTY_NOTICES.md`, applicable dependency and embedded asset license texts
under `licenses/`, and a `MANIFEST.txt` recording the tag/commit.

The native dependencies (FFmpeg, libass, FreeType, HarfBuzz, FriBidi, zlib, and
Android's dav1d AV1 software decoder) are **statically linked** via the `lgpl`
profile, so each library is self-contained except for OS frameworks
(VideoToolbox/Metal/CoreAudio on Apple; Direct3D 11 / WASAPI on Windows;
MediaCodec/Camera2/AAudio/ANativeWindow on Android), which are always present on
the target OS. The Android shared library additionally depends on the bundled
NDK `libc++_shared.so` for the same ABI.

Linux is **not yet published**. Android is cross-built with NDK r29 at API 26;
the four ABI archives are reproducible through the same `xtask` dependency
pipeline as Apple and Windows.

## How to cut a release

The release is fully automated by
[`.github/workflows/release.yml`](../.github/workflows/release.yml).

1. Make sure `main` is green and the docs/version are up to date. Bump
   `version` in the root `Cargo.toml` if appropriate.
2. Tag and push:
   ```sh
   git tag v0.1.2
   git push origin v0.1.2
   ```
3. The workflow builds the macOS / iOS / Windows / Android bundles (each builds the native
   deps from source, so expect a long run on a cold cache) and attaches the
   archives to a new GitHub Release for that tag.

To dry-run the builds without publishing, trigger the workflow manually
("Run workflow" / `workflow_dispatch`) — the build jobs run, but the publish job
is skipped because it is gated on a tag ref.

## Packaging

[`packaging/bundle.sh`](../packaging/bundle.sh) stages and zips a bundle from a
set of built artifacts (lib files or an `.xcframework`), adding the header,
`LICENSE`, `THIRD_PARTY_NOTICES.md`, and `MANIFEST.txt`. It runs the same locally
and in CI:

```sh
bash packaging/bundle.sh erika-capi-macos-universal \
  dist/erika-capi-macos-universal.zip out/liberika_capi.dylib out/liberika_capi.a
```

## Consuming prebuilt bundles from the Flutter plugin (opt-in)

The `erika_flutter` plugin can download a prebuilt bundle from a release instead
of building Erika from source, which avoids compiling FFmpeg in the host app's
build. It is **opt-in** and always falls back to the source build on any
failure, so enabling it cannot break a build.

Enable it by setting environment variables in the host app's build:

| Variable | Effect |
|----------|--------|
| `ERIKA_PREBUILT=1` | Download the prebuilt `erika_capi` instead of building from source. |
| `ERIKA_PREBUILT_TAG=v0.1.2` | Release tag to download (default `v0.1.2`). |
| `ERIKA_FORCE_SOURCE_BUILD=1` | macOS/iOS only: bypass the prebuilt path and build the local source, useful when debugging Erika changes through the Flutter plugin. |

- **Windows** (`build_erika_runtime.cmake`): downloads `erika-capi-windows-x64.zip`
  and drops `erika_capi.dll` where the plugin bundles it. The plugin loads the
  DLL dynamically, so this is feature-agnostic and works against `v0.1.2`.
- **iOS** (podspec): downloads `erika-capi-ios.zip`, picks the device or
  simulator slice from the XCFramework, and links it. The prebuilt static lib
  must be built `--no-default-features --features libass` to match the plugin's
  link flags (the release workflow does this); verify against a release built
  that way before relying on it.
- **macOS** (podspec `script_phase`): downloads `erika-capi-macos-universal.zip`,
  extracts the universal `liberika_capi.dylib`, and bundles it into the app's
  `Contents/Frameworks` (`install_name @rpath`, codesigned) — where the plugin's
  `dlopen` search finds it. Without `ERIKA_PREBUILT`, the same phase builds the
  universal dylib from source. `ERIKA_MACOS_CAPI_DYLIB` can point at an explicit
  dylib instead. The macOS plugin is self-contained, so a host app no longer
  needs its own dylib-provisioning step.
- **Android**: `erika-capi-android.zip` provides `liberika_capi.so`,
  `liberika_capi.a`, and `libc++_shared.so` per Android ABI. Flutter integration
  packages the shared pair directly; native C/C++ embedders may instead link
  the static archive and provide the matching C++ runtime themselves.

Pin `ERIKA_PREBUILT_TAG` to a release whose Erika source matches the plugin
revision you build against, so the C ABI in the header and the prebuilt library
agree.

## Consuming a bundle

Unzip, then point your build at `include/` for the header and `lib/` for the
library. See [integration.md](integration.md) for the embedding model and
[capi_reference.md](capi_reference.md) for the API. On macOS the dylib's install
name is `@rpath/liberika_capi.dylib`, so add an `@rpath` entry (or copy it
beside your binary).

## Licensing

Erika is MPL-2.0. The bundled native libraries keep their own licenses
(`THIRD_PARTY_NOTICES.md`). Because Erika is open source with a reproducible
build, the LGPL components (FFmpeg, FriBidi) satisfy the LGPL relinking
requirement: the `MANIFEST.txt` records the exact source commit, and anyone can
rebuild against a modified FFmpeg via `xtask deps build --all` + `cargo build`
(see [building.md](building.md)). Keep `LICENSE` and `THIRD_PARTY_NOTICES.md` in
every published archive.

## First-run notes

The native-dependency builds (especially Windows MSVC + MSYS2 + nasm, and the
multi-arch Apple builds) are the parts most likely to need a tweak on the first
real CI run. If a job fails, the failure is almost always in the
`xtask deps build` step; consult [building.md](building.md) for the per-platform
tool requirements and adjust the "Install native build tools" step accordingly.
