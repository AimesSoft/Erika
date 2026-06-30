# erika_flutter

Flutter plugin for the Erika media playback engine.

The plugin keeps Dart out of the hot path:

- Dart exposes low-frequency player commands and event streams.
- The native plugins expose two surface strategies: `ErikaWindowOverlayVideoView`
  for the recommended window-hosted overlay path (Metal on macOS/iOS, a D3D11
  swapchain on Windows), and `ErikaVideoView` for compatibility platform-view
  embedding.
- The macOS plugin loads the Erika dynamic library.
- The iOS plugin links the Erika static library.
- The Windows plugin builds and links the Erika C ABI DLL.
- Erika owns playback, rendering, audio, timing, and overlays through
  `ErikaPresenterHandle`.

## Video Surfaces

Use `ErikaWindowOverlayVideoView` for full-player macOS/iOS UIs. It reserves a
Flutter layout rect while the plugin hosts a sibling native `CAMetalLayer`, so
video stays outside Flutter's platform-view compositor.

On Windows `ErikaWindowOverlayVideoView` hosts a window-level Direct3D 11
swapchain as a sibling surface, following the same overlay model.

Use `ErikaVideoView` when a standard Flutter platform view is required for a
small embedder, compatibility path, or diagnostics.

## macOS Setup

The macOS plugin's podspec build phase builds the universal
`liberika_capi.dylib` from source (or downloads a prebuilt one — see below) and
bundles it into the app's `Contents/Frameworks`, codesigned, during the macOS
app build. At runtime the plugin loads it via `dlopen`.

Overrides: `ERIKA_CAPI_DYLIB` forces the runtime dylib path; `ERIKA_MACOS_CAPI_DYLIB`
points the build phase at an explicit dylib to bundle instead of building.

## Prebuilt binaries (opt-in)

To skip building Erika (and FFmpeg) from source, set `ERIKA_PREBUILT=1` in the
app build to download the prebuilt `erika_capi` from a GitHub Release
(`ERIKA_PREBUILT_TAG` selects the tag, default `v0.1.0`). Supported on macOS,
Windows, and iOS; any failure falls back to the source build. See
[`docs/releasing.md`](../../docs/releasing.md).

When debugging local Erika source changes on macOS or iOS, set
`ERIKA_FORCE_SOURCE_BUILD=1` to bypass the prebuilt download path even if the
host app enables `ERIKA_PREBUILT=1`.

## iOS Setup

The iOS CocoaPod script phase builds the Erika native dependencies and C ABI
static library automatically during Xcode builds. Requirements:

- Rust toolchain with the appropriate iOS target (`rustup target add aarch64-apple-ios`)

## Windows Setup

The Windows plugin (`ErikaFlutterPluginCApi`) builds the Erika C ABI runtime
(`erika_capi.dll`) during the CMake build via `build_erika_runtime.cmake`,
invoking cargo for the `x86_64-pc-windows-msvc` target and staging the DLL next
to the app. Requirements:

- Rust toolchain with the MSVC target (`rustup target add x86_64-pc-windows-msvc`)
- Visual Studio Build Tools (MSVC) + Windows SDK
- Native dependencies built into `third_party/dist/x86_64-pc-windows-msvc/`
  (via the repo `xtask deps build` flow)

Set `ERIKA_REPO_ROOT` if the plugin cannot locate the Erika checkout
automatically.

## Output Mode

`ErikaPlayer()` lets the macOS plugin choose SDR or Apple EDR from the current
screen and environment. To force EDR from Dart:

```dart
final player = ErikaPlayer(
  outputMode: ErikaOutputMode.appleEdr,
  edrHeadroom: 4.0,
);
```

Use `ErikaOutputMode.sdr` to force SDR output.

## Upscaler

Enable ArtCNN upscaling from Dart after creating a player:

```dart
await player.setUpscaler(ErikaUpscalerMode.artCnnC4F16);
```

Use `ErikaUpscalerMode.off` to disable it. Call
`player.getUpscalerStatus()` to inspect the requested mode, active backend,
fallback count, upscaled frame count, and recent GPU timings.
