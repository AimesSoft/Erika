# erika_flutter

[中文](README.zh.md) | [English](README.md) | [日本語](README.ja.md)

Erika 媒体播放引擎的 Flutter plugin。

插件让 Dart 不进入热路径：

- Dart 只暴露低频播放器命令和事件流。
- 原生插件提供两种 surface：推荐的 `ErikaWindowOverlayVideoView`（macOS/iOS 为 Metal，Windows 为 D3D11 swapchain），以及兼容用的 `ErikaVideoView`。
- macOS 插件加载 Erika 动态库。
- iOS 插件链接 Erika 静态库。
- Windows 插件构建并链接 Erika C ABI DLL。
- Erika 通过 `ErikaPresenterHandle` 负责播放、渲染、音频、时序和 overlay。

## Video Surfaces

全播放器 macOS/iOS UI 推荐使用 `ErikaWindowOverlayVideoView`。它会在 Flutter 布局中预留矩形区域，同时插件在旁边托管一个原生 `CAMetalLayer`，让视频保持在 Flutter platform-view compositor 之外。

Windows 上 `ErikaWindowOverlayVideoView` 以 sibling surface 的形式托管一个 window-level Direct3D 11 swapchain，遵循同样的 overlay 模型。

需要标准 Flutter platform view 时则使用 `ErikaVideoView`。

## macOS Setup

本地开发时，macOS 插件通过 `dlopen` 加载 Erika。可设置 `ERIKA_CAPI_DYLIB` 覆盖动态库路径；若未设置，插件会按 app bundle、可执行文件目录、再到 `$WORKSPACE/target/debug/liberika_capi.dylib` 依次查找。

构建动态库：

```sh
cargo run -p xtask -- deps build --all --profile lgpl
cargo build -p erika_capi
```

## iOS Setup

iOS CocoaPod script phase 会在 Xcode 构建期间自动构建 Erika 原生依赖和 C ABI static library。需要安装对应 iOS target 的 Rust toolchain：

- `rustup target add aarch64-apple-ios`

## Windows Setup

Windows 插件（`ErikaFlutterPluginCApi`）在 CMake 构建期间通过 `build_erika_runtime.cmake` 构建 Erika C ABI runtime（`erika_capi.dll`），对 `x86_64-pc-windows-msvc` target 调用 cargo，并把 DLL 部署到 app 旁边。需要：

- 安装 MSVC target 的 Rust toolchain（`rustup target add x86_64-pc-windows-msvc`）
- Visual Studio Build Tools (MSVC) + Windows SDK
- 原生依赖已构建到 `third_party/dist/x86_64-pc-windows-msvc/`（见仓库的 `xtask deps build` 流程）

若插件无法自动定位 Erika checkout，可设置 `ERIKA_REPO_ROOT`。

## Output Mode

`ErikaPlayer()` 会让 macOS 插件根据当前屏幕和环境选择 SDR 或 Apple EDR。若要从 Dart 强制 EDR：

```dart
final player = ErikaPlayer(
  outputMode: ErikaOutputMode.appleEdr,
  edrHeadroom: 4.0,
);
```

使用 `ErikaOutputMode.sdr` 可强制 SDR 输出。

## Upscaler

创建播放器后即可从 Dart 启用 ArtCNN 超分：

```dart
await player.setUpscaler(ErikaUpscalerMode.artCnnC4F16);
```

使用 `ErikaUpscalerMode.off` 关闭。`player.getUpscalerStatus()` 会返回请求模式、当前后端、fallback 次数、超分帧数和最近 GPU timing。

