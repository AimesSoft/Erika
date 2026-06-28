# Erika C ABI Reference

> Translations: [中文](capi_reference.zh.md) · [日本語](capi_reference.ja.md)

This document describes the stable C ABI exported by `erika_capi`, declared in
[`crates/erika_capi/include/erika.h`](../crates/erika_capi/include/erika.h). The
ABI is the single integration surface for every non-Rust host (C, C++, Swift,
Dart FFI, Win32, …). Rust embedders should use the `erika` crate directly; this
layer exists for FFI.

For the embedding walkthrough (surface attach, the render loop, teardown) see
[integration.md](integration.md). For the high-level engine design see
[architecture.md](architecture.md).

## Two handle families

Erika exposes two independent entry points. Pick one per integration.

| Handle | Model | Who renders | Use when |
|--------|-------|-------------|----------|
| `ErikaHandle` | **Pull** | The host | You own the render loop and pull decoded frames / drive your own compositor. |
| `ErikaPresenterHandle` | **Push** | Erika | You give Erika a native surface and call `render_tick` once per display frame. Erika owns decode, timing, audio, overlays, and presentation. |

`ErikaPresenterHandle` is the recommended path and what the Flutter plugin and
the native demos use. It is compiled only on **macOS, iOS, and Windows**. On
other targets `erika_presenter_create` is still exported but returns `NULL`, and
the rest of the presenter family is absent — guard presenter usage by platform.

The two families do not share state; a process may use both, but a given media
session lives in exactly one handle.

## Conventions

### Status codes

Every fallible call returns `ErikaStatus`:

| Value | Code | Meaning |
|-------|------|---------|
| `ErikaStatus_Ok` | 0 | Success. The thread-local error is cleared. |
| `ErikaStatus_NullPointer` | 1 | A required handle or out-pointer was `NULL` (or a surface pointer was 0). |
| `ErikaStatus_InvalidUtf8` | 2 | A `const char*` argument was not valid UTF-8. |
| `ErikaStatus_PlayerError` | 3 | The engine rejected the call; read the message (see below). |
| `ErikaStatus_Panic` | 4 | A Rust panic was caught at the boundary. The call had no effect beyond what completed before the panic; the handle should be considered suspect. |
| `ErikaStatus_NoEvent` | 5 | `*_poll_event` only: the queue is empty (not an error). |

Always check the return value. `Ok` and `NoEvent` are the only non-error
results.

### Panic safety

The ABI never unwinds across the FFI boundary. Every entry point wraps its body
in `catch_unwind`; a panic becomes `ErikaStatus_Panic` with an error message
set. You can call across the boundary without C++ `noexcept`/SEH concerns.

### Error messages (thread-local)

On any non-`Ok`/`NoEvent` result, Erika stores a human-readable message in a
**thread-local** slot. Retrieve it with:

```c
char *msg = erika_last_error_message();   // heap-allocated, may be NULL
if (msg) { fprintf(stderr, "erika: %s\n", msg); erika_string_free(msg); }
```

Because the slot is thread-local, read it **on the same thread** that made the
failing call, before making another call on that thread (a subsequent `Ok`
clears it). `erika_last_error_message` returns a copy you own — free it with
`erika_string_free`.

### String ownership

Any `char*` Erika hands back is heap-allocated and owned by the caller:

- Standalone strings (e.g. `erika_last_error_message`) → free with
  `erika_string_free`.
- Strings embedded in `ErikaTrackInfo` → free the whole record with
  `erika_track_info_free(&track)` (frees every inner string).
- Strings embedded in `ErikaDanmakuTrackInfo` → free with
  `erika_danmaku_track_info_free(&track)`.

Never `free()` these with libc; always use the matching Erika free function so
allocation crosses the ABI on the same allocator.

`const char*` arguments you pass in are borrowed for the duration of the call
only; Erika copies what it needs. They must be NUL-terminated UTF-8.

### Counted-array idiom

List getters (`erika_tracks`, `erika_presenter_tracks`,
`erika_presenter_danmaku_tracks`) use a caller-allocated buffer:

```c
size_t total = 0;
erika_presenter_tracks(p, NULL, 0, &total);          // 1) query count
ErikaTrackInfo *buf = calloc(total, sizeof *buf);
erika_presenter_tracks(p, buf, total, &total);        // 2) fill
for (size_t i = 0; i < total; i++) { /* use buf[i] */ }
for (size_t i = 0; i < total; i++) erika_track_info_free(&buf[i]);
free(buf);
```

`out_len` is **always** set to the total number of available records. At most
`capacity` records are written; passing `capacity == 0` (with a `NULL` buffer)
is the supported way to size. Only the records actually written own strings that
must be freed.

