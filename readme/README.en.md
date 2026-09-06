[中文](../README.md) | [English](README.en.md) | [日本語](README.ja.md)

# Erika

> "GOOD! I'm Erika, the fifth player kernel in NipaPlay after mdk, video player, libmpv, and media kit."
> "Even counting you, there are only four player kernels!"

**The in-house playback core of NipaPlay.** Written in Rust, embeddable, handling everything from decode to render.

> Named after the detective **Furude Erika** from *Umineko When They Cry*.
> [NipaPlay](https://github.com/AimesSoft/NipaPlay-Reload) takes its name from **Furude Rika**'s catchphrase "nipah~☆" in *Higurashi When They Cry* — the community simply calls her "Rika".
> One is the player the audience sees; the other is the engine behind the curtain. Two sides of the same coin, from the same universe.

The host application provides a rendering surface and sends playback commands — decoding, timing, video rendering, subtitles, danmaku, and audio output are handled entirely inside Erika, without passing through the host's rendering pipeline.

## Features

- **Hardware-accelerated decoding** -- VideoToolbox (macOS/iOS/tvOS), D3D11VA (Windows), MediaCodec (Android), and AVCodec (HarmonyOS), with explicit software-decode fallback when interop is unavailable
- **Zero-copy rendering** -- CVPixelBuffer to MTLTexture (Apple), D3D11VA texture interop (Windows), MediaCodec Surface to AHardwareBuffer/Vulkan (Android), and AVCodec Surface to OHNativeBuffer/Vulkan (HarmonyOS), with explicit CPU-upload fallback when import fails
- **HDR/EDR output** -- Apple EDR, Windows HDR10, and Android FP16 extended-linear scRGB negotiation with explicit SDR fallback
- **Native Metal renderer** -- YCbCr sampling, color space conversion, tone mapping, subtitle/danmaku compositing in a single render pass (macOS/iOS/tvOS)
- **Native Direct3D 11 renderer** -- Windows: D3D11VA zero-copy texture interop, YCbCr sampling, HDR10 output, subtitle/danmaku overlay compositing
- **Neural upscaling** -- ArtCNN anime luma 2x super-resolution using Metal, D3D11, and wgpu/Vulkan compute, integrated into the rendering pipeline
- **Audio output** -- CoreAudio (macOS) / AudioQueue (iOS/tvOS) / WASAPI (Windows) / AAudio (Android) / OHAudio (HarmonyOS), f32 PCM ring buffer, audio clock synchronization
- **Subtitles** -- SRT / WebVTT / ASS parsing, libass rendering (statically linked), embedded and external subtitle tracks
- **Danmaku** -- Bilibili XML / JSON parsing, DFM+ collision-aware lane layout engine, glyph atlas native GPU rendering
- **Playback engine** -- play / pause / stop / seek / rate control, audio-master clock discipline, vsync-quantized frame scheduling
- **C ABI** -- opaque handle design with a versioned public header; callable from C / C++ / Swift / Dart FFI / any FFI-capable language. See `erika.h` for the authoritative export set.
- **Flutter plugin** -- macOS + iOS + tvOS + Windows + Android + HarmonyOS native view/Texture embedding with platform-native high-dynamic-range surface paths
- **wgpu backend** -- Android playback, overlays, capture, and bounded Vulkan/GLES recovery are available; HarmonyOS runs on Vulkan, presenting through OHNativeWindow with OHNativeBuffer zero-copy import; Linux remains planned

## Quick Start

### Rust

```rust
use erika::{Player, PlayerConfig, MediaRequest};

let player = Player::new(PlayerConfig::default())?;
player.open(MediaRequest::file("/path/to/video.mp4"))?;
player.play()?;
```

### C ABI

```c
#include "erika.h"

ErikaPresenterHandle *presenter = erika_presenter_create();
erika_presenter_attach_metal_layer(presenter, (uint64_t)layer, w, h, scale);
erika_presenter_open(presenter, "/path/to/video.mp4");
erika_presenter_play(presenter);

// On every display tick:
ErikaPresenterStats stats;
erika_presenter_render_tick(presenter, host_time, &stats);
```

### Flutter

```dart
final player = ErikaPlayer();
await player.open('/path/to/video.mp4');
await player.play();

// Recommended for full-player UIs: keep video in Erika's native Metal layer.
ErikaWindowOverlayVideoView(player: player)

// Compatibility/diagnostics: Flutter platform-view embedding remains available.
ErikaVideoView(player: player)
```

### Flutter package

`erika_flutter` is available on [pub.dev](https://pub.dev/packages/erika_flutter)
for macOS, iOS, tvOS, Windows, Android, and HarmonyOS/OpenHarmony. Add it to a
Flutter app with:

```sh
flutter pub add erika_flutter
```

The package downloads the matching verified native runtime during the platform
build. Android downloads only the selected ABI archive; Linux and Web are not
published targets yet.

### OpenHarmony package

The native ArkTS package `erika` is published on
[OHPM](https://ohpm.openharmony.cn/#/cn/detail/erika) for OpenHarmony arm64
applications (API 18+). Install it with:

```sh
ohpm install erika
```

See the [OpenHarmony package guide](../packages/erika_ohos/README.md) for the
`ErikaPlayer` API and `XComponent` surface setup.

## C ABI Families

Erika provides two C ABI entrypoint families for different embedding scenarios:

| Family | Use Case | Rendering |
|--------|----------|-----------|
| `ErikaHandle` | Host manages its own render loop | Host pulls frame data |
| `ErikaPresenterHandle` | Erika owns the full playback stack | Host provides a surface and drives `render_tick` |

Header: [`crates/erika_capi/include/erika.h`](../crates/erika_capi/include/erika.h)

## Platform Support

| Platform | Decode | Render | Audio | Status |
|----------|--------|--------|-------|--------|
| macOS 14+ | VideoToolbox | Metal | CoreAudio | **Available** |
| iOS 16+ | VideoToolbox | Metal | AudioQueue | **Available** |
| tvOS 13+ (Apple TV) | VideoToolbox | Metal | AudioQueue | **Available** |
| Windows 10+ | D3D11VA | Direct3D 11 | WASAPI | **Available** |
| Linux | -- | wgpu (planned) | -- | Planned |
| Android 8+ | MediaCodec / software | wgpu (Vulkan + GLES fallback) | AAudio | **Available**; SDR verified, extended-linear scRGB implemented, API 35 HDR-device active-path acceptance pending |
| HarmonyOS API 18+ | AVCodec (H.264/HEVC) / software | wgpu (Vulkan) + `OHNativeBuffer` zero-copy import | OHAudio | **Available**; validated on device, not yet covered by CI |

## Repository Structure

```
crates/erika              Core playback library
crates/erika_capi         C ABI export layer
crates/erika_ffmpeg_sys   Low-level FFmpeg bindings
packages/erika_flutter    Flutter plugin (macOS + iOS + tvOS + Windows + Android + HarmonyOS)
packages/erika_ohos       OpenHarmony ArkTS / OHPM package
examples/                 Validation and demo programs
xtask/                    Native dependency build orchestration
docs/                     Architecture and embedding documentation
```

## Documentation

- [Architecture](../docs/architecture.md) — engine design, render backends, platform support
- [C ABI Reference](../docs/capi_reference.md) — every export, status codes, ownership & threading
- [Integration Guide](../docs/integration.md) — embedding in C/C++/Win32/Swift and other non-Flutter hosts
- [Build Guide](../docs/building.md) — xtask, native deps, cross-compilation
- [Flutter Embedding](../docs/flutter_embedding.md) · [Danmaku Architecture](../docs/danmaku_architecture.en.md)
- [Releasing & Prebuilt Binaries](../docs/releasing.md) — downloadable per-platform `erika_capi` libraries and packaging
- [Contributing / Developer Guide](../CONTRIBUTING.md) — repo layout, threading model, adding a platform backend

## Building

### Prerequisites

- Rust 1.92+
- Xcode Command Line Tools (macOS/iOS/tvOS)
- MSVC toolchain + Windows SDK (Windows, target `x86_64-pc-windows-msvc`)
- Android SDK + NDK r29 and the corresponding Android Rust targets
- DevEco Studio OpenHarmony Native SDK and the Rust `aarch64-unknown-linux-ohos` target
- CMake, pkg-config

### Build Native Dependencies

```sh
# Build FFmpeg (LGPL profile)
cargo run -p xtask -- deps build --profile lgpl

# Build all dependencies (including libass/FreeType/HarfBuzz/FriBidi)
cargo run -p xtask -- deps build --all --profile lgpl

# Check dependency status
cargo run -p xtask -- deps status
```

### Compile and Test

```sh
cargo build -p erika
cargo test --workspace
```

### Verify Playback Path

```sh
# macOS
export SAMPLE="/path/to/video.mp4"
cargo run -p macos_native_demo -- "$SAMPLE"
cargo run -p macos_native_demo -- --smoke-seconds 3 "$SAMPLE"

# Windows
cargo run -p windows_native_demo -- "%SAMPLE%"
```

## License

Rust workspace: [MPL-2.0](../LICENSE)

Native dependency build profiles and license boundaries are managed independently through `xtask`.
