# Flutter Embedding

[中文](flutter_embedding.zh.md) | [English](flutter_embedding.md) | [日本語](flutter_embedding.ja.md)

Erika is not a Flutter video renderer. Flutter is an optional host UI.
The player owns decode, timing, native rendering, subtitles, danmaku, audio, and
HDR presentation.

## API Families

There are two C ABI entrypoint families:

- `ErikaHandle`: control and event API. Use this when the host owns its own
  presenter loop or only wants to probe/control playback.
- `ErikaPresenterHandle`: presenter-owned API. Use this when Erika should
  own `Player + renderer + audio output` and the host only supplies a
  native surface plus a display-tick callback.

Both families are declared in `crates/erika_capi/include/erika.h`.

## Apple Surface Strategies

The Apple HDR path uses a native Metal-backed surface, not Flutter Texture.
The Flutter plugin intentionally exposes two native surface strategies on both
macOS and iOS so hosts can pick the composition model that matches their UI.

### ErikaVideoView (Platform View)

Standard Flutter platform view backed by `NSView`/`CAMetalLayer` on macOS and
`UIView`/`CAMetalLayer` on iOS. The plugin creates a native video view
registered as `erika_flutter/video_view`, attaches it to the presenter, and
drives rendering from a display link.

This path is useful for simple embedders and diagnostics. On macOS it is not the
recommended production path because AppKit/Flutter platform view composition can
show black flicker or other compositor artifacts.

### ErikaWindowOverlayVideoView (Window Overlay)

For the preferred HDR/EDR path, the plugin creates a window-hosted native
overlay that sits outside Flutter's platform-view compositor:

1. Dart `ErikaWindowOverlayVideoView` reserves a rectangle in the widget tree.
2. The platform plugin creates a window-level native view with a `CAMetalLayer`
   as a sibling/underlay of the Flutter host view.
3. Flutter paints the widget region transparent, leaving a hole for native video.
4. The widget tracks its position and sends geometry updates with a surface
   generation number, so stale hide calls from disposed widgets cannot affect
   newly attached surfaces.
5. Attach retry with exponential backoff handles window readiness timing.

The overlay path is the recommended path for NipaPlay and other full-player
UIs. It keeps video presentation owned by Erika/Metal while Flutter remains a
control and layout layer. On iOS the native side uses `UIWindow` plus a sibling
`UIView`/`CAMetalLayer`; on macOS it uses the host `NSWindow` plus a sibling
`NSView`/`CAMetalLayer`.

Touch events pass through both native video strategies, so Flutter controls can
remain above or around the video surface.

## Android Surface Strategies

On Android, both video widgets use the same native-view selector. SDR uses a
real `TextureView` and has been verified. wgpu selects Vulkan with a bounded
GLES fallback. Requesting `ErikaOutputMode.extendedLinear` instead creates a
`SurfaceView` through `PlatformViewLink` and Hybrid Composition so FP16 scRGB
does not pass through Flutter's texture-layer compositor. `Choreographer`
drives the surface, while lifecycle, resize, audio focus, and output fallback
remain owned by the plugin.

The FP16 extended-linear scRGB implementation is complete, including
`Rgba16Float` negotiation and `ADATASPACE_SCRGB_LINEAR` verification. Its active
path is not yet claimed as device-validated: final acceptance still requires an
API 35 HDR device. Unsupported displays, GLES, `TextureView`, missing FP16, or
dataspace verification failures continue in SDR with a queryable fallback
reason and explicit logs.

## HarmonyOS Surface Strategies

On HarmonyOS, use `ErikaVideoView`. The ArkTS plugin registers a Flutter
external texture, takes that texture's surface as an `OHNativeWindow`, and
attaches it to the presenter; wgpu then renders through Vulkan, using
`VK_OHOS_surface` for window-system integration.

Video decoding defaults to HarmonyOS AVCodec (H.264 and HEVC). AVCodec decodes
straight into a Surface, whose `OHNativeBuffer` is imported as a Vulkan
external image and resolved by a Vulkan YCbCr sampler, so decoded frames reach
the compositor with no CPU copy. Subtitles, danmaku, and overlays composite in
the same wgpu pass as every other platform.

Devices missing the required Vulkan extensions fall back to FFmpeg software
decode with CPU upload. The fallback is reported through `VideoDecoderChanged`
events and presenter diagnostics instead of failing playback. The HarmonyOS
path is validated on device but is not yet covered by CI.

## iOS Build Path

The iOS plugin links the Erika C ABI static library into the app through a
CocoaPod script phase that builds the Rust `erika_capi` crate for the target iOS
architecture.

## Minimal Presenter Flow