### Surface geometry and scale

`attach_*` and `resize_surface` take `width`, `height` in **physical pixels**
and a `scale` (backing/DPI factor, e.g. `2.0` on Retina, the monitor scale on
Windows). A surface pointer of `0` is rejected with `NullPointer`.

### Threading

A single handle is **not internally synchronized**. Do not call into the same
handle concurrently from multiple threads; serialize calls yourself (or confine
a handle to one thread). The presenter's `render_tick` should be driven from the
thread that owns the display timer / surface. Distinct handles on distinct
threads are independent. Remember error messages are thread-local.

## `ErikaHandle` — pull model

The host drives its own rendering and pulls state/events.

### Lifecycle

```c
ErikaHandle *erika_create(void);
void         erika_destroy(ErikaHandle *handle);
char        *erika_last_error_message(void);   // thread-local, caller frees
void         erika_string_free(char *value);
```

`erika_create` never fails (returns a valid handle). `erika_destroy(NULL)` is a
no-op. Destroying a handle stops playback and releases all resources.

### Playback control

```c
ErikaStatus erika_open(ErikaHandle *handle, const char *uri);   // file path or URL
ErikaStatus erika_play(ErikaHandle *handle);
ErikaStatus erika_pause(ErikaHandle *handle);
ErikaStatus erika_stop(ErikaHandle *handle);
ErikaStatus erika_close(ErikaHandle *handle);
ErikaStatus erika_seek(ErikaHandle *handle, uint64_t position_micros);
```

`uri` is a local filesystem path or an HTTP(S) URL. `seek` takes microseconds.
`open` begins asynchronously; watch for `StateChanged` /`DurationChanged` events
to learn when the media is ready.

### Tracks and subtitles

```c
ErikaStatus erika_add_external_subtitle(ErikaHandle *, const char *uri, int64_t *out_track_id);
ErikaStatus erika_remove_subtitle_track(ErikaHandle *, int64_t track_id);
ErikaStatus erika_select_audio_track(ErikaHandle *, int64_t track_id);
ErikaStatus erika_select_subtitle_track(ErikaHandle *, int64_t track_id);
ErikaStatus erika_track_selection(ErikaHandle *, ErikaTrackSelection *out_selection);
ErikaStatus erika_tracks(ErikaHandle *, ErikaTrackInfo *out_tracks, uintptr_t capacity, uintptr_t *out_len);
void        erika_track_info_free(ErikaTrackInfo *track);
```

`erika_tracks` follows the counted-array idiom. `erika_track_selection` reports
the currently selected video/audio/subtitle track ids (`-1` for none). Selecting
the subtitle track id `-1` disables subtitles.

### State and events

```c
ErikaStatus erika_state(ErikaHandle *, ErikaState *out_state);
ErikaStatus erika_poll_event(ErikaHandle *, ErikaEvent *out_event);
```

