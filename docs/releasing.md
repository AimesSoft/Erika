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

Every archive also includes `include/erika.h`, `LICENSE` (Erika, MPL-2.0),
`THIRD_PARTY_NOTICES.md`, and a `MANIFEST.txt` recording the tag/commit.

The native dependencies (FFmpeg, libass, FreeType, HarfBuzz, FriBidi, zlib) are
**statically linked** via the `lgpl` profile, so each library is self-contained
except for OS frameworks (VideoToolbox/Metal/CoreAudio on Apple; Direct3D 11 /
WASAPI on Windows), which are always present on the target OS.

Linux is **not yet published**: `xtask`'s native dependency build targets only
Apple and Windows (it forces `--enable-videotoolbox` on non-Windows). Adding a
Linux artifact requires Linux support in `xtask` first.

## How to cut a release

The release is fully automated by
[`.github/workflows/release.yml`](../.github/workflows/release.yml).

1. Make sure `main` is green and the docs/version are up to date. Bump
   `version` in the root `Cargo.toml` if appropriate.
2. Tag and push:
   ```sh
   git tag v0.1.0
   git push origin v0.1.0
   ```
3. The workflow builds the macOS / iOS / Windows bundles (each builds the native
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