```c
ErikaPresenterHandle *presenter = erika_presenter_create();
erika_presenter_attach_metal_layer(
    presenter,
    (uint64_t)cametal_layer,
    width,
    height,
    backing_scale);
erika_presenter_open(presenter, "/path/to/media.mp4");
erika_presenter_play(presenter);

// On every display tick:
ErikaPresenterStats stats;
erika_presenter_render_tick(presenter, host_time_seconds, &stats);

// On resize:
erika_presenter_resize_surface(presenter, width, height, backing_scale);

// On dispose:
erika_presenter_detach_surface(presenter);
erika_presenter_destroy(presenter);
```

## Flutter Texture Path

Flutter Texture is a lower-capability compatibility path.

Useful for:
- SDR fallback.
- Platforms where native view composition is not ready.
- Test surfaces or constrained embedding environments.

Not the preferred HDR/EDR route because video enters Flutter's compositor. The
C ABI reserves `erika_attach_flutter_texture` for this path.

## wgpu and Android

The Apple HDR path remains native Metal, and Windows uses a native Direct3D 11
renderer (D3D11VA zero-copy decode, HDR10 output). On Android, wgpu is the active
renderer: Vulkan imports MediaCodec Surface frames through AHardwareBuffer, and
software frames have an explicit CPU-upload fallback. Video, subtitles,
danmaku, capture, and ArtCNN compute share this path. Vulkan can negotiate FP16
extended-linear scRGB; GLES and failed capability negotiation explicitly fall
back to SDR. Android SDR is verified, while the API 35 HDR-device active-path
acceptance remains pending. Linux support remains planned.

## Dart API

```dart
final player = ErikaPlayer(
  outputMode: ErikaOutputMode.appleEdr,  // optional: force EDR
  edrHeadroom: 4.0,                      // optional: EDR headroom
);

await player.open(
  'https://example.com/video.mp4',
  httpHeaders: <String, String>{
    'Authorization': 'Bearer token',
    'Referer': 'https://example.com/',
  },
);
await player.play();

// Preferred for full-player UIs on macOS/iOS:
ErikaWindowOverlayVideoView(player: player)

// Compatibility/diagnostic platform-view path:
ErikaVideoView(player: player)

// Playback control
await player.pause();
await player.seek(Duration(seconds: 30));
await player.setVolume(0.8);
await player.setPlaybackRate(1.5);

// Neural upscaler (anime luma 2x; Apple Metal / Android Vulkan)
await player.setUpscaler(ErikaUpscalerMode.artCnnC4F16); // off / artCnnC4F16 / artCnnC4F32
final status = await player.getUpscalerStatus();
// status.requestedMode  -- what was requested
// status.activeBackend  -- off / inactive / building / scalar / simdgroupMatrix
// status.upscaledFrames -- frames produced by the network so far

// Track management
final tracks = await player.tracks();
await player.selectAudioTrack(trackId);
await player.selectSubtitleTrack(trackId);
await player.addExternalSubtitle('/path/to/subtitle.srt');

// Danmaku
await player.loadDanmakuFile('/path/to/danmaku.xml');
await player.addDanmakuTrackJson(jsonString, name: 'source', offset: Duration.zero);
await player.setDanmakuConfig(fontSize: 30, displayArea: 0.5);

// Events
player.events.listen((event) {
  // event.kind, event.state, event.position, event.duration, ...
});

await player.dispose();
```

## Neural Upscaler Status

`setUpscaler` requests a mode; the kernels are compiled on a background thread,
so the host should poll `getUpscalerStatus` to drive its UI:

| `activeBackend` | Meaning |
|-----------------|---------|
| `off` | No mode requested. |
| `building` | Kernels compiling (first use of a mode); frames render unscaled until ready. |
| `inactive` | Mode requested but not applied this frame — e.g. the video is not displayed above its source resolution, or the source is HDR (upscaler runs on SDR luma only). |
| `scalar` | Running on the Metal scalar or wgpu compute backend. |
| `simdgroupMatrix` | Running on the `simdgroup_matrix` backend (Apple Silicon default). |

The upscaler only engages when the drawable shows the video larger than its
source resolution, so a 1080p source in a 1080p (or smaller) view stays
`inactive`. C4F16 is the real-time recommendation. On Apple, C4F32 generally
needs an M-Pro/Max-class GPU at 1080p input; on Android, both models use Vulkan
compute and GLES reports an explicit `inactive` fallback. See
`docs/architecture.md` for the renderer-side design.

## Ownership Rule

Flutter owns layout and controls. Erika owns the video plane, subtitle plane,
danmaku plane, audio, and timing. The plugin bridges commands and events through
a `MethodChannel`; rendering never passes through Dart.
