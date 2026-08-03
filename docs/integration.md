# Integrating Erika in a Native Host

> Translations: [中文](integration.zh.md) · [日本語](integration.ja.md)

This guide walks through embedding Erika in a non-Flutter host — a C/C++/Swift
app, a Win32 window, or any runtime with C FFI. It uses the **presenter** (push)
model, where Erika owns decode, timing, audio, overlays, and presentation, and
the host supplies a surface plus a per-frame `render_tick`.

Prerequisites: the C ABI ([capi_reference.md](capi_reference.md)) and a built
Erika library ([building.md](building.md)). For Flutter, use the
[`erika_flutter`](../packages/erika_flutter) plugin instead, described in
[flutter_embedding.md](flutter_embedding.md).

Two runnable references accompany this guide:
[`examples/macos_native_demo`](../examples/macos_native_demo) (AppKit +
`CAMetalLayer`) and [`examples/windows_native_demo`](../examples/windows_native_demo)
(Win32 + `HWND`). They drive the Rust `PresenterRuntime` directly; the C-ABI
calls below are the 1:1 equivalents.

## 1. Choose a handle family

Use `ErikaPresenterHandle` unless you have a reason to render yourself. The
pull-model `ErikaHandle` is for hosts that own their compositor and only want
Erika's decode/timing/state. The rest of this guide is presenter-based.

The presenter family is compiled on **macOS, iOS, tvOS, Windows, and Android**.

## 2. The lifecycle

```
create ──▶ attach surface ──▶ open ──▶ play ──▶ (render_tick + poll_event loop)
                                                      │
                              pause / seek / set_* ◀──┤
                                                      ▼
                        detach surface ──▶ destroy
```

`open` and `play` are asynchronous. The handle moves through
`Opening → Ready → Playing`; observe transitions and failures via events rather
than blocking. You can attach the surface
before or after `open`, but attaching first lets the idle test pattern / first
frame appear immediately.

## 3. Create the presenter

```c
ErikaPresenterConfig cfg = {
    .output_mode  = ErikaPresenterOutputMode_Sdr,   // AppleEdr or Android ExtendedLinear
    .edr_headroom = 1.0f,                            // requested content-headroom ceiling
    .luma_upscaler = ErikaLumaUpscalerMode_Off,      // or ArtCnnC4F16 / C4F32
};
ErikaPresenterHandle *p = erika_presenter_create_with_config(cfg);
if (!p) { /* read erika_last_error_message() */ }
```

`erika_presenter_create()` uses defaults (SDR, no upscaler). The neural luma
upscaler is implemented by Metal on Apple platforms and by wgpu compute on
capable Vulkan-class adapters, including Android. Backends without compute
(notably GLES 3.0 and D3D11) retain native luma sampling and report an explicit
`Inactive` status and fallback reason.

## 4. Attach a surface

Erika renders directly into a platform surface you own. Width/height are
**physical pixels**; `scale` is the DPI/backing factor.

### macOS / iOS / tvOS — `CAMetalLayer`

Create a `CAMetalLayer`, size it, then hand its pointer to Erika:

```c
// `layer` is a CAMetalLayer* (e.g. from your NSView/UIView host layer)
erika_presenter_attach_metal_layer(p, (uint64_t)(uintptr_t)layer,
                                   pixel_w, pixel_h, backing_scale);
```

On macOS the recommended arrangement is a window-hosted layer that is a sibling
/ underlay of your content view, so video stays outside the AppKit view
compositor (the same model the Flutter plugin uses — see
[flutter_embedding.md](flutter_embedding.md)).

### Windows — `HWND`

```c
HWND hwnd = /* your window */;
HINSTANCE hinst = GetModuleHandleW(NULL);
UINT dpi = GetDpiForWindow(hwnd);
double scale = dpi ? (double)dpi / 96.0 : 1.0;
RECT rc; GetClientRect(hwnd, &rc);
uint32_t w = max(1, rc.right - rc.left), h = max(1, rc.bottom - rc.top);

erika_presenter_attach_windows_hwnd(p, (uint64_t)(uintptr_t)hwnd,
                                    (uint64_t)(uintptr_t)hinst, w, h, scale);
```

`attach_windows_hwnd` is a convenience wrapper over `attach_wgpu_surface` with
kind `WindowsHwnd`. With the default presenter config the surface drives the
**native Direct3D 11** renderer (D3D11VA zero-copy, HDR10); pass the wgpu
fallback renderer in config only if you specifically need it.

### Generic — `attach_wgpu_surface`

For X11/Wayland/Android or to be explicit about the surface kind, use
`erika_presenter_attach_wgpu_surface(p, kind, raw_window, raw_display, w, h, scale)`
with the matching `ErikaWgpuSurfaceKind` and platform handles.

