# Erika Architecture

[中文](architecture.zh.md) | [English](architecture.md) | [日本語](architecture.ja.md)

Erika is an embeddable Rust media playback library. Host applications call into
the engine through the Rust API, a C ABI (`erika_capi`), or Flutter bindings
(`erika_flutter`). Video frames, subtitles, and danmaku stay inside the engine
and are composited in the renderer — they do not flow through the host.

## System Overview

```text
Rust Player Core
  source abstraction ─── file + HTTP range
  FFmpeg wrappers ────── custom AVIO, probe, demux, decode, seek, audio resample
  playback engine ────── video/audio tick, clock, frame scheduler
  video decode ───────── VideoToolbox (macOS/iOS), software fallback
  audio output ───────── CoreAudio (macOS), AudioQueue (iOS), ring buffer
  overlay timeline ───── subtitle + danmaku composition
  renderer core ──────── color state, render graph, tone map, scaler policy
  Metal renderer ─────── zero-copy NV12/P010, HDR/EDR, subtitle/danmaku pass
  wgpu renderer ──────── cross-platform video + danmaku rendering
  presenter runtime ──── ties player + renderer + audio + overlays
  C ABI ──────────────── 63 exported functions, two handle families
  Flutter plugin ─────── macOS + iOS native view embedding
```

## Native Dependencies

`xtask` downloads, builds, and installs native dependencies from pinned upstream
sources into `third_party/`. The default profile is `lgpl`.

| Dependency | Version | Purpose |
|------------|---------|---------|
| FFmpeg | 7.1.1 | Demux, decode, audio resample, VideoToolbox |
| libass | 0.17.3 | ASS subtitle rendering |
| FreeType | 2.13.3 | Font rasterization (libass dependency) |
| HarfBuzz | 10.4.0 | Text shaping (libass dependency) |
| FriBidi | 1.0.16 | Bidirectional text (libass dependency) |

All dependencies are statically linked. libass and its dependencies are enabled
by default (`features = ["libass"]`).

```sh
cargo run -p xtask -- deps build --all --profile lgpl
cargo run -p xtask -- deps status
```

## FFmpeg Integration

`erika_ffmpeg_sys` generates low-level bindings via bindgen at build time.
`erika::ffmpeg` provides safe Rust wrappers:

- **Demuxer** — owns `AVFormatContext`, optionally with a Rust-backed custom
  `AVIOContext` from `MediaSource`. Supports stream selection, reference-counted
  packets, and timestamp-based seek.
- **Decoder** — software and VideoToolbox hardware backends. Hardware frames
  preserve BT.2020/PQ metadata and carry `CVPixelBufferRef` for zero-copy Metal
  import.
- **AudioResampler** — wraps `libswresample`, converts to interleaved f32 PCM
  (default 48 kHz stereo).
- **SubtitleDecoder** — decodes embedded text and bitmap subtitle streams.

## Playback Engine

`PlaybackSession` opens media, selects tracks, configures decode backend, and
produces video frames and PCM audio blocks.

`VideoPlaybackEngine` adds clocked playback:

- Play, pause, stop, seek, playback rate control, EOF detection.
- `PlaybackClock` — media-time anchor with audio-master clock discipline
  (deadband correction, bounded per-frame adjustment, large-drift snap).
- `VideoFrameScheduler` — present/wait/drop decisions for decoded video frames.
- `DisplaySyncState` — vsync quantizer that carries residual frame-duration
  error across frames.

## Audio Output

- **macOS**: CoreAudio output with ring buffer and PTS-tracking clock snapshots.
  The presenter feeds CoreAudio output snapshots back to the player worker for
  audio-master clock discipline.
- **iOS**: AudioQueue output with the same ring buffer and clock snapshot model.
- Ring buffer: interleaved f32, configurable capacity, drop-oldest overflow
  policy, volume control.

## Subtitle System

- **Parsing**: SRT, WebVTT, ASS timeline parsing. Embedded and external subtitle
  tracks. External tracks can be added/removed at runtime.
- **libass renderer**: Statically linked, enabled by default. Accepts ASS
  scripts, calls `ass_render_frame`, imports alpha planes into Erika's overlay
  system. Uses CoreText font provider on Apple platforms.
- **SubtitleRendererCore**: Renderer-facing boundary that tracks changed/unchanged
  frames to avoid redundant GPU uploads.

## Danmaku System

The danmaku subsystem implements the NipaPlay DFM+ layout algorithm natively in
Rust. See `docs/danmaku_architecture.md` for the full design.

- **Input**: Bilibili XML, JSON, JSON-lines parsing.
- **DanmakuSession**: Multi-track management with per-track enable/disable,
  per-track offset, global offset.
- **DFM+ layout core**: Prepare/frame-query separation. Prepare processes the
  entire track once (measurement, filtering, duplicate merge, collision avoidance,
  lane allocation). Frame query returns positioned items for a given media time.
- **Text rasterizer**: Glyph atlas with fill and outline alpha masks, version
  tracking for GPU texture reuse.
