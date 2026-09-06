# Changelog

## Unreleased

### C API

- Added `ErikaOpenOptions` and `erika_open_with_options` /
  `erika_presenter_open_with_options`, superseding the `_with_headers` pair.
  The options struct bundles the header array with per-request tuning, starting
  with `http_read_ahead_bytes` to override the HTTP(S) read-ahead window
  (0 uses `ERIKA_HTTP_READAHEAD_BYTES` when set, otherwise the 2 MiB default;
  an explicit value supersedes the environment variable). Reserved fields must
  be zero and are validated so future fields cannot silently change behavior.

### Flutter and OpenHarmony

- Added `httpReadAheadBytes` to Flutter `ErikaPlayer.open` on Android, Apple
  platforms, Windows, and OpenHarmony, and exposed matching open options in the
  standalone OpenHarmony SDK.

### Danmaku

- Kept both accepted and rejected placement decisions stable across sliding
  planner windows, preventing dropped comments from reappearing mid-flight or
  forcing visible comments onto another lane.
- Decoupled host-provided danmaku IDs from internal layout identity and parse
  optional JSON IDs as exact `u64` values without floating-point rounding.

### Playback

- Audio output now has a dedicated owner for PCM delivery, clock observations,
  device recovery and playback-rate commits. Rendering stalls no longer stop
  audio feeding. Acknowledged control boundaries preserve pause, seek, track
  changes, EOF/replay and shutdown ordering with the existing 250 ms rate bridge.
- Host-provided Metal layers explicitly retain the finite drawable timeout
  (up to one second before returning nil); skipped presents are counted with
  throttled diagnostics. Audio delivery does not depend on that wait finishing.
- Recover audio-master clock drift after a blocked render tick without increasing
  audio queue depth. Presenter feedback retains its capture time, playback intent,
  generation, and output epoch; stale feedback and silence-only callbacks cannot
  drive clock correction. Each new output first establishes a progress baseline.
- When the first audio stream cannot be decoded, try the remaining audio
  streams in container order and report all decoder failures if none can be
  opened.
- Added packed-alpha video presentation with premultiplied GPU output on
  Windows and macOS, including native backdrop-aware overlay composition.

## 0.1.7 - 2026-08-16

### Flutter package distribution

- Prepared `erika_flutter` for its first pub.dev release with package-local
  licensing, changelog, metadata, examples, and standalone package tests.
- Made verified, version-pinned native release bundles the default on Android,
  Apple platforms, Windows, and OpenHarmony; source builds are now explicitly
  selected with `ERIKA_FORCE_SOURCE_BUILD=1`.
- Added per-ABI Flutter Android runtime archives so app builds download only
  the requested architecture and omit the native-embedder static library.
- Added a release-level `SHA256SUMS` manifest covering every published native
  archive.
- Added isolated GitHub Actions consumers for Android, iOS, macOS, tvOS,
  Windows, and OpenHarmony so package builds cannot depend on the monorepo.

### OpenHarmony package distribution

- Published the independent native ArkTS package `erika` to OHPM for
  OpenHarmony arm64 (API 18+), with `XComponent` surface lifecycle,
  `renderTick()` scheduling, playback controls, events, and screenshots.

## 0.1.6 - 2026-08-14

### Rendering and playback

- Added the ArtCNN C4F16 DS denoising/sharpening luma upscaler for degraded
  anime sources across the Rust, C, Flutter, Metal, and wgpu interfaces.
- Moved Android and macOS presenter rendering off the application UI/main
  thread, reduced Android AHardwareBuffer conversion bandwidth, and prevented
  D3D11 frame reuse across devices.
- Added source-aware SDR/EDR output selection on Apple platforms and exposed
  resource status through the Android, Windows, and OpenHarmony integrations.

### Integration and behavior

- Disabled danmaku scroll overwrite by default, preserving already-stable
  comments during dense playback.
- Restored the Windows system-media-controls build and made the OpenHarmony
  Flutter plugin's ArkTS declarations explicit.

## 0.1.5 - 2026-08-03

### System media controls and background audio

- Added Now Playing metadata, playback state, timeline, play/pause/seek, and
  previous/next navigation controls across iOS, tvOS, macOS, Android, Windows,
  and OpenHarmony.
- Added opt-in background audio playback for iOS and Android while suspending
  video decoding, with bounded playback-worker barriers so lifecycle callbacks
  cannot block an application thread indefinitely.
- Cleared stale metadata when opening media without `ErikaMediaMetadata`, made
  pause/play sequencing deterministic, and isolated Android media callbacks
  across multiple Flutter engines.
- Cached Android artwork and avoided rebuilding media notifications when native
  playback state has not changed.

### Platform support and release artifacts