### Android extended-linear scRGB

Android `ExtendedLinear` is FP16 extended-linear scRGB, not HDR10/PQ. A native
host must provide an `ANativeWindow` from a `SurfaceView` that is directly
composited (the Flutter plugin uses Hybrid Composition) and attach it with the
probed output capabilities:

```c
ErikaSurfaceOutputCapabilities caps = {
    .extended_linear = display_and_surface_are_hdr_capable,
    .direct_composition = true,       // SurfaceView, not TextureView
    .desired_headroom = requested_headroom, // 0 = system auto
    .fallback_reason = host_probe_reason, // 0 when eligible
};
erika_presenter_attach_wgpu_surface_with_output_capabilities(
    p, ErikaWgpuSurfaceKind_AndroidNativeWindow,
    (uint64_t)(uintptr_t)native_window, 0, w, h, scale, caps);
```

The renderer activates extended-linear only when the request, display/surface
eligibility, direct composition, Vulkan backend, `Rgba16Float` support, and
post-configure `ADATASPACE_SCRGB_LINEAR` readback all succeed. Any failed
condition keeps playback on SDR and records a stable `fallback_reason` code
`0..8`; GLES and `TextureView` are always SDR paths.

`ErikaPresenterConfig.edr_headroom` is the content ceiling. A positive
`desired_headroom` is an optional surface ceiling; `0` leaves the surface in
system-auto mode. The effective wgpu target follows those ceilings and the
current display HDR/SDR ratio when that ratio is known. On API 34+, a native
host should observe `Display.registerHdrSdrRatioChangedListener` and publish
each real state change with
`erika_presenter_set_output_headroom(p, ratio, true)`. Publish
`(1.0f, false)` when the ratio becomes unavailable or the view detaches. The
Flutter plugin performs this wiring automatically. On API 35 it also calls
`SurfaceView.setDesiredHdrHeadroom` per view; it never changes the global
window.

After attach and after every resize/recovery, query
`erika_presenter_get_output_status`. Active Android scRGB reports
`AndroidExtendedLinearScRgb`, `SixteenBitFloat`, native dataspace `406913024`,
and `extended_linear_active = true`. When Android supplies a display ratio,
`active_headroom` contains that ratio and `active_headroom_known` is true;
otherwise the field is only a fallback value and `active_headroom_known` is
false. `headroom_updates` increments only for a real known-state or ratio
change. A requested mode is not evidence that the mode is active. Current
non-HDR/emulator testing covers the SDR fallback path; API 35 HDR-device
acceptance remains required for the active path.

## 5. Open and play

```c
if (erika_presenter_open(p, "/path/to/video.mkv") != ErikaStatus_Ok) { /* log */ }
erika_presenter_play(p);
```

`uri` is a local path or HTTP(S) URL.

## 6. The render loop

Drive `render_tick` from the surface's display timer — `CADisplayLink`
(iOS/tvOS) / `CVDisplayLink` or `CADisplayLink` (macOS) / a frame scheduler on
Windows / `Choreographer` on Android. Pass the frame's **presentation time in
seconds** from a monotonic host clock; Erika uses it for vsync-quantized
scheduling, so pass an absolute timestamp, not a delta.

```c
// Once per display frame:
ErikaPresenterStats stats;
erika_presenter_render_tick(p, host_time_seconds, &stats);   // out_stats may be NULL

// Drain events the same iteration:
ErikaEvent ev;
while (erika_presenter_poll_event(p, &ev) == ErikaStatus_Ok) {
    handle_event(&ev);
}
```

On any drawable-size or scale change (window resize, monitor DPI change, device
rotation), call `erika_presenter_resize_surface(p, w, h, scale)` **before** the
next tick. The Windows demo polls `GetClientRect` + `GetDpiForWindow` each frame
and resizes when they change.

`render_tick` returns quickly; it does not block on vsync itself — your display
timer provides the cadence. If you are not on a display callback (e.g. a smoke
test), a `~16 ms` sleep per iteration approximates 60 Hz.

## 7. Handle events

`poll_event` is non-blocking and returns `NoEvent` when the queue is empty.
Dispatch on `ev.kind`:

| Kind | Meaning | Read |
|------|---------|------|
| `StateChanged` | Playback state moved | `ev.state` |
| `DurationChanged` | Duration known/updated | `ev.duration_micros` |
| `PositionChanged` | Periodic position tick | `ev.position_micros` |
| `TracksChanged` | Track list changed | re-query `erika_presenter_tracks` |
| `TrackSelectionChanged` | Selection changed | `erika_presenter_track_selection` |
| `BufferingChanged` | Buffering toggled | `ev.buffering` |
| `VideoParamsChanged` | Resolution / color metadata | `ev.video` |
| `Error` | A failure occurred | `ev.status` + `erika_last_error_message` |

