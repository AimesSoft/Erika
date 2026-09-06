# Flutter Embedding

[中文](flutter_embedding.zh.md) | [English](flutter_embedding.md) | [日本語](flutter_embedding.ja.md)

Erika 不是 Flutter 视频渲染器。Flutter 只是可选宿主 UI。播放器内部负责解码、时序、原生渲染、字幕、弹幕、音频和 HDR 呈现。

## API Families

有两组 C ABI 入口：

- `ErikaHandle`：控制与事件 API。适合宿主管理自己的 presenter loop，或只想探测/控制播放的场景。
- `ErikaPresenterHandle`：presenter-owned API。适合 Erika 自己持有 `Player + renderer + audio output`，宿主只提供 native surface 和 display tick callback 的场景。

两组入口都声明在 `crates/erika_capi/include/erika.h`。

## Apple Surface Strategies

Apple HDR 路径使用 native Metal-backed surface，而不是 Flutter Texture。Flutter plugin 在 macOS、iOS 和 tvOS 上都提供两种 native surface 策略，方便宿主按 UI 结构选择合适的合成模型。

### ErikaVideoView (Platform View)

标准 Flutter platform view，macOS 上由 `NSView`/`CAMetalLayer` 提供，iOS/tvOS 上由 `UIView`/`CAMetalLayer` 提供。plugin 会创建注册为 `erika_flutter/video_view` 的 native video view，attach 到 presenter，并通过 display link 驱动渲染。

这个路径适合简单嵌入和诊断。macOS 上它不是推荐的生产路径，因为 AppKit/Flutter platform view 合成可能出现黑屏闪烁或其他 compositor artifacts。

### ErikaWindowOverlayVideoView (Window Overlay)

这是推荐的 HDR/EDR 路径。plugin 会创建一个 window-hosted native overlay，位于 Flutter 的 platform-view compositor 之外：

1. Dart `ErikaWindowOverlayVideoView` 在 widget tree 中预留一个矩形区域。
2. platform plugin 创建一个 window-level native view，使用 `CAMetalLayer` 作为 Flutter host view 的 sibling/underlay。
3. Flutter 将该 widget 区域绘制为透明，为 native video 留出空洞。
4. widget 跟踪自身位置，并通过 surface generation number 发送几何更新，因此已销毁 widget 的旧 hide 调用不会影响新 attach 的 surface。
5. attach retry 使用 exponential backoff 处理 window readiness 时机。

这个 overlay 路径是 NipaPlay 和其他 full-player UI 的推荐方案。它让视频呈现由 Erika/Metal 持有，而 Flutter 继续承担控制层和布局层。iOS/tvOS 上 native side 使用 window 加 sibling `UIView`/`CAMetalLayer`；macOS 上使用 host `NSWindow` 加 sibling `NSView`/`CAMetalLayer`。

触摸事件会穿透两种 native video strategy，因此 Flutter controls 可以保持在视频 surface 上方或周围。

### ErikaTextureVideoView (Flutter Texture)

`ErikaTextureVideoView` 通过 Flutter 的 texture registrar 渲染，而不是 platform view。macOS 上 plugin 会分配 IOSurface-backed 的 `CVPixelBuffer` 池，直接渲染进其 Metal texture（无逐帧 CPU 回读），再把帧发布给 Flutter，因此 `Opacity`、裁剪、变换和颜色滤镜等常规 Flutter 效果都作用在视频上。OpenHarmony 上复用已注册的外部纹理。其他平台回退到 `ErikaVideoView`。

macOS 上当 `blendMode` 不是 `srcOver` 时，widget 会改走 native platform view，让 Core Animation 基于真实背景做混合（见下文"透明视频与混合模式"）。

Windows 上 `ErikaVideoView` 默认继续使用原生 HWND/swapchain，保留普通视频的 HDR10 输出；不会根据当前片源或 `outputMode` 自动切入 Flutter Texture。显式使用 `ErikaTextureVideoView` 才启用 SDR BGRA8 texture；`srcOver` 支持 Flutter 的透明度、裁剪、变换和颜色滤镜，`overlay` 转入原生 Windows Composition，其余混合模式明确报错。

