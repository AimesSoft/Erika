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
  video decode ───────── VideoToolbox, D3D11VA, MediaCodec, AVCodec, software fallback
  audio output ───────── CoreAudio, AudioQueue, WASAPI, AAudio, OHAudio, ring buffer
  overlay timeline ───── subtitle + danmaku composition
  renderer core ──────── color state, render graph, tone map, scaler policy
  Metal renderer ─────── zero-copy NV12/P010, HDR/EDR, subtitle/danmaku pass
  D3D11 renderer ─────── zero-copy D3D11VA, HDR10, subtitle/danmaku pass (Windows)
  wgpu renderer ──────── cross-platform video, overlays, capture, Android scRGB, OHOS Vulkan
  presenter runtime ──── ties player + renderer + audio + overlays
  C ABI ──────────────── 79 exported functions, two handle families
  Flutter plugin ─────── macOS + iOS + Windows + Android + OpenHarmony embedding
```

## Native Dependencies

`xtask` downloads, builds, and installs native dependencies from pinned upstream
sources into `third_party/`. The default profile is `lgpl`.

| Dependency | Version | Purpose |
|------------|---------|---------|
| FFmpeg | 8.1.2 | Demux, decode, audio resample, platform hardware decode |
| dav1d | 1.5.1 | Android AV1 software fallback (8-bit and high bit depth) |
| libass | 0.17.5 | ASS subtitle rendering |
| FreeType | 2.14.3 | Font rasterization (libass dependency) |
| HarfBuzz | 14.2.1 | Text shaping (libass dependency) |
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
- **Decoder** — software plus VideoToolbox, D3D11VA, MediaCodec, and OpenHarmony
  AVCodec hardware backends. Android software AV1 explicitly selects the source-built
  `libdav1d` decoder. Hardware frames preserve color metadata for the renderer's
  platform-specific import or upload path.
- **AudioResampler** — wraps `libswresample`, converts to interleaved f32 PCM
  (default 48 kHz stereo).
- **SubtitleDecoder** — decodes embedded text and bitmap subtitle streams.

## Playback Engine

`PlaybackSession` opens media, selects tracks, configures decode backend, and
produces video frames and PCM audio blocks.

Decoder availability is a session invariant: when a video track is selected,
the play, seek, and video-frame-pump entry points require an active video
decoder. Destructive MediaCodec transitions, including seek reopen and
Surface-to-ByteBuffer/software fallback, first record a decoder-unavailable
reason. If the final software decoder open also fails, those entry points
return that explicit error and require the media to be reopened; they never
enter an audio-only false `Playing` state.

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
  system. macOS uses the CoreText font provider and Windows DirectWrite; every
  other target runs without a system font provider (the vendored libass build
  disables fontconfig), so Erika's bundled Droid Sans Fallback — registered as
  an in-memory font on all platforms — is the family libass resolves by default.
- **Subtitle style**: A custom font family, font file, font size, outline width
  and text/outline colours act as fallbacks that fill in what a script leaves
  open, plus the styling of plain-text subtitles; the subtitle scale multiplies
  the metrics. `force_override` promotes them to libass' selective style
  override so they also replace what ASS dialogue events request.
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
  Blob validation, model layout, execution policy, and frame-token caching live
  in the backend-neutral `renderer/artcnn.rs` module and are also consumed by
  the wgpu implementation.
- Subtitle overlay: RGBA plane upload and alpha blending.
- Danmaku: Instanced glyph quad drawing from atlas (shadow → outline → fill passes).
- Presentation layout preserves source aspect ratio.

### Direct3D 11 Renderer (Windows)

The native renderer for Windows (`renderer/d3d11.rs`):

- Zero-copy D3D11VA decode-texture interop: decoded `ID3D11Texture2D` surfaces
  are shared into the render device, no CPU round-trip.
- YCbCr sampling and color space conversion (HLSL shaders), same pipeline model
  as Metal.
- HDR10 output via an `R10G10B10A2_UNORM` swapchain with `DXGI_HDR_METADATA_HDR10`,
  with SDR (`BGRA8`) fallback.
- Subtitle overlay alpha-atlas upload and blending; instanced danmaku glyph
  quad drawing from the atlas.
- Window-hosted swapchain driven by `render_tick`.

### wgpu Renderer (cross-platform)

Second renderer backend for portability:

- Real `wgpu` dependency with device/surface/pipeline creation.
- NV12/P010 video frame upload and WGSL YCbCr conversion shader.
- Android MediaCodec Surface import through AHardwareBuffer/Vulkan, with an
  explicit ByteBuffer/CPU-upload path when native interop is unavailable.
- The shared `renderer/frame.rs` boundary carries geometry/color metadata plus
  either a decoded FFmpeg frame or an independent prepared AHardwareBuffer.
  MediaCodec Surface AVFrames are released on the playback worker while their
  decoder callback context is still alive; presenter and GPU recovery never
  retain the decoder-owned AVFrame.
- Color space conversion, tone mapping (same pipeline model as Metal).
- Tiled ArtCNN C4F16/C4F32 compute (`renderer/wgpu_artcnn.rs`) with bounded
  feature textures and a source-sized packed DepthToSpace output. It accepts
  both native luma planes and Android's converted nonlinear RGB texture while
  preserving chroma as `rgb + (Y_sr - Y)`. GLES 3.0 reports `Inactive` with a
  structured `native_luma_sampling` fallback instead of attempting compute.
- Subtitle/danmaku compositing, frame capture, and offscreen headless tests.
  Capture always renders to an SDR RGBA8 target, including when the display
  surface is extended-linear, so screenshots never expose unclamped scRGB
  values as if they were SDR pixels.
- Surface handle model covers macOS NSView, iOS UIView, Windows HWND,
  X11/Wayland, Android native windows, OpenHarmony `OHNativeWindow`.
- OpenHarmony imports AVCodec Surface output as an `OHNativeBuffer`-backed
  Vulkan external image and resolves YUV on the GPU with a Vulkan YCbCr
  sampler, so decoded frames reach the wgpu compositor without a CPU copy.
  Devices without the required Vulkan extensions fall back to software decode
  and CPU upload, and the fallback is reported through the diagnostics events.
- Android has bounded Vulkan/GLES backend recovery and explicit import,
  capability, quality-reduction, and device-failure diagnostics. Its
  high-headroom output is FP16 **extended-linear scRGB**, not HDR10/PQ: the
  renderer uses `Rgba16Float`, Vulkan's extended-sRGB-linear color space, and
  verifies `ADATASPACE_SCRGB_LINEAR` (`0x18410000`) on the `ANativeWindow` after
  every configure/reconfigure. Android scRGB uses BT.709 primaries with
  `1.0 = 80 nit`; it does not emit PQ or HDR10 static metadata.
- Extended-linear activation requires an explicit `ExtendedLinear` request, an
  HDR-capable display/surface, a `SurfaceView` hosted with Flutter Hybrid
  Composition, the Vulkan wgpu backend, `Rgba16Float` surface support, and a
  successful `SCRGB_LINEAR` dataspace readback. A missing condition selects the
  normal SDR surface immediately and records one of the stable fallback reason
  codes `0..8`; GLES and `TextureView` are therefore SDR paths.
- On API 34+, the Android host observes the display with
  `Display.registerHdrSdrRatioChangedListener` and publishes real changes
  through `erika_presenter_set_output_headroom`. wgpu updates the effective
  content headroom used by subsequent frames and the queryable output status
  without reattaching the surface. When the ratio is available,
  `activeHeadroomKnown` is true; `headroomUpdates` grows only when the known
  flag or ratio actually changes.
- A Flutter extended-linear player with no explicit `edrHeadroom` uses a 4x
  content ceiling while passing `0` to the `SurfaceView` as system-auto desired
  headroom. An explicit value becomes the content ceiling and, on API 35, the
  per-`SurfaceView` desired headroom; Erika never changes the global window.
- Emulator/non-HDR coverage verifies the explicit SDR fallback path. Active
  `Rgba16Float + SCRGB_LINEAR` presentation still requires acceptance on an
  API 35 HDR device before it is described as device-validated.

### Render Pipeline

`renderer::pipeline` describes rendering decisions in Rust before any backend
consumes them:

- `SourceColorState` / `TargetColorState` — primaries, transfer, range.
- `VideoRenderPipeline` — gamut matrix, tone map operator, transfer functions.
- `renderer::output` — requested mode, active encoding, surface format,
  dataspace/headroom state, and stable fallback diagnostics shared by the
  native renderers and wgpu.
- HDR metadata: mastering display, content light level, nominal peak nits.

## Presenter Runtime

`PresenterRuntime` ties together Player, MetalRenderer, OverlayTimeline,
DanmakuEngine, and audio output. The host supplies a native surface and drives
`render_tick` from a display timer.

- Pumps video frames, updates overlay (subtitle + danmaku), renders, presents.
- Decoder-changing operations use a quiesce/ACK barrier, discard renderer and
  receiver state, perform the transition, then resume. Playback generations
  remain monotonic across reopen and stale import feedback is gated by both
  generation and the exact MediaCodec route.
- Danmaku plan generation is time-synchronized with video frames using
  generation + media_time gating.
- Supports playback rate, volume, track selection, subtitle/danmaku
  configuration at runtime.

## C ABI

`erika_capi` exports 79 functions through two handle families:

- **`ErikaHandle`** — player control and event polling. The host owns rendering.
- **`ErikaPresenterHandle`** — Erika owns the full stack. The host provides a
  surface and calls `render_tick`.

Covers: create/destroy, open/play/pause/stop/seek, track selection, subtitle
track add/remove, danmaku track management (add/remove/enable/offset/config),
surface attach/detach/resize, event polling, volume, playback rate, neural
luma upscaler switching, upscaler diagnostics, and the 13-field output status
snapshot returned by `erika_presenter_get_output_status`.

Header: `crates/erika_capi/include/erika.h`

## Flutter Plugin

`packages/erika_flutter` provides macOS, iOS, Windows, Android, and HarmonyOS
Flutter embedding:

- **Dart**: `ErikaPlayer` (commands + events), `ErikaWindowOverlayVideoView`
  (recommended window-hosted native surface — Metal on Apple, D3D11 swapchain on
  Windows), and `ErikaVideoView` (compatibility platform view).
- **macOS Swift plugin**: Loads `liberika_capi.dylib`, creates either
  `NSWindow`-hosted overlay or `NSView`/`CAMetalLayer` platform view surfaces,
  and drives `render_tick` from a display link.
- **iOS Swift plugin**: Links `liberika_capi.a` statically, creates either
  `UIWindow`-hosted overlay or `UIView`/`CAMetalLayer` platform view surfaces,
  and uses the same presenter model.
- **Windows C++ plugin** (`ErikaFlutterPluginCApi`): builds and links
  `erika_capi.dll` via CMake (`build_erika_runtime.cmake`, cargo target
  `x86_64-pc-windows-msvc`), hosts a window-level D3D11 swapchain, and drives
  `render_tick` from a frame scheduler.
- **Android Kotlin/JNI plugin**: builds the Rust runtime for Android ABIs and
  gives each player an independent native surface. SDR uses `TextureView`;
  requested extended-linear output uses `SurfaceView` through Flutter Hybrid
  Composition. The plugin coordinates Activity surface lifecycle, audio focus,
  noisy-route policy, HDR eligibility/headroom, and drives presentation from a
  shared frame scheduler only while players are active.
- **HarmonyOS ArkTS/N-API plugin**: builds `liberika_capi.so` for
  `aarch64-unknown-linux-ohos` through Hvigor/CMake, registers a Flutter
  external texture, and attaches that texture's `OHNativeWindow` to the
  presenter. Audio goes through OHAudio as interleaved f32 PCM.

See `docs/flutter_embedding.md` for the embedding model and HDR strategy.

## Platform Support

| Platform | Decode | Render | Audio | Status |
|----------|--------|--------|-------|--------|
| macOS 14+ | VideoToolbox | Metal | CoreAudio | Available |
| iOS 16+ | VideoToolbox | Metal | AudioQueue | Available |
| Windows 10+ | D3D11VA | Direct3D 11 | WASAPI | Available |
| Linux | — | wgpu (planned) | — | Planned |
| Android 8+ | MediaCodec / software | wgpu Vulkan with GLES fallback | AAudio | Available; SDR validated, extended-linear scRGB implementation awaits API 35 HDR-device acceptance |
| HarmonyOS API 18+ | AVCodec (H.264/HEVC) / software | wgpu Vulkan, `OHNativeBuffer` zero-copy import | OHAudio | Available; validated on device, not yet covered by CI |
