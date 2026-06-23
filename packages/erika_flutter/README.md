# erika_flutter

Flutter plugin for the Erika media playback engine.

The plugin keeps Dart out of the hot path:

- Dart exposes low-frequency player commands and event streams.
- The Apple plugins expose two native Metal surface strategies:
  `ErikaWindowOverlayVideoView` for the recommended window-hosted overlay path,
  and `ErikaVideoView` for compatibility platform-view embedding.
- The macOS plugin loads the Erika dynamic library.
- The iOS plugin links the Erika static library.
- Erika owns playback, rendering, audio, timing, and overlays through
  `ErikaPresenterHandle`.

## Video Surfaces

Use `ErikaWindowOverlayVideoView` for full-player macOS/iOS UIs. It reserves a
Flutter layout rect while the plugin hosts a sibling native `CAMetalLayer`, so
video stays outside Flutter's platform-view compositor.

Use `ErikaVideoView` when a standard Flutter platform view is required for a
small embedder, compatibility path, or diagnostics.

## macOS Setup

For local development the macOS plugin loads Erika through `dlopen`.
Set `ERIKA_CAPI_DYLIB` to override the dynamic library path. If unset, the
plugin searches the app bundle, the executable directory, and then
`$WORKSPACE/target/debug/liberika_capi.dylib`.

Build the dynamic library:

```sh
cargo run -p xtask -- deps build --all --profile lgpl
cargo build -p erika_capi
```

## iOS Setup

The iOS CocoaPod script phase builds the Erika native dependencies and C ABI
static library automatically during Xcode builds. Requirements:

- Rust toolchain with the appropriate iOS target (`rustup target add aarch64-apple-ios`)

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
