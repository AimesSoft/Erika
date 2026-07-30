# Changelog

## Unreleased

### Danmaku appearance (breaking for existing hosts)

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
  and subtitles only.

### Subtitle font fallback and style

- libass now registers Erika's bundled Droid Sans Fallback on every platform,
  not just iOS/Android, and targets without a system font provider (the
  vendored libass build disables fontconfig) default to that family instead of
  an unresolvable `Arial`.
- Added a subtitle style: a custom font family and font file, text/outline
  colours (`0xRRGGBBAA`), metrics (size, outline, shadow, blur, spacing, scale),
  bold/italic/underline/strike-out, border style, alignment and margins. They act
  as fallbacks — an ASS script keeps its own styling — and an `override_mask`
  promotes chosen fields to libass' selective style override so they replace what
  dialogue events request. The override metrics are renormalized against the
  frame height, so they land on the same pixels whatever `PlayResY` a script
  declares; override margins stay in script units, which libass does not
  normalize. The existing subtitle scale still multiplies the metrics.
- New C API: `erika_presenter_set_subtitle_font`,
  `erika_presenter_set_subtitle_style`. New Dart API:
  `ErikaPlayer.setSubtitleStyle`.

### Native architectures and release artifacts

- Added native Windows ARM64 dependency and `erika_capi` builds, plus the new
  `erika-capi-windows-arm64.zip` release archive. Windows x64 and ARM64 CI run
  on matching GitHub-hosted architectures.
- Added selectable macOS arm64, x86_64, and universal builds and corresponding
  architecture-specific release archives.
- Re-enabled FFmpeg's optimized x86 assembly for shipped x86_64 builds. These
  builds now require NASM and replace the previous C/compiler-vectorized FFmpeg
  paths used while `--disable-x86asm` was enabled.

### Custom HTTP headers

- Added `erika_open_with_headers` and `erika_presenter_open_with_headers`,
  which carry caller-supplied headers (`Authorization`, session cookies, …)
  on the `HEAD` probe, every ranged `GET`, and the prefetch thread. The C ABI
  now exports 75 functions.
- `erika_open` and `erika_presenter_open` are unchanged and delegate to the new
  entry points with an empty header list.
- `ErikaPlayer.open` accepts `httpHeaders` on Android, iOS, macOS, and Windows.
  When the loaded native library predates these exports, an open that carries
  headers now fails with an explicit error instead of silently dropping them.
- Headers Erika derives itself (`Range`, `Host`, `Content-Length`,
  `Transfer-Encoding`, `Connection`) are rejected at the ABI boundary, as are
  header names and values that are not valid HTTP field tokens.
- External subtitle and danmaku sidecar loads still use the headerless path;
  they do not yet inherit the request's headers.

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