`erika_poll_event` is non-blocking: it returns `NoEvent` when the queue is
empty. Drain it in your loop. See [Events](#events).

### Surface attach (host-managed)

```c
ErikaStatus erika_attach_metal_layer(ErikaHandle *, uint64_t raw_layer, uint32_t w, uint32_t h, double scale);
ErikaStatus erika_attach_wgpu_surface(ErikaHandle *, ErikaWgpuSurfaceKind kind,
                                      uint64_t raw_window, uint64_t raw_display,
                                      uint32_t w, uint32_t h, double scale);
ErikaStatus erika_attach_flutter_texture(ErikaHandle *, ErikaFlutterTextureKind kind,
                                         int64_t texture_id, uint32_t w, uint32_t h, double scale);
ErikaStatus erika_detach_surface(ErikaHandle *);
```

`raw_layer` is a `CAMetalLayer*` cast to `uint64_t`. For `erika_attach_wgpu_surface`,
`raw_window`/`raw_display` are the platform window/display handles for the given
`kind` (e.g. `HWND` + `HINSTANCE` for `WindowsHwnd`, `xcb`/Xlib window + display
for `XlibWindow`). `erika_attach_flutter_texture` registers an external texture
id with a platform texture registrar.

## `ErikaPresenterHandle` — push model

Erika owns the full stack; the host supplies a surface and calls `render_tick`.
**macOS / iOS / Windows only.**

### Lifecycle and configuration

```c
ErikaPresenterHandle *erika_presenter_create(void);
ErikaPresenterHandle *erika_presenter_create_with_config(ErikaPresenterConfig config);
ErikaPresenterHandle *erika_presenter_create_with_output_mode(int32_t output_mode, float edr_headroom);
void                  erika_presenter_destroy(ErikaPresenterHandle *handle);
```

`ErikaPresenterConfig` selects the output mode (SDR / Apple EDR), the EDR
headroom, and the initial luma upscaler. `create_with_output_mode` is a
shorthand; `create` uses defaults (SDR, no upscaler). A `NULL` return means
creation failed — check `erika_last_error_message`.

### Playback and runtime parameters

```c
ErikaStatus erika_presenter_open(ErikaPresenterHandle *, const char *uri);
ErikaStatus erika_presenter_play(ErikaPresenterHandle *);
ErikaStatus erika_presenter_pause(ErikaPresenterHandle *);
ErikaStatus erika_presenter_stop(ErikaPresenterHandle *);
ErikaStatus erika_presenter_close(ErikaPresenterHandle *);
ErikaStatus erika_presenter_seek(ErikaPresenterHandle *, uint64_t position_micros);
ErikaStatus erika_presenter_set_playback_rate(ErikaPresenterHandle *, double rate);
ErikaStatus erika_presenter_set_volume(ErikaPresenterHandle *, double volume);   // 0.0–1.0
ErikaStatus erika_presenter_set_upscaler(ErikaPresenterHandle *, int32_t mode);  // ErikaLumaUpscalerMode
ErikaStatus erika_presenter_set_subtitle_scale(ErikaPresenterHandle *, double scale);
```

`set_playback_rate(1.0)` is normal speed. `set_upscaler` switches the neural
luma upscaler at runtime (see [`erika_presenter_get_upscaler_status`](#diagnostics-and-capture));
it is a no-op fallback on backends without a Metal compute path.

### Tracks and subtitles

```c
ErikaStatus erika_presenter_add_external_subtitle(ErikaPresenterHandle *, const char *uri, int64_t *out_track_id);
ErikaStatus erika_presenter_remove_subtitle_track(ErikaPresenterHandle *, int64_t track_id);
ErikaStatus erika_presenter_select_audio_track(ErikaPresenterHandle *, int64_t track_id);
ErikaStatus erika_presenter_select_subtitle_track(ErikaPresenterHandle *, int64_t track_id);
ErikaStatus erika_presenter_track_selection(ErikaPresenterHandle *, ErikaTrackSelection *out_selection);
ErikaStatus erika_presenter_tracks(ErikaPresenterHandle *, ErikaTrackInfo *out_tracks, uintptr_t capacity, uintptr_t *out_len);
```

Same semantics as the `ErikaHandle` track functions.

### Danmaku (bullet comments)

```c
ErikaStatus erika_presenter_load_danmaku_file(ErikaPresenterHandle *, const char *uri);
ErikaStatus erika_presenter_load_danmaku_json(ErikaPresenterHandle *, const char *json);
ErikaStatus erika_presenter_add_danmaku_track_file(ErikaPresenterHandle *, const char *uri, const char *name, int64_t offset_micros, uint64_t *out_track_id);
ErikaStatus erika_presenter_add_danmaku_track_json(ErikaPresenterHandle *, const char *json, const char *name, int64_t offset_micros, uint64_t *out_track_id);
ErikaStatus erika_presenter_remove_danmaku_track(ErikaPresenterHandle *, uint64_t track_id);
ErikaStatus erika_presenter_set_danmaku_track_enabled(ErikaPresenterHandle *, uint64_t track_id, bool enabled);
ErikaStatus erika_presenter_set_danmaku_track_offset(ErikaPresenterHandle *, uint64_t track_id, int64_t offset_micros);
ErikaStatus erika_presenter_set_danmaku_global_offset(ErikaPresenterHandle *, int64_t offset_micros);
ErikaStatus erika_presenter_danmaku_tracks(ErikaPresenterHandle *, ErikaDanmakuTrackInfo *out_tracks, uintptr_t capacity, uintptr_t *out_len);
ErikaStatus erika_presenter_clear_danmaku(ErikaPresenterHandle *);
ErikaStatus erika_presenter_set_danmaku_enabled(ErikaPresenterHandle *, bool enabled);
ErikaStatus erika_presenter_set_danmaku_config(ErikaPresenterHandle *, ErikaDanmakuConfig config);
ErikaStatus erika_presenter_set_danmaku_config_ptr(ErikaPresenterHandle *, const ErikaDanmakuConfig *config);
ErikaStatus erika_presenter_get_danmaku_config(ErikaPresenterHandle *, ErikaDanmakuConfig *out_config);
ErikaStatus erika_presenter_set_danmaku_font(ErikaPresenterHandle *, const char *family, const char *file_path);
ErikaStatus erika_presenter_set_danmaku_block_words_json(ErikaPresenterHandle *, const char *json);
```

`load_danmaku_*` replaces the active danmaku with a single anonymous track;
`add_danmaku_track_*` builds a multi-track list (each with a name and time
offset). Input is Bilibili XML (`*_file`, by path/URL) or JSON (`*_json`,
inline). `offset_micros` shifts a track's timeline; the global offset shifts all
tracks. `set_danmaku_config` / `_ptr` apply the full `ErikaDanmakuConfig` (the
`_ptr` variant avoids passing the struct by value); `get_danmaku_config` reads
it back. See [danmaku_architecture.md](danmaku_architecture.md) for the layout
engine. `set_danmaku_block_words_json` takes a JSON array of strings to filter.

### Surface and presentation

```c
ErikaStatus erika_presenter_attach_metal_layer(ErikaPresenterHandle *, uint64_t raw_layer, uint32_t w, uint32_t h, double scale);
ErikaStatus erika_presenter_attach_wgpu_surface(ErikaPresenterHandle *, ErikaWgpuSurfaceKind kind, uint64_t raw_window, uint64_t raw_display, uint32_t w, uint32_t h, double scale);
ErikaStatus erika_presenter_attach_windows_hwnd(ErikaPresenterHandle *, uint64_t hwnd, uint64_t hinstance, uint32_t w, uint32_t h, double scale);
ErikaStatus erika_presenter_resize_surface(ErikaPresenterHandle *, uint32_t w, uint32_t h, double scale);
ErikaStatus erika_presenter_detach_surface(ErikaPresenterHandle *);
```

Use `attach_metal_layer` on macOS/iOS (a `CAMetalLayer*`), and
`attach_windows_hwnd` on Windows (it is a convenience wrapper over
`attach_wgpu_surface` with kind `WindowsHwnd`, passing `HWND` + `HINSTANCE`).
The renderer backend bound to the surface (native Metal, native Direct3D 11, or
wgpu) is decided by the presenter configuration, not by the attach call. Call
`resize_surface` whenever the drawable size or scale changes.

### Render loop and events

```c
ErikaStatus erika_presenter_render_tick(ErikaPresenterHandle *, double time_seconds, ErikaPresenterStats *out_stats);
ErikaStatus erika_presenter_poll_event(ErikaPresenterHandle *, ErikaEvent *out_event);
```

Call `render_tick` once per display frame (e.g. from `CADisplayLink`,
`CVDisplayLink`, or a Windows frame scheduler). `time_seconds` is the host
display clock for the frame in seconds — Erika uses it for vsync-quantized
scheduling, so pass the presentation timestamp, not wall-clock deltas. If
`out_stats` is non-`NULL` it is filled with a snapshot of pipeline counters.
`poll_event` is non-blocking and returns `NoEvent` when idle.

### Diagnostics and capture

```c
ErikaStatus erika_presenter_get_upscaler_status(ErikaPresenterHandle *, ErikaUpscalerStatus *out_status);
ErikaStatus erika_presenter_capture_frame_rgba(ErikaPresenterHandle *, uint32_t width, uint32_t height,
                                               uint8_t *out_rgba, uintptr_t out_capacity);
```

`get_upscaler_status` reports the requested upscaler mode, the active backend
(off / inactive / building / scalar / simdgroup-matrix), the fallback count,
upscaled frame count, and recent encode/GPU timings in microseconds.

`capture_frame_rgba` is a **screenshot**: it renders the current composited
frame (video + subtitle + danmaku) off-screen into a caller-allocated RGBA8
buffer at the requested `width`×`height` (independent of the display surface
size). `out_capacity` must be at least `width*height*4`. It returns `PlayerError`
when no frame is available yet, and is implemented on the native Metal and
Direct3D 11 backends (the wgpu backend returns no capture).

```c
uint32_t w = 1920, h = 1080;
uint8_t *rgba = malloc((size_t)w * h * 4);
if (erika_presenter_capture_frame_rgba(p, w, h, rgba, (uintptr_t)w * h * 4) == ErikaStatus_Ok) {
    /* rgba holds w*h tightly-packed RGBA8 pixels — encode to PNG, etc. */
}
free(rgba);
```

## Enums

| Enum | Values |
|------|--------|
| `ErikaState` | `Idle` `Opening` `Ready` `Playing` `Paused` `Stopped` `Closed` `Error` |
| `ErikaEventKind` | `None` `StateChanged` `DurationChanged` `PositionChanged` `TracksChanged` `BufferingChanged` `VideoParamsChanged` `SurfaceAttached` `SurfaceDetached` `Error` `TrackSelectionChanged` |
| `ErikaTrackKind` | `Video` `Audio` `Subtitle` |
| `ErikaTrackSource` | `Embedded` `External` |
| `ErikaWgpuSurfaceKind` | `Unknown` `MacOsNsView` `MacOsCaMetalLayer` `IosUiView` `WindowsHwnd` `XlibWindow` `WaylandSurface` `AndroidNativeWindow` |
| `ErikaFlutterTextureKind` | `Unknown` `MacOsTextureRegistrar` `IosTextureRegistrar` `AndroidSurfaceTexture` `WindowsTextureRegistrar` `LinuxTextureRegistrar` |
| `ErikaPresenterOutputMode` | `Sdr` `AppleEdr` |
| `ErikaLumaUpscalerMode` | `Off` `ArtCnnC4F16` `ArtCnnC4F32` |
| `ErikaUpscalerBackendStatus` | `Off` `Inactive` `Building` `Scalar` `SimdgroupMatrix` |

## Structs

- **`ErikaPresenterConfig`** `{ int32 output_mode; float edr_headroom; int32 luma_upscaler; }` —
  passed by value to `create_with_config`.
- **`ErikaUpscalerStatus`** — requested mode, active backend, fallback count,
  upscaled frames, last encode/GPU micros.
- **`ErikaDanmakuConfig`** — full danmaku layout/appearance config (font size,
  opacity, display area, scroll timing, collision/stacking flags, blocked modes,
  shadow style). `font_size` is a NipaPlay/Flutter *logical* size; Erika
  multiplies by the surface scale for glyph pixels.
- **`ErikaDanmakuTrackInfo`** `{ id, enabled, offset_micros, item_count, char *name, char *source }` —
  free with `erika_danmaku_track_info_free`.
- **`ErikaVideoParams`** `{ width, height, primaries, transfer }` — color
  metadata reported via `VideoParamsChanged`.
- **`ErikaTrackCounts`** / **`ErikaTrackSelection`** — per-kind counts / selected
  ids (`-1` = none).
- **`ErikaTrackInfo`** — full per-track metadata; the six `char*` fields are
  owned by the caller (free via `erika_track_info_free`).
- **`ErikaEvent`** — a tagged union-by-struct: `kind` selects which fields are
  meaningful (`state`, `duration_micros`, `position_micros`, `buffering`,
  `video`, `tracks`); `status` carries the code for `Error` events.
- **`ErikaPresenterStats`** — pipeline counters: decoded/rendered frames,
  pushed audio frames, overlay/danmaku frames, hardware vs software vs zero-copy
  frame counts, HDR source/HDR10-output/SDR-tonemap counts, audio-clock
  read/queued/underflow frames, and last render timings.

## Events

Poll on each loop iteration and dispatch on `kind`:

```c
ErikaEvent ev;
while (erika_presenter_poll_event(p, &ev) == ErikaStatus_Ok) {
    switch (ev.kind) {
        case ErikaEventKind_StateChanged:    /* ev.state */                 break;
        case ErikaEventKind_DurationChanged: /* ev.duration_micros */        break;
        case ErikaEventKind_PositionChanged: /* ev.position_micros */        break;
        case ErikaEventKind_TracksChanged:   /* re-query erika_*_tracks */   break;
        case ErikaEventKind_BufferingChanged:/* ev.buffering */              break;
        case ErikaEventKind_VideoParamsChanged: /* ev.video */               break;
        case ErikaEventKind_Error:           /* ev.status + last_error */    break;
        default: break;
    }
}
```

The event queue is bounded and drained by polling; a host that stops polling
will simply stop observing state. `position_micros` is emitted periodically
during playback.

## Minimal presenter integration (C)

```c
#include "erika.h"

ErikaPresenterHandle *p = erika_presenter_create();
erika_presenter_attach_metal_layer(p, (uint64_t)layer, w, h, scale);  // or attach_windows_hwnd
if (erika_presenter_open(p, "/path/to/video.mkv") != ErikaStatus_Ok) {
    char *m = erika_last_error_message(); /* log */ erika_string_free(m);
}
erika_presenter_play(p);

// Per display frame:
ErikaPresenterStats stats;
erika_presenter_render_tick(p, host_time_seconds, &stats);
ErikaEvent ev;
while (erika_presenter_poll_event(p, &ev) == ErikaStatus_Ok) { /* dispatch */ }

// Teardown:
erika_presenter_detach_surface(p);
erika_presenter_destroy(p);
```

See [integration.md](integration.md) for the per-platform surface and
display-timer details, and the runnable
[`macos_native_demo`](../examples/macos_native_demo) /
[`windows_native_demo`](../examples/windows_native_demo) examples.
