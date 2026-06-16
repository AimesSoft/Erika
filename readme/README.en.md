[中文](../README.md) | [English](README.en.md) | [日本語](README.ja.md)

# Erika

**The in-house playback core of NipaPlay.** Written in Rust, embeddable, handling everything from decode to render.

> Named after the detective **Furude Erika** from *Umineko When They Cry*.
> [NipaPlay](https://github.com/AimesSoft/NipaPlay-Reload) takes its name from **Furude Rika**'s catchphrase "nipah~☆" in *Higurashi When They Cry* — the community simply calls her "Rika".
> One is the player the audience sees; the other is the engine behind the curtain. Two sides of the same coin, from the same universe.

The host application provides a rendering surface and sends playback commands — decoding, timing, video rendering, subtitles, danmaku, and audio output are handled entirely inside Erika, without passing through the host's rendering pipeline.

## Features

- **Hardware-accelerated decoding** -- VideoToolbox (macOS/iOS), extensible to other platform backends
- **Zero-copy rendering** -- CVPixelBuffer to MTLTexture passthrough, video frames never round-trip through CPU memory
- **HDR/EDR output** -- native Apple EDR support, PQ (BT.2020) metadata preservation and tone mapping
- **Native Metal renderer** -- YCbCr sampling, color space conversion, tone mapping, subtitle/danmaku compositing in a single render pass
- **Neural upscaling** -- ArtCNN anime luma 2x super-resolution, Metal compute kernels, simdgroup-matrix and scalar backends, luma-plane-only and zero-copy into the render pipeline
- **Audio output** -- CoreAudio (macOS) / AudioQueue (iOS), f32 PCM ring buffer, audio clock synchronization
- **Subtitles** -- SRT / WebVTT / ASS parsing, libass rendering (statically linked), embedded and external subtitle tracks
- **Danmaku** -- Bilibili XML / JSON parsing, DFM+ collision-aware lane layout engine, glyph atlas native GPU rendering
- **Playback engine** -- play / pause / stop / seek / rate control, audio-master clock discipline, vsync-quantized frame scheduling
- **C ABI** -- 63 exported functions, opaque handle design, callable from C / C++ / Swift / Dart FFI / any FFI-capable language
- **Flutter plugin** -- macOS + iOS native view embedding, HDR native layer path support
- **wgpu backend** -- cross-platform rendering foundation in place (Windows / Linux / Android direction)

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

// In your widget tree:
ErikaVideoView(player: player)
```

## C ABI Families

Erika provides two C ABI entrypoint families for different embedding scenarios:

| Family | Use Case | Rendering |
|--------|----------|-----------|
| `ErikaHandle` | Host manages its own render loop | Host pulls frame data |
| `ErikaPresenterHandle` | Erika owns the full playback stack | Host provides a surface and drives `render_tick` |

Header: [`crates/erika_capi/include/erika.h`](crates/erika_capi/include/erika.h)

## Platform Support

| Platform | Decode | Render | Audio | Status |
|----------|--------|--------|-------|--------|
| macOS 14+ | VideoToolbox | Metal | CoreAudio | **Available** |
| iOS 16+ | VideoToolbox | Metal | AudioQueue | **Available** |
| Windows | -- | wgpu (planned) | -- | Planned |
| Linux | -- | wgpu (planned) | -- | Planned |
| Android | -- | wgpu (planned) | -- | Planned |

## Repository Structure

```
crates/erika              Core playback library
crates/erika_capi         C ABI export layer
crates/erika_ffmpeg_sys   Low-level FFmpeg bindings
packages/erika_flutter    Flutter plugin (macOS + iOS)
examples/                 Validation and demo programs
xtask/                    Native dependency build orchestration
docs/                     Architecture and embedding documentation
```

## Building

### Prerequisites

- Rust 1.92+
- Xcode Command Line Tools (macOS/iOS)
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
export SAMPLE="/path/to/video.mp4"
cargo run -p macos_native_demo -- "$SAMPLE"
cargo run -p macos_native_demo -- --smoke-seconds 3 "$SAMPLE"
```

## License

Rust workspace: [MPL-2.0](../LICENSE)

Native dependency build profiles and license boundaries are managed independently through `xtask`.