Windows texture 每次画面内容变化后通过 GPU copy 发布完成且不可变的快照，无 CPU 像素回读；复制完成前不会交给 Flutter。暂停且字幕/弹幕/HUD 未变化时复用已发布快照，不重复通知 Flutter。尺寸变化、seek、字幕、弹幕和 HUD 更新均会使输出失效。Flutter 打开共享句柄不等于 GPU 采样结束，因此已发布快照永不被再次写入。插件仅保留最新和正在被 raster callback 读取的快照，未消费的中间 resize 快照可立即回收。

此能力需要匹配的 v0.1.8 或更新版本 native runtime。Flutter package 在 `native_artifacts.properties` 中固定原生版本和校验值。旧 runtime 缺少 texture 符号时，仅显式 texture 绑定报错，不影响原生播放器创建。

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

## HarmonyOS Surface Strategies

HarmonyOS 上请使用 `ErikaVideoView`。ArkTS 插件注册 Flutter 外部纹理，把该纹理的
surface 取为 `OHNativeWindow` 并 attach 给 presenter；wgpu 随后通过 Vulkan 渲染，
窗口系统集成走 `VK_OHOS_surface`。

视频解码默认使用 HarmonyOS AVCodec（H.264 与 HEVC）。AVCodec 直接解码到 Surface，
其 `OHNativeBuffer` 作为 Vulkan 外部图像导入，并由 Vulkan YCbCr sampler 解析，
因此解码帧无需 CPU 拷贝即可到达合成器。字幕、弹幕和 overlay 与其他平台一样，
在同一个 wgpu pass 里合成。

缺少所需 Vulkan 扩展的设备回退到 FFmpeg 软解 + CPU 上传。回退通过
`VideoDecoderChanged` 事件和 presenter 诊断上报，而不是让播放失败。HarmonyOS
路径已在真机验证；CI 构建 OpenHarmony C ABI，但无设备侧运行验证。

## 透明视频与混合模式

Erika 可以呈现带透明度的视频素材，同时保留 Flutter 的布局与合成能力。透明度按 player 请求：

```dart
final player = ErikaPlayer(
  videoAlphaMode: ErikaVideoAlphaMode.packedAlphaRight,
);
```

`ErikaVideoAlphaMode.packedAlphaRight` 要求左右分区的编码帧：左半是颜色，右半是灰度 alpha 蒙版。GPU 呈现着色器（Metal、wgpu、D3D11）会重建预乘 alpha 的帧，视频以编码宽度的一半呈现。未请求该模式的 player 仍将画面视为完全不透明，因此既有内容不受影响。

混合与不透明度在视频 widget 上请求：

```dart
ErikaTextureVideoView(
  player: player,
  blendMode: BlendMode.overlay,
  opacity: 0.8,
)
```

各平台支持范围不同：

| 平台与路径 | `blendMode` | `opacity` |
|------------|-------------|-----------|
| macOS，`ErikaTextureVideoView`（Flutter texture） | 仅 `srcOver`；其他值改走 native 层 | Flutter `Opacity` |
| macOS，native 层（`blendMode != srcOver`） | 支持 `overlay`；其他值忽略 | Native `CALayer` opacity |
| Windows 窗口 overlay（DirectComposition） | `srcOver`、`overlay`；其他值抛插件错误 | `IDCompositionEffectGroup` opacity |
| Android / iOS / OpenHarmony | 接受创建参数但不消费 | texture 路径上由 Flutter 侧应用 |

overlay 混合是背景感知的：macOS 的 native 层基于窗口内视频背后的内容合成，Windows 的 blend effect 采样同一个 HWND，因此底层的 Flutter 或游戏画面保持可见。Windows 透明 overlay 视频使用绑定到 Flutter HWND 的 SDR BGRA8 预乘 composition swap chain；HDR 片源会在 Erika 内部 tone map 到该合成空间。不透明的 Windows overlay 视频仍沿用之前的专用子 HWND 路径；解码器或输出设备重建 swap chain 时会自动重新绑定 composition 内容。

## iOS Build Path

iOS plugin 通过 CocoaPod script phase 把 Erika C ABI static library 链接进 app。默认下载匹配的预构建归档；设 `ERIKA_FORCE_SOURCE_BUILD=1`（配合 `ERIKA_REPO_ROOT`）才会为目标 iOS architecture 从源码构建 Rust `erika_capi` crate。

## tvOS Build Path