- Added the tvOS Flutter plugin and native target support for Apple TV devices
  and arm64/x86_64 simulators, including Metal presentation, Apple audio and
  VideoToolbox integration, subtitles, danmaku, screenshots, and HTTP sources.
- Added `erika-capi-tvos.zip`, containing a tvOS device and universal simulator
  `erika_capi.xcframework`, to the automated GitHub Release workflow.
- Added opt-in OpenHarmony prebuilt consumption: the plugin CMake build can
  download the tagged `liberika_capi.so`, stage it beside
  `liberika_flutter.so`, and fall back to a source build on failure.
- Unified macOS, iOS, and tvOS CI/release builds on macOS 26 runners; macOS x64
  is cross-built alongside arm64 before the universal archive is assembled.
- Fixed tvOS platform detection during native dependency builds and accepted
  valid zero-length HTTP resources without misclassifying them as truncated.

### Subtitle memory fonts

- Added an in-memory subtitle font registry across Rust, the C API, Dart, and
  every native Flutter embedding. Hosts can inspect font faces, select ordered
  fallback sets, replace font data, and clear the registry without writing font
  files to disk.
- Preserved ordered fallback in the bundled libass build, shared TTC/OTC data
  across faces, and invalidated renderer caches when a same-path font is
  replaced so active subtitle tracks immediately use the new data.
- Kept registry and selected-font generations separate, replayed libass codec
  private data after renderer recreation, and added OpenHarmony and tvOS C API
  coverage for the complete memory-font surface.

### Playback recovery

- Added explicit buffering transitions when audio and video input starves. The
  playback clock now freezes during a stall and reanchors to recovered media
  time instead of running ahead and causing a prolonged frame drop or black
  screen after data resumes.

## 0.1.4 - 2026-07-30

### Compatibility notes

- **`outline_width` is now a profile, not a multiplier.** `0` is off, `1` fine,
  `2` normal, `3` thick; values above `3` clamp to thick. Normal and thick
  reproduce exactly the widths the old continuous multiplier produced at `1.0`
  and `2.0`, so a host that used to send `outline_width: 1.0` must now send
  `2.0` to keep the same stroke. Left unmigrated, outlines render thinner —
  at the default font size the rasterized radius halves from 2 px to 1 px.
- **Danmaku font size is now interpreted as pixels per em.** `ab_glyph`'s
  `PxScale` is an ascent-to-descent height, and the previous code passed the
  em size straight into it, so text rendered smaller than requested by the
  font's own height/em ratio. Text now matches the requested size, which means
  existing users see larger danmaku on upgrade: the ratio is 1.0 for STHeiti
  and Hiragino Sans GB, but 1.4 for PingFang SC (the default macOS face), so
  the same configuration can render up to 40% larger and fit fewer tracks.
  Hosts that want the previous look should lower their configured font size.
- Scroll duration now scales with the viewport's logical width (×0.9 at 640 pt
  up to ×1.3 at 1920 pt and wider) so a danmaku crosses wide windows in a
  comparable amount of visual time. The same configuration therefore scrolls
  more slowly on a large window than it did before.
- Danmaku screenshots no longer include danmaku; `capture_*` composites video
  and subtitles only. The debug HUD is also excluded from captures.

### Platform support and release artifacts

- Added an OpenHarmony player backend and Flutter plugin with AVCodec H.264/HEVC
  hardware decoding, OHNativeBuffer/Vulkan zero-copy presentation, WGPU
  composition, subtitles, danmaku, audio, diagnostics, and RGBA screenshots.
  The release includes `erika-capi-openharmony-arm64.zip` with the C API runtime
  and Flutter native bridge.
- Added native Windows ARM64 dependency and `erika_capi` builds, plus the new
  `erika-capi-windows-arm64.zip` release archive. Windows x64 and ARM64 CI run
  on matching GitHub-hosted architectures.
- Added selectable macOS arm64, x86_64, and universal builds and corresponding
  architecture-specific release archives.
- Fixed Android source builds on Apple Silicon by selecting the universal
  `glslc` shipped in the NDK's `darwin-x86_64` tools directory.
- Re-enabled FFmpeg's optimized x86 assembly for shipped x86_64 builds. Source
  builds for these targets now require NASM.

### Playback, rendering, and window integration

- Published the shared playback clock directly and made frame snapshots sample
  time, state, and generation atomically. Danmaku, subtitles, and rendering now
  use the same time base without a separate forward-only display clock.
- Paused seeks now decode and present the requested preview frame while keeping
  the media clock frozen. Seek preroll, EOF, immediate pause-after-seek, and
  resume startup races no longer produce clock rollback, worker spin, or a
  prolonged black frame.
- Separated audio and video demux demand so audio backpressure cannot stall video
  packet scanning, while retaining a bounded decoded-audio queue. Switching or
  adding subtitles no longer resets audio/video demux selection.
