# Releasing Erika

> Translations: [中文](releasing.zh.md) · [日本語](releasing.ja.md)

This describes how prebuilt `erika_capi` binaries are published so that other
projects can link Erika without building it from source.

## What ships

Per-platform archives, each containing the C ABI library, the header, and
license files:

| Platform | Archive | Library artifacts |
|----------|---------|-------------------|
| macOS arm64 | `erika-capi-macos-arm64.zip` | `liberika_capi.dylib`, `liberika_capi.a` |
| macOS x64 | `erika-capi-macos-x64.zip` | `liberika_capi.dylib`, `liberika_capi.a` |
| macOS universal | `erika-capi-macos-universal.zip` | `liberika_capi.dylib`, `liberika_capi.a` (arm64 + x86_64) |
| Windows x64 | `erika-capi-windows-x64.zip` | `erika_capi.dll`, `erika_capi.dll.lib` (import), `erika_capi.lib` (static) |
| Windows ARM64 | `erika-capi-windows-arm64.zip` | `erika_capi.dll`, `erika_capi.dll.lib` (import), `erika_capi.lib` (static) |
| iOS | `erika-capi-ios.zip` | `erika_capi.xcframework` (device + simulator) |
| tvOS | `erika-capi-tvos.zip` | `erika_capi.xcframework` (device + arm64/x86_64 simulator) |
| Android | `erika-capi-android.zip` | `liberika_capi.so`, `liberika_capi.a`, and `libc++_shared.so` for `arm64-v8a`, `armeabi-v7a`, `x86_64`, and `x86` |
| Flutter Android | `erika-flutter-android-<abi>.zip` | `liberika_capi.so` and `libc++_shared.so` for one requested ABI |
| OpenHarmony arm64 | `erika-capi-openharmony-arm64.zip` | `liberika_capi.so`, `liberika_flutter.so` |

The OpenHarmony archive is built against the OpenHarmony 5.1.0 native SDK with
compatible SDK version 18. It contains the C API runtime and Flutter N-API
bridge. The Flutter plugin downloads the package-pinned release by default,
verifies its SHA-256, links the prebuilt runtime, and packages it beside the
locally linked N-API bridge. Download and verification failures are explicit;
source builds are enabled only with `ERIKA_FORCE_SOURCE_BUILD=1`.

The Android archive stores each ABI at `lib/android/<abi>/`. Flutter/Gradle
consumers instead download one `erika-flutter-android-<abi>.zip` per requested
ABI and package `liberika_capi.so` together with the matching NDK
`libc++_shared.so`. They do not download other architectures or the static
library. The combined C API archive retains `liberika_capi.a` for native
embedders that prefer static linkage.

Every archive also includes `include/erika.h`, `LICENSE` (Erika, MPL-2.0),
`THIRD_PARTY_NOTICES.md`, applicable dependency and embedded asset license texts
under `licenses/`, and a `MANIFEST.txt` recording the tag/commit.
Every GitHub Release also includes `SHA256SUMS` with the digest of each archive.

The native dependencies (FFmpeg, libass, FreeType, HarfBuzz, FriBidi, zlib,
dav1d, and SoundTouch) are **statically linked** via the `lgpl`
profile, so each library is self-contained except for OS frameworks
(VideoToolbox/Metal/CoreAudio on Apple; Direct3D 11 / WASAPI on Windows;
MediaCodec/AAudio/ANativeWindow on Android), which are always present on
the target OS. The Android shared library additionally depends on the bundled
NDK `libc++_shared.so` for the same ABI.

Linux is **not yet published**. Android is cross-built with NDK r29 at API 26;
the four ABI archives are reproducible through the same `xtask` dependency
pipeline as Apple and Windows.

## Publishing the Flutter package

