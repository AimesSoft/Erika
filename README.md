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

- **硬件加速解码** — VideoToolbox (macOS/iOS)，可扩展至其他平台后端
- **零拷贝渲染** — CVPixelBuffer → MTLTexture 直通，视频帧不经过 CPU 内存
- **HDR/EDR 输出** — Apple EDR 原生支持，PQ (BT.2020) 元数据保留与 tone mapping
- **原生 Metal 渲染器** — YCbCr 采样、色彩空间转换、tone mapping、字幕/弹幕合成，一次 render pass 完成
- **AI 超分** — ArtCNN 动漫亮度 2x 神经超分，Metal compute 算子，simdgroup_matrix / 标量双后端，仅处理 Y 平面、零拷贝接入渲染管线
- **音频输出** — CoreAudio (macOS) / AudioQueue (iOS)，f32 PCM ring buffer，音频时钟同步
- **字幕** — SRT / WebVTT / ASS 解析，libass 渲染 (静态链接)，嵌入与外挂字幕轨
- **弹幕** — Bilibili XML / JSON 解析，DFM+ 碰撞避让布局引擎，glyph atlas 原生 GPU 渲染
- **播放引擎** — play / pause / stop / seek / 倍速，音频主时钟同步，vsync 量化调度
- **C ABI** — 63 个导出函数，opaque handle 设计，可从 C / C++ / Swift / Dart FFI / 任何 FFI 语言调用
- **Flutter 插件** — macOS + iOS 原生视图嵌入，支持 HDR native layer 路径
- **wgpu 后端** — 跨平台渲染基础就绪 (Windows / Linux / Android 方向)

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

// Widget 树中:
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
| Windows | — | wgpu (planned) | — | 规划中 |
| Linux | — | wgpu (planned) | — | 规划中 |
| Android | — | wgpu (planned) | — | 规划中 |

## 仓库结构

```
crates/erika              核心播放库
crates/erika_capi         C ABI 导出层
crates/erika_ffmpeg_sys   FFmpeg 底层 bindings
packages/erika_flutter    Flutter 插件 (macOS + iOS)
examples/                 验证与演示程序
xtask/                    原生依赖构建编排
docs/                     架构与嵌入文档
```

## 构建

### 前置依赖

- Rust 1.92+
- Xcode Command Line Tools (macOS/iOS)
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
export SAMPLE="/path/to/video.mp4"
cargo run -p macos_native_demo -- "$SAMPLE"
cargo run -p macos_native_demo -- --smoke-seconds 3 "$SAMPLE"
```

## 许可证

Rust workspace: [MPL-2.0](LICENSE)

原生依赖通过 `xtask` 独立管理构建 profile 和许可证边界。