- **Render plan**: `DanmakuRenderPlan` carries glyph instances with screen rects,
  atlas tex rects, colors, outline, shadow. Metal and wgpu renderers draw
  instanced quads from the atlas.

## Renderer

### Metal Renderer (macOS/iOS)

The primary renderer for Apple platforms:

- Zero-copy CVPixelBuffer → MTLTexture import via `CVMetalTextureCache`.
- YCbCr sampling, transfer decode, gamut mapping (BT.2020→BT.709, Display P3→BT.709).
- Tone mapping: Mobius, Reinhard, clip operators with absolute nits.
- SDR output (`BGRA8Unorm`) and Apple EDR output (`RGBA16Float` with EDR
  headroom).
- Neural luma upscaler (`LumaUpscalerMode`): ArtCNN C4F16/C4F32 2x doublers
  as Metal compute passes on the decoded Y plane, encoded on the same command
  buffer ahead of the render pass (`renderer/metal/upscaler.rs`). Chroma keeps
  its source resolution. Engages only when the video is displayed above source
  resolution; the network output is cached per decoded frame so repeated vsync
  ticks of the same frame skip the compute. Weights are converted from the
  upstream ONNX releases (`assets/artcnn/`) and verified against onnxruntime
  references (`tests/artcnn_upscaler.rs`). Two kernel backends: a
  `simdgroup_matrix` matmul implementation (default on Apple Silicon) and a
  scalar texture fallback; both are compiled on a background thread, so
  playback continues unscaled until the pipelines are ready.
- Subtitle overlay: RGBA plane upload and alpha blending.
- Danmaku: Instanced glyph quad drawing from atlas (shadow → outline → fill passes).
- Presentation layout preserves source aspect ratio.

### wgpu Renderer (cross-platform)

Second renderer backend for portability:

- Real `wgpu` dependency with device/surface/pipeline creation.
- NV12/P010 video frame upload and WGSL YCbCr conversion shader.
- Color space conversion, tone mapping (same pipeline model as Metal).
- Danmaku glyph atlas rendering.
- Offscreen render target for headless testing.
- Surface handle model covers macOS NSView, iOS UIView, Windows HWND,
  X11/Wayland, Android native windows.
- VideoToolbox zero-copy import and HDR/EDR output are not yet implemented in
  this backend.

### Render Pipeline

`renderer::pipeline` describes rendering decisions in Rust before any backend
consumes them:

- `SourceColorState` / `TargetColorState` — primaries, transfer, range.
- `VideoRenderPipeline` — gamut matrix, tone map operator, transfer functions.
- HDR metadata: mastering display, content light level, nominal peak nits.

## Presenter Runtime

`PresenterRuntime` ties together Player, MetalRenderer, OverlayTimeline,
DanmakuEngine, and audio output. The host supplies a native surface and drives
`render_tick` from a display timer.

- Pumps video frames, updates overlay (subtitle + danmaku), renders, presents.
- Danmaku plan generation is time-synchronized with video frames using
  generation + media_time gating.
- Supports playback rate, volume, track selection, subtitle/danmaku
  configuration at runtime.

## C ABI

`erika_capi` exports 63 functions through two handle families:

- **`ErikaHandle`** — player control and event polling. The host owns rendering.
- **`ErikaPresenterHandle`** — Erika owns the full stack. The host provides a
  surface and calls `render_tick`.

Covers: create/destroy, open/play/pause/stop/seek, track selection, subtitle
track add/remove, danmaku track management (add/remove/enable/offset/config),
surface attach/detach/resize, event polling, volume, playback rate, neural
luma upscaler switching, and upscaler backend status diagnostics.

Header: `crates/erika_capi/include/erika.h`

## Flutter Plugin

`packages/erika_flutter` provides macOS and iOS Flutter embedding:

- **Dart**: `ErikaPlayer` (commands + events), `ErikaWindowOverlayVideoView`
  (recommended window-hosted native Metal surface), and `ErikaVideoView`
  (compatibility platform view).
- **macOS Swift plugin**: Loads `liberika_capi.dylib`, creates either
  `NSWindow`-hosted overlay or `NSView`/`CAMetalLayer` platform view surfaces,
  and drives `render_tick` from a display link.
- **iOS Swift plugin**: Links `liberika_capi.a` statically, creates either
  `UIWindow`-hosted overlay or `UIView`/`CAMetalLayer` platform view surfaces,
  and uses the same presenter model.

See `docs/flutter_embedding.md` for the embedding model and HDR strategy.

## Platform Support

| Platform | Decode | Render | Audio | Status |
|----------|--------|--------|-------|--------|
| macOS 14+ | VideoToolbox | Metal | CoreAudio | Available |
| iOS 16+ | VideoToolbox | Metal | AudioQueue | Available |
| Windows | — | wgpu (planned) | — | Planned |
| Linux | — | wgpu (planned) | — | Planned |
| Android | — | wgpu (planned) | — | Planned |