- Reworked macOS presentation around `CVDisplayLink`, coalesced slow frames, and
  retargeted the display link after screen changes. Reduced redundant overlay
  attachment and GPU resource work during resize and window migration.
- Added multi-`FlutterView` and secondary-window targeting to the desktop overlay
  API. macOS and Windows overlays now follow a player between host windows while
  preserving surface, visibility, generation, and aspect-fit state.
- Improved Windows D3D11 overlay lifetime, non-blocking presentation, and resume
  seek handling. Subtitles are composited in the video viewport while danmaku
  remains in the full-window viewport.

### Danmaku and subtitles

- Aligned danmaku collision bounds with rasterized outlines, corrected
  shadow/outline/text layer ordering, stabilized track preference, and limited
  high-density overlap to overflow tracks.
- Kept the previous danmaku plan moving while asynchronous relayout completes;
  paint-only settings reuse layout, visibility changes apply immediately, and
  stale plans cannot reappear after danmaku is disabled.
- Added incremental glyph-atlas uploads and reusable Metal instance buffers, and
  reduced per-frame DFM allocation and candidate traversal overhead.
- Added charset detection and transcoding for external text subtitles, including
  GBK, Big5, Shift_JIS, and UTF-16, with UTF-8 passthrough and guarded fallback
  for low-confidence or binary input.
- libass now registers Erika's bundled Droid Sans Fallback on every platform,
  not just iOS/Android, and targets without a system font provider default to
  that family instead of an unresolvable `Arial`.
- Added subtitle fallback and selective override styling: custom font family and
  file, RGBA colours, metrics, text attributes, border, alignment, margins, and
  blur. New entry points are `erika_presenter_set_subtitle_font`,
  `erika_presenter_set_subtitle_style`, and `ErikaPlayer.setSubtitleStyle`.

### Networking and media diagnostics

- Added `erika_open_with_headers` and `erika_presenter_open_with_headers`.
  `ErikaPlayer.open` accepts `httpHeaders` on Android, iOS, macOS, and Windows,
  and applies them to the probe, ranged reads, retries, and prefetch requests.
  The original open functions remain compatible and use an empty header list.
- Rejects caller-supplied transport headers managed by Erika and invalid HTTP
  field names or values. External subtitle and danmaku sidecars remain on the
  headerless path and do not inherit media request headers.
- Hardened HTTP range input against ignored `Range` requests, incorrect response
  offsets, transient request/body failures, partial responses, duplicate
  prefetch downloads, and false EOF. Retries are bounded across the complete
  fetch operation.
- Added track bitrate and rational frame-rate metadata across Rust, C, and Dart,
  plus an opt-in native debug HUD for decoder, renderer, GPU, audio, output/HDR,
  and danmaku diagnostics.

### Audio, colour, and codec updates

- Normalized surround-to-stereo downmix matrices to prevent clipping.
- Added smooth per-callback volume ramps on all audio backends and WASAPI device
  loss recovery with observable recovery state and bounded backoff.
- Added BT.2100 HLG decoding on Metal, WGPU, and D3D11, and correct PQ encoding
  for D3D11 overlays on HDR10 output.
- Upgraded FFmpeg to 8.1.2, improved Darwin AV1 hardware decode/import handling,
  and preserved the last frame during seek loading instead of flashing black.

## 0.1.3 - 2026-07-17

### Android playback and packaging

- Added the complete MediaCodec, AHardwareBuffer/wgpu, AAudio, Flutter
  PlatformView, SAF/content-source, subtitle, danmaku, screenshot, SDR/HDR,
  diagnostics, and recovery paths.
- `ERIKA_PREBUILT=1` now stages `liberika_capi.so` and `libc++_shared.so` from
  the tagged Android release archive for the requested Flutter ABIs, with an
  explicit source-build fallback.

### Breaking C API surface-size semantics

The `width` and `height` arguments passed to
`erika_presenter_attach_metal_layer`, `erika_presenter_attach_wgpu_surface`,
`erika_presenter_attach_wgpu_surface_with_output_capabilities`,
`erika_presenter_attach_windows_hwnd`, and `erika_presenter_resize_surface`
now mean the exact drawable extent in physical pixels.

Previously, native renderers multiplied those values by `scale`. The `scale`
argument is now independent and affects logical UI content such as danmaku; it
never changes the surface extent. Direct C API hosts that currently pass logical
dimensions must convert them to physical pixels before calling these functions.
The in-tree macOS, iOS, Windows, and Android Flutter embeddings and examples
have already been updated.

### Playback command dispatch

`play` is queued asynchronously and no longer waits indefinitely for the
playback worker. Hosts must observe `StateChanged` and `Error` events for the
authoritative result of the transition.
