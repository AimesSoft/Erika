# Erika Architecture

[中文](architecture.zh.md) | [English](architecture.md) | [日本語](architecture.ja.md)

Erika 是一个可嵌入的 Rust 媒体播放库。宿主应用可以通过 Rust API、C ABI (`erika_capi`) 或 Flutter 绑定 (`erika_flutter`) 调用它。视频帧、字幕和弹幕都留在引擎内部，并在渲染器里合成，不会流经宿主渲染管线。

## 系统概览

```text
Rust Player Core
  source abstraction ─── file + HTTP range
  FFmpeg wrappers ────── custom AVIO, probe, demux, decode, seek, audio resample
  playback engine ────── video/audio tick, clock, frame scheduler
  video decode ───────── VideoToolbox (macOS/iOS), D3D11VA (Windows), software fallback
  audio output ───────── CoreAudio (macOS), AudioQueue (iOS), WASAPI (Windows), ring buffer
  overlay timeline ───── subtitle + danmaku composition
  renderer core ──────── color state, render graph, tone map, scaler policy
  Metal renderer ─────── zero-copy NV12/P010, HDR/EDR, subtitle/danmaku pass
  D3D11 renderer ─────── zero-copy D3D11VA, HDR10, subtitle/danmaku pass (Windows)
  wgpu renderer ──────── cross-platform video + danmaku rendering
  presenter runtime ──── ties player + renderer + audio + overlays
  C ABI ──────────────── 69 exported functions, two handle families
  Flutter plugin ─────── macOS + iOS + Windows native view embedding
```

## 原生依赖

`xtask` 会从固定上游源下载、构建并安装原生依赖到 `third_party/`。默认 profile 是 `lgpl`。

| 依赖 | 版本 | 作用 |
|------|------|------|
| FFmpeg | 7.1.1 | Demux、decode、audio resample、VideoToolbox |
| libass | 0.17.3 | ASS 字幕渲染 |
| FreeType | 2.13.3 | 字体栅格化（libass 依赖） |
| HarfBuzz | 10.4.0 | 文本 shaping（libass 依赖） |
| FriBidi | 1.0.16 | 双向文本处理（libass 依赖） |

所有依赖都静态链接。libass 及其依赖默认启用（`features = ["libass"]`）。

```sh
cargo run -p xtask -- deps build --all --profile lgpl
cargo run -p xtask -- deps status
```

## FFmpeg 集成

`erika_ffmpeg_sys` 在构建时通过 bindgen 生成底层绑定。`erika::ffmpeg` 提供安全的 Rust 封装：

- **Demuxer**：持有 `AVFormatContext`，可选使用来自 `MediaSource` 的 Rust 后端自定义 `AVIOContext`。支持流选择、引用计数 packet 和基于时间戳的 seek。
- **Decoder**：软件和 VideoToolbox 硬件后端。硬件帧保留 BT.2020/PQ 元数据，并携带 `CVPixelBufferRef` 供 Metal 零拷贝导入。
- **AudioResampler**：封装 `libswresample`，输出 interleaved f32 PCM（默认 48 kHz stereo）。
- **SubtitleDecoder**：解码内嵌文本和位图字幕流。

## 播放引擎

`PlaybackSession` 负责打开媒体、选择轨道、配置解码后端，并产出视频帧和 PCM 音频块。

`VideoPlaybackEngine` 增加时钟驱动播放：

- 播放、暂停、停止、seek、倍速控制、EOF 检测。
- `PlaybackClock`：带音频主时钟约束的 media-time 锚点（deadband 校正、逐帧有界调整、大漂移 snap）。
- `VideoFrameScheduler`：为解码后的视频帧决定 present / wait / drop。
- `DisplaySyncState`：携带残余帧时长误差的 vsync 量化器。

## 音频输出

