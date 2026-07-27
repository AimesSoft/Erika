# Flutter Embedding

[中文](flutter_embedding.zh.md) | [English](flutter_embedding.md) | [日本語](flutter_embedding.ja.md)

Erika 不是 Flutter 视频渲染器。Flutter 只是可选宿主 UI。播放器内部负责解码、时序、原生渲染、字幕、弹幕、音频和 HDR 呈现。

## API Families

有两组 C ABI 入口：

- `ErikaHandle`：控制与事件 API。适合宿主管理自己的 presenter loop，或只想探测/控制播放的场景。
- `ErikaPresenterHandle`：presenter-owned API。适合 Erika 自己持有 `Player + renderer + audio output`，宿主只提供 native surface 和 display tick callback 的场景。

两组入口都声明在 `crates/erika_capi/include/erika.h`。

## Apple Surface Strategies

Apple HDR 路径使用 native Metal-backed surface，而不是 Flutter Texture。Flutter plugin 在 macOS 和 iOS 上都提供两种 native surface 策略，方便宿主按 UI 结构选择合适的合成模型。

### ErikaVideoView (Platform View)

标准 Flutter platform view，macOS 上由 `NSView`/`CAMetalLayer` 提供，iOS 上由 `UIView`/`CAMetalLayer` 提供。plugin 会创建注册为 `erika_flutter/video_view` 的 native video view，attach 到 presenter，并通过 display link 驱动渲染。

这个路径适合简单嵌入和诊断。macOS 上它不是推荐的生产路径，因为 AppKit/Flutter platform view 合成可能出现黑屏闪烁或其他 compositor artifacts。

### ErikaWindowOverlayVideoView (Window Overlay)

这是推荐的 HDR/EDR 路径。plugin 会创建一个 window-hosted native overlay，位于 Flutter 的 platform-view compositor 之外：

1. Dart `ErikaWindowOverlayVideoView` 在 widget tree 中预留一个矩形区域。
2. platform plugin 创建一个 window-level native view，使用 `CAMetalLayer` 作为 Flutter host view 的 sibling/underlay。
3. Flutter 将该 widget 区域绘制为透明，为 native video 留出空洞。
4. widget 跟踪自身位置，并通过 surface generation number 发送几何更新，因此已销毁 widget 的旧 hide 调用不会影响新 attach 的 surface。
5. attach retry 使用 exponential backoff 处理 window readiness 时机。

这个 overlay 路径是 NipaPlay 和其他 full-player UI 的推荐方案。它让视频呈现由 Erika/Metal 持有，而 Flutter 继续承担控制层和布局层。iOS 上 native side 使用 `UIWindow` 加 sibling `UIView`/`CAMetalLayer`；macOS 上使用 host `NSWindow` 加 sibling `NSView`/`CAMetalLayer`。

触摸事件会穿透两种 native video strategy，因此 Flutter controls 可以保持在视频 surface 上方或周围。

## Android Surface Strategies

Android 上两个视频 widget 都使用同一套 native-view selector。SDR 使用真实的
`TextureView`，且已完成验证；wgpu 优先选择 Vulkan，并提供有界 GLES fallback。请求
`ErikaOutputMode.extendedLinear` 时则通过 `PlatformViewLink` 和 Hybrid Composition
创建 `SurfaceView`，避免 FP16 scRGB 经过 Flutter texture-layer compositor。surface 由
`Choreographer` 驱动，lifecycle、resize、audio focus 和 output fallback 仍由 plugin 管理。

FP16 extended-linear scRGB 已实现完整的 `Rgba16Float` 协商和
`ADATASPACE_SCRGB_LINEAR` 验证，但 active path 尚不宣称通过真机验收；最终仍需 API 35
HDR 真机。显示器不支持 HDR、GLES、`TextureView`、缺少 FP16 或 dataspace 验证失败时都会
继续 SDR 播放，并提供可查询的 fallback reason 和明确日志。

## iOS Build Path

iOS plugin 通过 CocoaPod script phase 把 Erika C ABI static library 链接进 app，并为目标 iOS architecture 构建 Rust `erika_capi` crate。

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