tvOS plugin 通过 CocoaPod script phase 链接 Erika C ABI static library；与 iOS 一样默认下载预构建归档，`ERIKA_FORCE_SOURCE_BUILD=1` 时才从源码构建。支持 tvOS 13+、arm64 真机，以及 arm64/x86_64 模拟器。详细的 Rust nightly、预构建包和源码构建选项见 [`packages/erika_flutter/README.zh.md`](../packages/erika_flutter/README.zh.md)。

## macOS Build Path

macOS pod 使用同样的 script-phase 构建。在 Erika checkout 内（包上层存在 `crates/erika_capi/Cargo.toml`，或 `ERIKA_REPO_ROOT` 指向 checkout 时）默认从源码构建 Rust `erika_capi`，这样本地渲染器改动无需等待新的预构建发布即可生效。已发布的包与隔离消费者——包括解析到 pub cache 的 git 依赖（它们保留了整个仓库，因此同样走源码构建）——该路径需要 Rust 工具链；设 `ERIKA_FORCE_PREBUILT=1` 可改为始终下载带校验的预构建归档。

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
- 需要进入 Flutter compositor 的透明视频（macOS/OpenHarmony 上的 `ErikaTextureVideoView`）。
- 测试 surface 或受限 embedding 环境。

它不是首选 HDR/EDR 路径，因为视频会进入 Flutter compositor。Apple 上 surface 是 IOSurface-backed 的 `CVPixelBuffer`：宿主注册 Flutter texture，在每个 render tick 前用 `erika_presenter_attach_flutter_texture` 和 `erika_presenter_set_flutter_texture_buffer` 选定该帧的 Metal texture，Flutter 随后合成同一个 buffer，无 CPU 回读。pixel buffer 的所有权始终在宿主侧。

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
  // optional: 左右分区颜色/alpha 素材以透明方式呈现
  videoAlphaMode: ErikaVideoAlphaMode.packedAlphaRight,
);

await player.open(
  'https://example.com/video.mp4',
  httpHeaders: <String, String>{
    'Authorization': 'Bearer token',
    'Referer': 'https://example.com/',
  },
  httpReadAheadBytes: 16 * 1024 * 1024,
);
await player.play();

// Preferred for full-player UIs on macOS/iOS/tvOS:
ErikaWindowOverlayVideoView(player: player)

// Flutter 合成的视频，支持不透明度/裁剪/滤镜（macOS/OpenHarmony）：
ErikaTextureVideoView(player: player, opacity: 0.8)

// Compatibility / diagnostic platform-view path:
ErikaVideoView(player: player)

// Playback control
await player.pause();
await player.seek(Duration(seconds: 30));
await player.setVolume(0.8);
await player.setPlaybackRate(1.5);

// Neural upscaler (anime luma 2x; Apple Metal / Android Vulkan)
await player.setUpscaler(ErikaUpscalerMode.artCnnC4F16Ds); // 推荐用于有明显劣化的片源
final status = await player.getUpscalerStatus();

// Track management
final tracks = await player.tracks();
for (final track in tracks) {
  if (track.kind == ErikaTrackKind.video && track.selected) {
    print('${track.codec} ${track.width}x${track.height}');
    print('${track.bitRate} bps / ${track.framesPerSecond} fps');
    break;
  }
}
await player.selectAudioTrack(trackId);
await player.selectSubtitleTrack(trackId);
await player.addExternalSubtitle('/path/to/subtitle.srt');
await player.setSubtitleScale(1.2);
// 字幕的回退外观（颜色为 0xRRGGBBAA）。省略的参数沿用该 player 上次应用的值；
// 置起 overrideMask 的对应位还会覆盖 ASS 脚本自带的样式。
await player.setSubtitleStyle(
  fontFamily: 'Source Han Sans SC',
  primaryColorRgba: 0xFFFFFFFF,
  outlineColorRgba: 0x0000007F,
  fontSize: 48,
  outlineWidth: 2,
  overrideMask:
      kErikaSubtitleOverrideFontName |
      kErikaSubtitleOverrideColors |
      kErikaSubtitleOverrideFontSizeFields |
      kErikaSubtitleOverrideBorder,
);

// Danmaku
await player.loadDanmakuFile('/path/to/danmaku.xml');
await player.addDanmakuTrackJson(jsonString, name: 'source', offset: Duration.zero);
await player.setDanmakuConfig(fontSize: 30, displayArea: 0.5);