- **macOS**：CoreAudio 输出，带 ring buffer 和 PTS 跟踪的 clock snapshot。presenter 会把输出快照回传给 player worker 做音频主时钟约束。
- **iOS**：AudioQueue 输出，使用同样的 ring buffer 和 clock snapshot 模型。
- Ring buffer：interleaved f32、容量可配、溢出丢最旧、支持音量控制。

## 字幕系统

- **Parsing**：SRT、WebVTT、ASS 时间线解析。支持内嵌和外部字幕轨，外部轨可在运行时增删。
- **libass renderer**：静态链接且默认启用。接收 ASS 脚本，调用 `ass_render_frame`，把 alpha plane 导入 Erika 的 overlay 系统。macOS 使用 CoreText 字体提供者；iOS 把 Erika 内嵌的 Droid Sans Fallback 注册为内存字体，避开应用不可访问的系统字体路径。
- **SubtitleRendererCore**：面向 renderer 的边界层，用 changed/unchanged frame 跟踪避免重复 GPU 上传。

## 弹幕系统

弹幕子系统用 Rust 原生实现了 NipaPlay DFM+ 的布局算法。完整设计见 `docs/danmaku_architecture.md`。

- **输入**：Bilibili XML、JSON、JSON-lines 解析。
- **DanmakuSession**：多轨管理，支持按轨启用/禁用、按轨 offset、全局 offset。
- **DFM+ layout core**：prepare / frame-query 分离。prepare 一次性处理整条轨道（测量、过滤、重复合并、碰撞避让、轨道分配），frame query 返回某一 media time 下的位置结果。
- **Text rasterizer**：带 fill/outline alpha mask 的 glyph atlas，并通过 version 跟踪 GPU 纹理复用。
- **Render plan**：`DanmakuRenderPlan` 携带 glyph instances，包含屏幕 rect、atlas tex rect、颜色、outline、shadow。Metal 和 wgpu 渲染器从 atlas 画实例化 quad。

## 渲染器

### Metal Renderer（macOS/iOS）

Apple 平台的主渲染器：

- 通过 `CVMetalTextureCache` 零拷贝导入 CVPixelBuffer → MTLTexture。
- YCbCr 采样、transfer decode、gamut mapping（BT.2020→BT.709、Display P3→BT.709）。
- Tone mapping：Mobius、Reinhard、clip，支持绝对 nits。
- SDR 输出（`BGRA8Unorm`）与 Apple EDR 输出（`RGBA16Float` + EDR headroom）。
- 神经亮度超分（`LumaUpscalerMode`）：ArtCNN C4F16/C4F32 2x doubler，以 Metal compute pass 跑在解码后的 Y plane 上，并与 render pass 使用同一 command buffer（`renderer/metal/upscaler.rs`）。色度保持原分辨率。仅在视频显示尺寸大于源分辨率时启用；网络输出会按解码帧缓存，重复 vsync tick 直接复用结果。权重来自上游 ONNX 发布（`assets/artcnn/`），并用 `tests/artcnn_upscaler.rs` 中的 onnxruntime 参考验证。提供 `simdgroup_matrix` matmul 后端（Apple Silicon 默认）和 scalar texture fallback；两者都在后台线程编译，编译完成前播放会先以未放大状态继续。
- 字幕 overlay：RGBA plane 上传与 alpha blending。
- 弹幕：来自 atlas 的 instanced glyph quad 绘制（shadow → outline → fill）。
- 呈现布局保持源宽高比。

### Direct3D 11 Renderer（Windows）

Windows 平台的原生渲染器（`renderer/d3d11.rs`）：

- 零拷贝 D3D11VA 解码纹理互操作：解码出的 `ID3D11Texture2D` 表面共享进渲染设备，不经过 CPU。
- YCbCr 采样与色彩空间转换（HLSL shader），与 Metal 保持同一流水线模型。
- HDR10 输出：`R10G10B10A2_UNORM` swapchain + `DXGI_HDR_METADATA_HDR10`，并提供 SDR（`BGRA8`）回退。
- 字幕 overlay alpha-atlas 上传与 blending；来自 atlas 的 instanced 弹幕 glyph quad 绘制。
- window-hosted swapchain，由 `render_tick` 驱动。