// 每个 display tick：
ErikaPresenterStats stats;
erika_presenter_render_tick(presenter, host_time_seconds, &stats);

// resize 时：
erika_presenter_resize_surface(presenter, width, height, backing_scale);

// dispose 时：
erika_presenter_detach_surface(presenter);
erika_presenter_destroy(presenter);
```

## Flutter Texture Path

Flutter Texture 是一个能力更低的兼容路径。

适合：
- SDR fallback。
- native view composition 尚未准备好的平台。
- 测试 surface 或受限 embedding 环境。

它不是首选 HDR/EDR 路径，因为视频会进入 Flutter compositor。C ABI 为此路径保留了 `erika_attach_flutter_texture`。

## wgpu 与 Android

Apple HDR 路径仍使用 native Metal，Windows 使用 native Direct3D 11 渲染器（D3D11VA
零拷贝解码、HDR10 输出）。Android 上 wgpu 是实际渲染器：Vulkan 通过 AHardwareBuffer
导入 MediaCodec Surface 帧，software frame 则有明确的 CPU upload fallback；视频、字幕、
弹幕、截图和 ArtCNN compute 共用这条路径。Vulkan 可协商 FP16 extended-linear scRGB，
GLES 或能力协商失败会明确回退 SDR。Android SDR 已验证，API 35 HDR 真机 active path
仍待验收；Linux 支持仍在规划中。

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

// Compatibility / diagnostic platform-view path:
ErikaVideoView(player: player)

// Playback control
await player.pause();
await player.seek(Duration(seconds: 30));
await player.setVolume(0.8);
await player.setPlaybackRate(1.5);

// Neural upscaler (anime luma 2x; Apple Metal / Android Vulkan)
await player.setUpscaler(ErikaUpscalerMode.artCnnC4F16); // off / artCnnC4F16 / artCnnC4F32
final status = await player.getUpscalerStatus();

// Track management
final tracks = await player.tracks();
await player.selectAudioTrack(trackId);
await player.selectSubtitleTrack(trackId);
await player.addExternalSubtitle('/path/to/subtitle.srt');
await player.setSubtitleScale(1.2);
// 字幕回退字体与颜色（0xRRGGBBAA）；forceOverride 还会覆盖 ASS 脚本自带的样式。
await player.setSubtitleStyle(
  fontFamily: 'Source Han Sans SC',
  primaryColorRgba: 0xFFFFFFFF,
  outlineColorRgba: 0x0000007F,
  fontSize: 48,
  outlineWidth: 2,
);

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

`setUpscaler` 只是在请求一个模式；kernel 会在后台线程编译，所以宿主应该轮询 `getUpscalerStatus` 来驱动 UI：

| `activeBackend` | 含义 |
|-----------------|------|
| `off` | 没有请求任何模式。 |
| `building` | kernel 正在编译（首次使用该模式）；在准备好之前视频会保持未放大。 |
| `inactive` | 请求了模式，但这一帧没有生效，例如视频显示尺寸没有超过源分辨率，或源视频是 HDR（upscaler 只处理 SDR luma）。 |
| `scalar` | 运行在 Metal scalar 或 wgpu compute backend 上。 |
| `simdgroupMatrix` | 运行在 `simdgroup_matrix` backend 上（Apple Silicon 默认）。 |

只有当 drawable 显示的视频尺寸大于源分辨率时，upscaler 才会生效，所以 1080p 源在
1080p（或更小）视图里会保持 `inactive`。C4F16 是实时推荐；Apple 上的 C4F32 在 1080p
输入下通常需要 M-Pro/Max 级别 GPU。Android 上两个模型都使用 Vulkan compute，GLES 会
明确报告 `inactive` fallback。渲染器侧设计见 `docs/architecture.md`。

## Ownership Rule

Flutter 负责布局和 controls。Erika 负责 video plane、subtitle plane、danmaku plane、audio 和 timing。plugin 通过 `MethodChannel` 传递命令和事件；渲染不会经过 Dart。
