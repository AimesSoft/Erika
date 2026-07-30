[English](readme/README.en.md) | [日本語](readme/README.ja.md)

# Erika

> 「GOOD！我是Erika，是NipaPlay里继mdk、video player、libmpv、media kit之后的第五个播放器内核。」
> 「即便算上你，也只有四个播放器内核！」

**NipaPlay 的自研播放内核。** Rust 实现，可嵌入，从解码到渲染一手包办。

> 名字取自《海猫鸣泣之时》的侦探 **古戸ヱリカ**。
> 而 [NipaPlay](https://github.com/AimesSoft/NipaPlay-Reload) 来自《寒蝉鸣泣之时》古手梨花的口癖「にぱー☆」——社区里大家都叫她「梨花」。
> 一个是台前的播放器，一个是幕后的引擎。同出一脉，互为表里。

宿主应用只需提供一个渲染表面并发送播放命令——解码、时序同步、音视频渲染、字幕、弹幕、音频输出均由 Erika 内部完成，不经过宿主的渲染管线。

## 特性

- **硬件加速解码** — VideoToolbox (macOS/iOS)、D3D11VA (Windows)、MediaCodec (Android)、AVCodec (HarmonyOS)，互操作不可用时明确回退软解
- **零拷贝渲染** — Apple CVPixelBuffer → MTLTexture、Windows D3D11VA 纹理互操作、Android MediaCodec Surface → AHardwareBuffer/Vulkan、HarmonyOS AVCodec Surface → OHNativeBuffer/Vulkan；无法导入时明确回退 CPU upload
- **HDR/EDR 输出** — Apple EDR、Windows HDR10，以及 Android FP16 extended-linear scRGB 协商与明确 SDR 回退
- **原生 Metal 渲染器** — YCbCr 采样、色彩空间转换、tone mapping、字幕/弹幕合成，一次 render pass 完成 (macOS/iOS)
- **原生 Direct3D 11 渲染器** — Windows: D3D11VA 零拷贝纹理互操作、YCbCr 采样、HDR10 输出、字幕/弹幕 overlay 合成
- **AI 超分** — ArtCNN 动漫亮度 2x 神经超分，Metal 与 wgpu/Vulkan compute 算子，仅处理亮度并接入渲染管线
- **音频输出** — CoreAudio (macOS) / AudioQueue (iOS) / WASAPI (Windows) / AAudio (Android) / OHAudio (HarmonyOS)，f32 PCM ring buffer，音频时钟同步
- **字幕** — SRT / WebVTT / ASS 解析，libass 渲染 (静态链接)，嵌入与外挂字幕轨
- **弹幕** — Bilibili XML / JSON 解析，DFM+ 碰撞避让布局引擎，glyph atlas 原生 GPU 渲染
- **播放引擎** — play / pause / stop / seek / 倍速，音频主时钟同步，vsync 量化调度
- **C ABI** — 79 个导出函数，opaque handle 设计，可从 C / C++ / Swift / Dart FFI / 任何 FFI 语言调用
- **Flutter 插件** — macOS + iOS + Windows + Android + HarmonyOS 原生视图/Texture 嵌入
- **wgpu 后端** — Android 播放、overlay、截图与 Vulkan/GLES 恢复路径可用；HarmonyOS 走 Vulkan，用 OHNativeWindow 呈现、OHNativeBuffer 零拷贝导入；Linux 仍在规划中

## 快速开始

### Rust

```rust
use erika::{Player, PlayerConfig, MediaRequest};

let player = Player::new(PlayerConfig::default())?;
player.open(MediaRequest::file("/path/to/video.mp4"))?;
player.play()?;
```

### C ABI

```c
#include "erika.h"

ErikaPresenterHandle *presenter = erika_presenter_create();
erika_presenter_attach_metal_layer(presenter, (uint64_t)layer, w, h, scale);
erika_presenter_open(presenter, "/path/to/video.mp4");
erika_presenter_play(presenter);

// 每个显示帧回调:
ErikaPresenterStats stats;
erika_presenter_render_tick(presenter, host_time, &stats);
```

### Flutter

```dart
final player = ErikaPlayer();
await player.open('/path/to/video.mp4');
await player.play();

// 推荐：完整播放器 UI 在 macOS/iOS 使用原生 window overlay / 挖空路径
ErikaWindowOverlayVideoView(player: player)

// 兼容/诊断：Flutter platform view 路径
ErikaVideoView(player: player)
```

## C ABI 接口族

Erika 提供两组 C ABI 入口，适配不同嵌入场景：

| 接口族 | 适用场景 | 渲染方式 |
|--------|----------|----------|
| `ErikaHandle` | 宿主自己管理渲染循环 | 宿主拉取帧数据 |
| `ErikaPresenterHandle` | Erika 托管完整播放栈 | 宿主只需提供 surface 并驱动 `render_tick` |

头文件: [`crates/erika_capi/include/erika.h`](crates/erika_capi/include/erika.h)

## 平台支持

| 平台 | 解码 | 渲染 | 音频 | 状态 |
|------|------|------|------|------|
| macOS 14+ | VideoToolbox | Metal | CoreAudio | **可用** |
| iOS 16+ | VideoToolbox | Metal | AudioQueue | **可用** |
| Windows 10+ | D3D11VA | Direct3D 11 | WASAPI | **可用** |
| Linux | — | wgpu (planned) | — | 规划中 |
| Android 8+ | MediaCodec / software | wgpu (Vulkan + GLES fallback) | AAudio | **可用** |
| HarmonyOS API 18+ | AVCodec（H.264/HEVC）/ 软解 | wgpu (Vulkan) + `OHNativeBuffer` 零拷贝导入 | OHAudio | **可用** |

## 仓库结构

```
crates/erika              核心播放库
crates/erika_capi         C ABI 导出层
crates/erika_ffmpeg_sys   FFmpeg 底层 bindings
packages/erika_flutter    Flutter 插件 (macOS + iOS + Windows + Android + HarmonyOS)
examples/                 验证与演示程序
xtask/                    原生依赖构建编排
docs/                     架构与嵌入文档
```

## 文档

- [架构总览](docs/architecture.zh.md) — 引擎设计、渲染后端、平台支持
- [C ABI 参考手册](docs/capi_reference.zh.md) — 全部导出函数、状态码、所有权与线程约定
- [原生接入指南](docs/integration.zh.md) — C/C++/Win32/Swift 等非 Flutter 宿主的端到端嵌入
- [构建与依赖指南](docs/building.zh.md) — xtask、native 依赖、交叉编译
- [Flutter 嵌入](docs/flutter_embedding.zh.md) ・ [弹幕架构](docs/danmaku_architecture.md)
- [发布与预编译产物](docs/releasing.md) — 各平台预编译 `erika_capi` 库下载与打包(英文)
- [贡献 / 开发者指南](CONTRIBUTING.zh.md) — 仓库布局、线程模型、新增平台后端

## 构建

### 前置依赖

- Rust 1.92+
- Xcode Command Line Tools (macOS/iOS)
- MSVC 工具链 + Windows SDK (Windows，target `x86_64-pc-windows-msvc`)
- Android SDK + NDK r29，以及对应 Android Rust target
- DevEco Studio OpenHarmony Native SDK，以及 Rust `aarch64-unknown-linux-ohos` target
- CMake, pkg-config

### 构建原生依赖

```sh
# 构建 FFmpeg (LGPL profile)
cargo run -p xtask -- deps build --profile lgpl

# 构建全部依赖 (含 libass/FreeType/HarfBuzz/FriBidi)
cargo run -p xtask -- deps build --all --profile lgpl

# 查看依赖状态
cargo run -p xtask -- deps status
```

### 编译与测试

```sh
cargo build -p erika
cargo test --workspace
```

### 验证播放路径

```sh
# macOS
export SAMPLE="/path/to/video.mp4"
cargo run -p macos_native_demo -- "$SAMPLE"
cargo run -p macos_native_demo -- --smoke-seconds 3 "$SAMPLE"

# Windows
cargo run -p windows_native_demo -- "%SAMPLE%"
```

## 许可证

Rust workspace: [MPL-2.0](LICENSE)

原生依赖通过 `xtask` 独立管理构建 profile 和许可证边界。
