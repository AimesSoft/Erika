# Changelog

## Unreleased

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
  OpenHarmony is built from source; the release workflow does not yet publish an
  OpenHarmony prebuilt archive.
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