`erika_flutter` is published separately on [pub.dev](https://pub.dev/packages/erika_flutter).
Version `0.1.7` is the first standalone package release and supports macOS,
iOS, tvOS, Windows, Android, and HarmonyOS/OpenHarmony. The package archive
contains the plugin sources, package `LICENSE`, README files, examples, and the
version-pinned native artifact manifest; platform builds fetch the matching
GitHub Release archives and verify their SHA-256 values.

From a clean worktree, validate and publish the package from its directory:

```sh
cd packages/erika_flutter
dart pub publish --dry-run
dart pub publish
```

The isolated package and platform consumer checks run from
[`.github/workflows/flutter-package.yml`](../.github/workflows/flutter-package.yml)
before a package release is merged. Linux and Web are not package targets yet.

Publishing to pub.dev is automated by
[`.github/workflows/pub-publish.yml`](../.github/workflows/pub-publish.yml).
Native archives include build metadata, so the package-pinned SHA-256 values can
only be updated after the corresponding GitHub Release exists. Use this order:

1. Push `v0.1.8` as described below and wait for the native GitHub Release and
   its `SHA256SUMS` asset.
2. Update `packages/erika_flutter/pubspec.yaml`, `native_artifacts.properties`,
   and the changelog to the same version. Copy all 12 platform archive digests
   from the Release into the native artifact manifest.
3. Merge after the Flutter package checks pass, then tag that package commit:

   ```sh
   VERSION=0.1.8
   git tag "erika_flutter-v${VERSION}"
   git push origin "erika_flutter-v${VERSION}"
   ```

The action verifies the package version, native version, and every pinned
SHA-256 against the GitHub Release before publishing through pub.dev OIDC. The
one-time pub.dev Admin configuration is repository `AimesSoft/Erika` with tag
pattern `erika_flutter-v{{version}}`; no long-lived upload secret is stored.

### Pub.dev publisher identity

`unverified uploader` means the package was uploaded by a pub.dev account that
is not associated with a verified pub.dev publisher. It does not indicate a
package validation, license, or build failure. To show a verified publisher,
create or join a pub.dev publisher for a domain you control and complete the
domain verification, then transfer ownership of the package to that publisher.

## How to cut a release

The release is fully automated by
[`.github/workflows/release.yml`](../.github/workflows/release.yml).

1. Make sure `main` is green and the docs/version are up to date. Review the
   root [`CHANGELOG.md`](../CHANGELOG.md), especially its breaking-change
   section, and bump `version` in the root `Cargo.toml` if appropriate.
2. Tag and push:
   ```sh
   VERSION=0.1.8
   git tag "v${VERSION}"
   git push origin "v${VERSION}"
   ```
3. The workflow cross-builds macOS arm64 and x64 on `macos-26`, then combines
   those outputs into the universal bundle. iOS and tvOS XCFrameworks also use
   `macos-26`. Windows x64 runs on `windows-latest` and Windows ARM64 runs
   natively on `windows-11-arm`. It also builds the Android and OpenHarmony
   bundles and attaches all archives to a new GitHub Release for that tag.

To dry-run the builds without publishing, trigger the workflow manually
("Run workflow" / `workflow_dispatch`) — the build jobs run, but the publish job
is skipped because it is gated on a tag ref.

## Pre-release validation

Before pushing a `v*` tag, use a clean worktree and record:

```sh
cargo fmt --all -- --check
cargo test -p erika -p erika_capi
cargo test --workspace
cargo clippy -p erika -p erika_capi --all-targets -- -D warnings
```

Also compile affected examples, compare the public `erika.h` header with the C
ABI reference and Flutter FFI glue, inspect every archive manifest/license, and
verify the pinned prebuilt tag in NipaPlay before publishing release notes.

## Packaging

[`packaging/bundle.sh`](../packaging/bundle.sh) stages and zips a bundle from a
set of built artifacts (lib files or an `.xcframework`), adding the header,
`LICENSE`, `THIRD_PARTY_NOTICES.md`, and `MANIFEST.txt`. It runs the same locally
and in CI:

```sh
bash packaging/bundle.sh erika-capi-macos-universal \
  dist/erika-capi-macos-universal.zip out/liberika_capi.dylib out/liberika_capi.a
```

## Consuming prebuilt bundles from the Flutter plugin

The `erika_flutter` plugin can download a prebuilt bundle from a release instead
of building Erika from source, which avoids compiling FFmpeg in the host app's
build. Verified prebuilt bundles are the default. Download, extraction, or
checksum failures stop with an actionable error instead of silently requiring a
Rust and native dependency toolchain.

The following environment variables customize that behavior:

| Variable | Effect |
|----------|--------|
| `ERIKA_PREBUILT_TAG=v0.1.7` | Override the package-pinned release tag. Also requires `ERIKA_PREBUILT_SHA256`. |
| `ERIKA_PREBUILT_SHA256=...` | Expected digest when overriding the release tag. |
| `ERIKA_PREBUILT_SHA256_<ABI>=...` | Per-ABI digest for custom multi-ABI Android builds; suffixes are `ARM64_V8A`, `ARMEABI_V7A`, `X86_64`, and `X86`. |
| `ERIKA_FORCE_SOURCE_BUILD=1` | Bypass the prebuilt path and build the local source, useful when debugging Erika changes through the Flutter plugin. |
| `ERIKA_MACOS_ARCHS=universal|arm64|x86_64|arm64,x86_64` | Select the macOS source and prebuilt artifact architecture. |

- **Windows** (`build_erika_runtime.cmake`): downloads the x64 or ARM64 archive
  selected by the CMake target and drops `erika_capi.dll` where the plugin
  bundles it.
- **iOS** (podspec): downloads `erika-capi-ios.zip`, picks the device or
  simulator slice from the XCFramework, and links it. The prebuilt static lib
  must be built `--no-default-features --features libass` to match the plugin's
  link flags (the release workflow does this); verify against a release built
  that way before relying on it.
- **tvOS** (podspec): downloads `erika-capi-tvos.zip` and selects the device or
  universal simulator slice from its XCFramework.
- **OpenHarmony** (plugin CMake): downloads
  `erika-capi-openharmony-arm64.zip`, links its `liberika_capi.so`, and stages
  that runtime beside `liberika_flutter.so` for HAR/HAP packaging.
- **macOS** (podspec `script_phase`): downloads the arm64, x64, or universal
  archive selected by `ERIKA_MACOS_ARCHS` and bundles its dylib into the app's
  `Contents/Frameworks` (`install_name @rpath`, codesigned). With
  `ERIKA_FORCE_SOURCE_BUILD=1`, the same phase builds the selected architecture from source.
  `ERIKA_MACOS_CAPI_DYLIB` can point at an explicit dylib instead.
- **Android** (`erika-native.gradle`): downloads only the ABI-specific
  `erika-flutter-android-<abi>.zip` assets requested by the Flutter build and
  stages `liberika_capi.so` plus `libc++_shared.so`. Native C/C++ embedders may
  instead use the combined `erika-capi-android.zip`, link its static archive,
  and provide the matching C++ runtime themselves.

The package pins its native tag and SHA-256 values. If you override the tag,
provide the matching digest so the C ABI in the package header and the prebuilt
library cannot drift silently. A single-ABI Android build may use the generic
digest variable; a multi-ABI Android build requires each selected ABI's
specific variable.

## Consuming a bundle

Unzip, then point your build at `include/` for the header and `lib/` for the
library. See [integration.md](integration.md) for the embedding model and
[capi_reference.md](capi_reference.md) for the API. On macOS the dylib's install
name is `@rpath/liberika_capi.dylib`, so add an `@rpath` entry (or copy it
beside your binary).

## Licensing

Erika is MPL-2.0. The bundled native libraries keep their own licenses
(`THIRD_PARTY_NOTICES.md`). Because Erika is open source with a reproducible
build, the LGPL components (FFmpeg, FriBidi, SoundTouch) satisfy the LGPL relinking
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