## 8. Runtime control

All of these are safe to call live, between ticks:

- **Transport:** `play` / `pause` / `stop` / `seek(position_micros)` /
  `set_playback_rate(rate)`.
- **Audio:** `set_volume(0.0–1.0)`.
- **Tracks:** `erika_presenter_tracks` (counted-array idiom),
  `select_audio_track` / `select_subtitle_track` (id `-1` disables subtitles),
  `add_external_subtitle`, `remove_subtitle_track`, `set_subtitle_scale`.
- **Subtitle style:** `set_subtitle_font(family, file_path)` and
  `set_subtitle_style(ErikaSubtitleStyle)` (font, colours, metrics, attributes,
  border, alignment and margins). Both are fallbacks — an ASS script keeps its
  own styling unless the matching `override_mask` bit is set — and
  `set_subtitle_scale` still multiplies the metrics. See
  [capi_reference.md](capi_reference.md#playback-and-runtime-parameters).
- **Danmaku:** load a track (`load_danmaku_file` / `_json` or the multi-track
  `add_danmaku_track_*`), toggle (`set_danmaku_enabled`), tune via
  `set_danmaku_config`, offset tracks, set the font, set blocked words. See
  [danmaku_architecture.md](danmaku_architecture.md).
- **Upscaler:** `set_upscaler(mode)`; inspect with `get_upscaler_status`.
- **Output:** inspect the actual mode and fallback counters with
  `erika_presenter_get_output_status`; Android native hosts publish dynamic
  display ratios with `erika_presenter_set_output_headroom`. `capture_frame_rgba`
  always produces an SDR RGBA8 screenshot of video + subtitle + danmaku, even
  when the display output is extended-linear.

## 9. Teardown

```c
erika_presenter_detach_surface(p);   // stop drawing into the surface first
erika_presenter_destroy(p);          // stops playback, releases everything
```

Detach before you tear down the window/layer so Erika stops touching the
surface. `destroy` is safe on a `NULL` handle.

## 10. Threading model

A handle is **not internally synchronized**. The simplest correct design: own
the presenter on one thread — the one running the display timer — and make all
calls (`render_tick`, transport, track changes) from there. If you must call
from another thread (e.g. a UI thread issuing `seek`), serialize with your own
lock so two calls never overlap on the same handle. Error messages are
thread-local, so read `erika_last_error_message` on the thread that made the
failing call.

## Per-language notes

### C / C++

Include `erika.h`, link the library (see [building.md](building.md)), and you
are done — the ABI is plain C. In C++ wrap the handle in an RAII type that calls
`erika_presenter_destroy` in its destructor, and free returned strings /
`ErikaTrackInfo` records with the matching Erika free functions, never `delete`.

### Swift

Import the C ABI through a bridging header or a module map over `erika.h`. Cast
the `CAMetalLayer` with `unsafeBitCast(layer, to: UInt64.self)` or
`UInt64(UInt(bitPattern: ...))`. Drive `erika_presenter_render_tick` from a
`CADisplayLink`/`CVDisplayLink` callback. This is what the macOS/iOS/tvOS Flutter
Swift plugins do over the same C ABI.

### Dart FFI

Bind the symbols with `dart:ffi` (`DynamicLibrary.open` for the dylib/dll, or
process symbols for a static link). Keep all FFI calls on one isolate; marshal
strings with `toNativeUtf8`/`free`. The high-level `erika_flutter` package
already does this — prefer it unless you are building a custom embedder.

## Checklist

- [ ] Create the presenter (with the right output mode for your display).
- [ ] Attach the surface with **physical-pixel** size and the correct scale.
- [ ] For Android extended-linear, use a Hybrid-Composition `SurfaceView`, pass
  probed output capabilities, and verify `Rgba16Float + SCRGB_LINEAR` through
  `erika_presenter_get_output_status`; otherwise accept and log the SDR reason.
- [ ] On API 34+, publish display HDR/SDR ratio changes through
  `erika_presenter_set_output_headroom`; on API 35 keep desired headroom scoped
  to the individual `SurfaceView`.
- [ ] Open, then play; don't block — watch events for readiness.
- [ ] `render_tick(absolute_time_seconds)` every display frame; drain events.
- [ ] `resize_surface` on every size/scale change.
- [ ] One thread per handle, or serialize calls.
- [ ] Free every returned string / `ErikaTrackInfo`; `detach` then `destroy`.
- [ ] Do not claim Android extended-linear device validation until the API 35
  HDR-device rotation/recovery, multi-player, and SDR-screenshot checks pass.