### wgpu Renderer（跨平台）

面向可移植性的第二渲染后端：

- 真正的 `wgpu` 依赖与设备/表面/pipeline 创建。
- NV12/P010 视频帧上传和 WGSL YCbCr 转换 shader。
- 色彩空间转换、tone mapping（与 Metal 保持同一流水线模型）。
- 弹幕 glyph atlas 渲染。
- 可用于无头测试的离屏 render target。
- 表面句柄模型覆盖 macOS NSView、iOS UIView、Windows HWND、X11/Wayland、Android native window。
- 目前尚未实现硬件零拷贝导入（VideoToolbox / D3D11VA）和 HDR/EDR 输出；这是面向 Linux 和 Android 的规划路径。

### Render Pipeline

`renderer::pipeline` 会在任何后端消费之前，先在 Rust 里描述渲染决策：

- `SourceColorState` / `TargetColorState`：primaries、transfer、range。
- `VideoRenderPipeline`：gamut matrix、tone map operator、transfer functions。
- HDR metadata：mastering display、content light level、nominal peak nits。

## Presenter Runtime

`PresenterRuntime` 把 Player、MetalRenderer、OverlayTimeline、DanmakuEngine 和音频输出串起来。宿主提供原生 surface，并从显示定时器驱动 `render_tick`。

- 推送视频帧，更新 overlay（字幕 + 弹幕），渲染并 present。
- 弹幕 plan 生成与视频帧通过 generation + media_time gate 保持同步。
- 运行时支持倍速、音量、轨道选择、字幕/弹幕配置。

## C ABI

`erika_capi` 通过两组 handle family 导出 69 个函数：

- **`ErikaHandle`**：播放器控制与事件轮询，渲染由宿主管理。
- **`ErikaPresenterHandle`**：Erika 持有完整栈，宿主只需提供 surface 并调用 `render_tick`。

覆盖范围包括：create/destroy、open/play/pause/stop/seek、轨道选择、字幕轨增删、弹幕轨管理（add/remove/enable/offset/config）、surface attach/detach/resize、事件轮询、音量、播放速率、神经亮度超分切换、超分后端状态诊断。

Header：`crates/erika_capi/include/erika.h`

## Flutter Plugin

`packages/erika_flutter` 提供 macOS、iOS 和 Windows 的 Flutter embedding：

- **Dart**：`ErikaPlayer`（命令 + 事件）、`ErikaWindowOverlayVideoView`（推荐的 window-hosted native surface——Apple 上是 Metal，Windows 上是 D3D11 swapchain）、`ErikaVideoView`（兼容 platform view）。
- **macOS Swift plugin**：加载 `liberika_capi.dylib`，创建 `NSWindow` overlay 或 `NSView`/`CAMetalLayer` platform view，并通过 display link 驱动 `render_tick`。
- **iOS Swift plugin**：静态链接 `liberika_capi.a`，创建 `UIWindow` overlay 或 `UIView`/`CAMetalLayer` platform view，并沿用同一 presenter 模型。
- **Windows C++ plugin**（`ErikaFlutterPluginCApi`）：通过 CMake（`build_erika_runtime.cmake`，cargo target `x86_64-pc-windows-msvc`）构建并链接 `erika_capi.dll`，host 一个 window-level D3D11 swapchain，并由帧调度器驱动 `render_tick`。

Embedding 模型和 HDR 策略见 `docs/flutter_embedding.md`。

## Platform Support

| Platform | Decode | Render | Audio | Status |
|----------|--------|--------|-------|--------|
| macOS 14+ | VideoToolbox | Metal | CoreAudio | Available |
| iOS 16+ | VideoToolbox | Metal | AudioQueue | Available |
| Windows 10+ | D3D11VA | Direct3D 11 | WASAPI | Available |
| Linux | — | wgpu (planned) | — | Planned |
| Android | — | wgpu (planned) | — | Planned |

