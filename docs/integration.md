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

The presenter family is compiled on **macOS, iOS, and Windows** only.

## 2. The lifecycle

```
create ──▶ attach surface ──▶ open ──▶ play ──▶ (render_tick + poll_event loop)
                                                      │
                              pause / seek / set_* ◀──┤
                                                      ▼
                        detach surface ──▶ destroy
```

`open` is asynchronous. The handle moves through `Opening → Ready → Playing`;
observe transitions via events rather than blocking. You can attach the surface
before or after `open`, but attaching first lets the idle test pattern / first
frame appear immediately.

## 3. Create the presenter

```c
ErikaPresenterConfig cfg = {
    .output_mode  = ErikaPresenterOutputMode_Sdr,   // or _AppleEdr on macOS/iOS
    .edr_headroom = 1.0f,                            // used only for Apple EDR
    .luma_upscaler = ErikaLumaUpscalerMode_Off,      // or ArtCnnC4F16 / C4F32
};
ErikaPresenterHandle *p = erika_presenter_create_with_config(cfg);
if (!p) { /* read erika_last_error_message() */ }
```

`erika_presenter_create()` uses defaults (SDR, no upscaler). The neural luma
upscaler is a Metal-compute feature; on the D3D11/wgpu backends `set_upscaler`
is an accepted no-op fallback.

## 4. Attach a surface

Erika renders directly into a platform surface you own. Width/height are
**physical pixels**; `scale` is the DPI/backing factor.

### macOS / iOS — `CAMetalLayer`

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

## 5. Open and play

```c
if (erika_presenter_open(p, "/path/to/video.mkv") != ErikaStatus_Ok) { /* log */ }
erika_presenter_play(p);
```

`uri` is a local path or HTTP(S) URL.

## 6. The render loop

Drive `render_tick` from the surface's display timer — `CADisplayLink`
(iOS) / `CVDisplayLink` or `CADisplayLink` (macOS) / a frame scheduler on
Windows. Pass the frame's **presentation time in seconds** from a monotonic
host clock; Erika uses it for vsync-quantized scheduling, so pass an absolute
timestamp, not a delta.

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
- **Danmaku:** load a track (`load_danmaku_file` / `_json` or the multi-track
  `add_danmaku_track_*`), toggle (`set_danmaku_enabled`), tune via
  `set_danmaku_config`, offset tracks, set the font, set blocked words. See
  [danmaku_architecture.md](danmaku_architecture.md).
- **Upscaler:** `set_upscaler(mode)`; inspect with `get_upscaler_status`.

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
`CADisplayLink`/`CVDisplayLink` callback. This is what the macOS/iOS Flutter
Swift plugins do over the same C ABI.

### Dart FFI

Bind the symbols with `dart:ffi` (`DynamicLibrary.open` for the dylib/dll, or
process symbols for a static link). Keep all FFI calls on one isolate; marshal
strings with `toNativeUtf8`/`free`. The high-level `erika_flutter` package
already does this — prefer it unless you are building a custom embedder.

## Checklist

- [ ] Create the presenter (with the right output mode for your display).
- [ ] Attach the surface with **physical-pixel** size and the correct scale.
- [ ] Open, then play; don't block — watch events for readiness.
- [ ] `render_tick(absolute_time_seconds)` every display frame; drain events.
- [ ] `resize_surface` on every size/scale change.
- [ ] One thread per handle, or serialize calls.
- [ ] Free every returned string / `ErikaTrackInfo`; `detach` then `destroy`.
