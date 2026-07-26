# Changelog

## Unreleased

### Subtitle font fallback and style

- libass now registers Erika's bundled Droid Sans Fallback on every platform,
  not just iOS/Android, and targets without a system font provider (the
  vendored libass build disables fontconfig) default to that family instead of
  an unresolvable `Arial`.
- Added a subtitle style: a custom font family, font file, font size, outline
  width, and text/outline colours (`0xRRGGBBAA`). They act as fallbacks — an ASS
  script keeps its own styling — and `force_override` promotes them to libass'
  selective style override so they replace what dialogue events request. The
  existing subtitle scale still multiplies the size and outline width.
- New C API: `erika_presenter_set_subtitle_font`,
  `erika_presenter_set_subtitle_style`. New Dart API:
  `ErikaPlayer.setSubtitleStyle`.

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
