[English](readme/README.en.md) | [日本語](readme/README.ja.md)

# Erika

> 「GOOD！我是Erika，是NipaPlay里继mdk、video player、libmpv、media kit之后的第五个播放器内核。」
> 「即便算上你，也只有四个播放器内核！」

**NipaPlay 的自研播放内核。** Rust 实现，可嵌入，从解码到渲染一手包办。

> 名字取自《海猫鸣泣之时》的侦探 **古戸ヱリカ**。
> 而 [NipaPlay](https://github.com/AimesSoft/NipaPlay-Reload) 来自《寒蝉鸣泣之时》古手梨花的口癖「にぱー☆」——社区里大家都叫她「梨花」。
> 一个是台前的播放器，一个是幕后的引擎。同出一脉，互为表里。

宿主应用只需提供一个渲染表面并发送播放命令——解码、时序同步、音视频渲染、字幕、弹幕、音频输出均由 Erika 内部完成。原生视频层直接呈现；Flutter 纹理可交给宿主合成。

当前版本：**0.2.1**。从 [文档目录](docs/README.md) 选择接入、构建或内核设计说明。

## 能力

- 硬件解码与软件回退：VideoToolbox、D3D11VA、MediaCodec、AVCodec；Linux 提供 NVDEC / VA-API。
- Metal、Direct3D 11 与 wgpu 呈现，合成视频、字幕和弹幕；支持 HDR/EDR、Dolby Vision 映射与 ArtCNN 亮度超分。
- 本地文件与 HTTP(S) 播放，支持自定义请求头、后台预读和回退缓存。
- 音频主时钟同步、暂停、seek、倍速、多音轨、外挂字幕和多轨弹幕。
- Rust、C ABI、Flutter、SwiftPM 与 OpenHarmony ArkTS 接入。

## 选择接入方式

| 宿主 | 安装或入口 | 指南 |
|---|---|---|
| Flutter | `flutter pub add erika_flutter`，当前 0.2.1 | [插件使用指南](packages/erika_flutter/README.zh.md) |
| Swift / Apple | Xcode 添加 `https://github.com/AimesSoft/ErikaSwift`，从 0.2.1 开始 | [ErikaSwift](https://github.com/AimesSoft/ErikaSwift) |
| OpenHarmony / ArkTS | `ohpm install erika` | [ArkTS SDK](packages/erika_ohos/README.md) |
| C / C++ | 下载匹配版本的 C ABI 归档与 `erika.h` | [原生接入](docs/integration.zh.md) |
| Rust | Git 依赖，固定 `tag = "v0.2.1"` | [构建与依赖](docs/building.zh.md) |
| Linux | 系统 FFmpeg 8 + Erika 补丁 libass，从源码构建 | [Linux 接入](docs/linux.zh.md) |

截至 2026-10-02，OHPM 公开版为 0.1.9，0.2.1 已提交审核。原生二进制从
[GitHub Releases](https://github.com/AimesSoft/Erika/releases/tag/v0.2.1) 下载。

### Flutter

```dart
import 'package:erika_flutter/erika_flutter.dart';

final player = ErikaPlayer();
await player.open('/path/to/video.mp4');
await player.play();

// 放入 Widget 树；Apple 全播放器可选 ErikaWindowOverlayVideoView。
ErikaVideoView(player: player)
```

`ErikaTextureVideoView` 适合需要 Flutter 裁剪和颜色滤镜的视频；原生视频层适合
Apple EDR、Windows HDR10 等直接呈现。平台差异和生命周期在插件指南中说明。

### Rust

```toml
[dependencies]
erika = { git = "https://github.com/AimesSoft/Erika", tag = "v0.2.1" }
```

```rust
use erika::{MediaRequest, Player, PlayerConfig};

let player = Player::new(PlayerConfig::default());
player.open(MediaRequest::new("/path/to/video.mp4"))?;
player.play()?;
```

从源码接入前先准备对应平台的原生依赖。`Player` 用于控制和帧订阅；
`PresenterRuntime` 托管渲染与音频。C ABI 对应入口为 `ErikaHandle` 和
`ErikaPresenterHandle`，完整声明位于 [erika.h](crates/erika_capi/include/erika.h)。

## 平台

| 平台 | 解码 | 渲染 | 音频 | 分发 |
|---|---|---|---|---|
| macOS 11+ | VideoToolbox / 软解 | Metal | CoreAudio | 预编译、Flutter、SwiftPM |
| iOS 13+ | VideoToolbox / 软解 | Metal | AudioQueue | XCFramework、Flutter、SwiftPM |
| tvOS 13+ | VideoToolbox / 软解 | Metal | AudioQueue | XCFramework、Flutter、SwiftPM |
| Windows 10+ | D3D11VA / 软解 | D3D11 | WASAPI | x64 / ARM64 预编译、Flutter |
| Android 8+ | MediaCodec / 软解 | wgpu Vulkan / GLES | AAudio | 四个 ABI 预编译、Flutter |
| OpenHarmony API 18+ | AVCodec / 软解 | wgpu | OHAudio | arm64 预编译、Flutter、OHPM |
| Linux（实验性） | NVDEC / VA-API / 软解 | wgpu X11 / Wayland | PulseAudio / pipewire-pulse | Rust、C ABI、Flutter 源码构建 |

各平台的视频表面和 HDR 路径在 [平台矩阵](docs/platform_matrix.zh.md) 中汇总。
Web 尚无播放后端。

## 开发

```sh
# Apple / Windows / Android / OpenHarmony：构建固定版本的原生依赖。
cargo run -p xtask -- deps build --all --profile lgpl
cargo build -p erika_capi
```

交叉编译需设置目标 triple；Linux 使用独立的系统库构建入口。
构建参数在 [构建指南](docs/building.zh.md)，发布流程在 [发布指南](docs/releasing.zh.md)，
代码约定在 [贡献指南](CONTRIBUTING.zh.md)。

## 仓库结构

| 目录 | 内容 |
|---|---|
| `crates/erika` | 播放、解码、时钟、字幕、弹幕与渲染 |
| `crates/erika_capi` | C ABI 和公开头文件 |
| `crates/erika_ffmpeg_sys` | FFmpeg bindings |
| `packages/erika_flutter` | Flutter 插件 |
| `packages/erika_ohos` | OpenHarmony ArkTS SDK |
| `examples` | 原生、Flutter 与 ArkTS 示例 |
| `xtask` | 原生依赖构建 |
| `docs` | 接入与设计文档；`investigations/` 保存历史调查和验证记录 |

版本变化见 [CHANGELOG](CHANGELOG.md)。

## 许可证

Rust workspace 使用 [MPL-2.0](LICENSE)。原生依赖的构建 profile 与许可说明见
[构建指南](docs/building.zh.md#许可证-profile) 和 [第三方声明](packaging/THIRD_PARTY_NOTICES.md)。