// Native diagnostics HUD (disabled by default)
await player.setDebugHudEnabled(true);
final presenterStats = await player.getPresenterStats();

// Events
player.events.listen((event) {
  // event.kind, event.state, event.position, event.duration, ...
});

await player.dispose();
```

## 媒体轨道信息

`tracks()` 返回每条嵌入或外挂轨道的 `ErikaTrackInfo`。视频轨道的 `codec`、`width`、
`height`、`pixelFormat`、`profile`、`level`、`bitRate`、`frameRateNumerator` 和
`frameRateDenominator` 可用于媒体详情页；音频轨道还包含 `sampleRate`、`channels` 和
`sampleFormat`。

- `bitRate` 单位为 bit/s。优先使用视频轨自身的编码参数；仅在单视频轨缺少该值、容器总码率
  和所有其它音频轨码率均已知时，才以“容器总码率减去音频轨码率”估算。未知时为 `null`；它不是
  实时瞬时码率，估算值也可能包含封装开销或其它非音频流的影响。
- `frameRateNumerator` / `frameRateDenominator` 保留原始有理数，避免把 `30000/1001` 截断。
  探测顺序为平均帧率、`r_frame_rate` 和 FFmpeg 的估算帧率；`framesPerSecond` 是 Dart 的便利
  getter。对可变帧率内容它仍是平均、声明或估算值，而非每帧更新值。
- `TracksChanged` 和 `TrackSelectionChanged` 事件的 `trackList` 会附带完整轨道列表。也可以在
  收到事件后再次调用 `tracks()` 获取当前快照。

```dart
player.events.listen((event) {
  if (event.kind == ErikaEventKind.tracksChanged) {
    for (final track in event.trackList) {
      if (track.kind == ErikaTrackKind.video && track.selected) {
        print(track.toMap());
        break;
      }
    }
  }
});
```

## 原生调试 HUD

`setDebugHudEnabled(true)` 让 Erika 在原生视频合成中绘制诊断 HUD；它不经过 Dart 渲染，
也不会改变 Flutter widget 层级。默认关闭，适合开发、性能分析和问题截图前的现场观察。

HUD 以低频快照显示轨道编码/分辨率/码率/帧率、播放位置与倍速、实时解码与渲染 FPS、硬件或
软件解码路径、零拷贝/回退计数、CPU/GPU 渲染耗时、音频队列与 underflow、HDR 输出协商和
弹幕数量。实时 FPS 是相邻统计采样窗口的增量；其它帧数和失败数为 presenter 生命周期内的
累计计数。HUD 不包含在 `screenshot()` 返回的离屏截图中。

如需自行设计 UI，使用 `getPresenterStats()` 获取最近一次原生显示 tick 的统计快照；它不是
HUD 的驱动机制，数据新鲜度取决于已挂载 surface 的显示循环。

## Neural Upscaler Status

`setUpscaler` 只是在请求一个模式；kernel 会在后台线程编译，所以宿主应该轮询 `getUpscalerStatus` 来驱动 UI：

| `activeBackend` | 含义 |
|-----------------|------|
| `off` | 没有请求任何模式。 |
| `building` | kernel 正在编译（首次使用该模式）；在准备好之前视频会保持未放大。 |
| `inactive` | 请求了模式但未生效——内核未就绪（且不在编译中），或后端记录了回退/失败。 |
| `scalar` | 运行在 Metal scalar 或 wgpu compute backend 上。 |
| `simdgroupMatrix` | 运行在 `simdgroup_matrix` backend 上（Apple Silicon 默认）。 |

只有当 drawable 显示的视频尺寸大于源分辨率时，upscaler 才会生效，所以 1080p 源在
1080p（或更小）视图里会保持 `inactive`。C4F16 是实时推荐；C4F16 DS 以相同算力成本面向严重压制或噪声片源；Apple 上的 C4F32 在 1080p
输入下通常需要 M-Pro/Max 级别 GPU。Android 上两个模型都使用 Vulkan compute，GLES 会
明确报告 `inactive` fallback。渲染器侧设计见 `docs/architecture.md`。

## Ownership Rule

Flutter 负责布局和 controls。Erika 负责 video plane、subtitle plane、danmaku plane、audio 和 timing。plugin 通过 `MethodChannel` 传递命令和事件；渲染不会经过 Dart。
