# erika_flutter

[中文](README.zh.md) | [English](README.md) | [日本語](README.ja.md)

Erika 媒体播放引擎的 Flutter plugin。

插件让 Dart 不进入热路径：

- Dart 只暴露低频播放器命令和事件流。
- 原生插件提供两种 surface：推荐的 `ErikaWindowOverlayVideoView`（macOS/iOS 为 Metal，Windows 为 D3D11 swapchain），以及 platform view 用的 `ErikaVideoView`。Android 上两者都通过同一套原生 view 选择器：SDR 使用真实 `TextureView`，请求 extended-linear 时使用 Hybrid Composition `SurfaceView`。
- macOS 插件加载 Erika 动态库。
- iOS 插件链接 Erika 静态库。
- Windows 插件构建并链接 Erika C ABI DLL。
- Android 插件按 ABI 构建 `liberika_capi.so`，并由 `Choreographer` 驱动原生 surface。
- Erika 通过 `ErikaPresenterHandle` 负责播放、渲染、音频、时序和 overlay。

## Video Surfaces

全播放器 macOS/iOS UI 推荐使用 `ErikaWindowOverlayVideoView`。它会在 Flutter 布局中预留矩形区域，同时插件在旁边托管一个原生 `CAMetalLayer`，让视频保持在 Flutter platform-view compositor 之外。

Windows 上 `ErikaWindowOverlayVideoView` 以 sibling surface 的形式托管一个 window-level Direct3D 11 swapchain，遵循同样的 overlay 模型。

需要标准 Flutter platform view 时则使用 `ErikaVideoView`。Android 的 SDR 视频 surface 是原生 `TextureView`；`ErikaOutputMode.extendedLinear` player 则通过 `PlatformViewLink`/Hybrid Composition 创建 `SurfaceView`，因为 scRGB 不能经过 Flutter texture-layer composition。插件把借用的 `Surface` 交给 Erika，并完整处理创建、resize、销毁、音频焦点、HDR eligibility 和 vsync tick。

## macOS Setup

本地开发时，macOS 插件通过 `dlopen` 加载 Erika。可设置 `ERIKA_CAPI_DYLIB` 覆盖动态库路径；若未设置，插件会按 app bundle、可执行文件目录、再到 `$WORKSPACE/target/debug/liberika_capi.dylib` 依次查找。

构建动态库：

```sh
cargo run -p xtask -- deps build --all --profile lgpl
cargo build -p erika_capi
```

macOS CocoaPod 默认构建 universal 动态库（arm64 + x86_64），并使用 `lipo` 合并两个 Rust target。可通过 `ERIKA_MACOS_ARCH` 选择构建架构：

```sh
ERIKA_MACOS_ARCH=arm64 flutter build macos
ERIKA_MACOS_ARCH=x86_64 flutter build macos
ERIKA_MACOS_ARCH=universal flutter build macos
```

可选值为 `arm64`、`x86_64` 和 `universal`，默认值是 `universal`。选择单一架构时只构建对应的 Rust target，不执行 `lipo`；其他值会使 CocoaPod 构建明确失败。

## iOS Setup

iOS CocoaPod script phase 会在 Xcode 构建期间自动构建 Erika 原生依赖和 C ABI static library。需要安装对应 iOS target 的 Rust toolchain：

- `rustup target add aarch64-apple-ios`

## Windows Setup

Windows 插件（`ErikaFlutterPluginCApi`）在 CMake 构建期间通过 `build_erika_runtime.cmake` 构建 Erika C ABI runtime（`erika_capi.dll`），对 `x86_64-pc-windows-msvc` target 调用 cargo，并把 DLL 部署到 app 旁边。需要：

- 安装 MSVC target 的 Rust toolchain（`rustup target add x86_64-pc-windows-msvc`）
- Visual Studio Build Tools (MSVC) + Windows SDK
- 原生依赖已构建到 `third_party/dist/x86_64-pc-windows-msvc/`（见仓库的 `xtask deps build` 流程）

若插件无法自动定位 Erika checkout，可设置 `ERIKA_REPO_ROOT`。

## Android Setup

Android Gradle 构建会先调用 Erika 的 `xtask` 构建原生依赖，再用 Cargo 为选定 ABI 构建 `erika_capi`。需要 Android API 26 或更高版本，并安装 Android NDK 和对应 Rust target。生成的 `jniLibs` 会同时包含 `liberika_capi.so` 与匹配 ABI 的 NDK `libc++_shared.so`。默认构建 arm64 与 x86_64；可通过 `-PerikaAndroidAbis=arm64-v8a,x86_64` 或 `ERIKA_ANDROID_ABIS` 指定。

Android `content://` 媒体和字幕 URI 会通过 `ContentResolver` 打开并 detach，连同 provider 的 offset/length 作为由 Rust 接管所有权的 `fd://` source 传入 Erika。

Android 最低版本仍为 API 26。Extended-linear 还要求 native-window dataspace API（API
28+）；API 26/27 会继续 SDR 播放并报告对应 fallback。API 34+ 上，插件会监听
`Display.registerHdrSdrRatioChangedListener`，把真实 ratio 变化发布给 Erika，让 wgpu 无需
重新 attach surface 就能更新后续帧 target 和输出状态。API 35 上插件还会按
`SurfaceView` 设置 desired HDR headroom，不修改宿主的全局 Window。

## Output Mode

## HTTP 请求头

播放 HTTP(S) 视频时，可以通过 `httpHeaders` 传递请求头：

```dart
await player.open(
  'https://example.com/video.mp4',
  httpHeaders: <String, String>{
    'Authorization': 'Bearer token',
    'Referer': 'https://example.com/',
  },
);
```

请求头会随 HEAD、Range GET 和预取请求发送，仅对 HTTP(S) URL 生效。`Range` 由播放引擎
管理，不应在 `httpHeaders` 中覆盖；`content://` 和本地文件播放不使用这些请求头。请避免
在应用日志中输出 Authorization、Cookie 等敏感值。

`ErikaPlayer()` 会让 Apple 插件根据当前屏幕和环境选择 SDR 或 Apple EDR；Android 默认
为 SDR。若要从 Dart 强制 Apple EDR：

```dart
final player = ErikaPlayer(
  outputMode: ErikaOutputMode.appleEdr,
  edrHeadroom: 4.0,
);
```

使用 `ErikaOutputMode.sdr` 可强制 SDR 输出。

Android 的高 headroom 模式是 FP16 **extended-linear scRGB**，不是 HDR10/PQ：

```dart
final player = ErikaPlayer(
  outputMode: ErikaOutputMode.extendedLinear,
  edrHeadroom: 4.0,
);
```

`edrHeadroom` 是内容 headroom 上限。Extended-linear player 未传该参数时，Erika 使用默认
4x 内容上限，同时给 `SurfaceView` 传 desired headroom `0`（系统 auto）。显式值在 API 35
上还会作为 per-`SurfaceView` desired headroom。显示器当前 HDR/SDR ratio 可用时，会进一步
约束 wgpu 的有效 target。

只有显示器/surface 支持 HDR、view 是 Hybrid Composition `SurfaceView`、wgpu 选择
Vulkan、surface 暴露 `Rgba16Float`，且配置后的 native window 回读为
`ADATASPACE_SCRGB_LINEAR`（`406913024`、`0x18410000`）时，该模式才会激活。GLES、
`TextureView`、缺少 FP16 或 dataspace 验证失败都会明确回退 SDR。Android scRGB 使用
BT.709 primaries，`1.0 = 80 nit`；不使用 PQ 或 HDR10 metadata。

始终查询协商结果，不要把请求值当成实际输出：

```dart
final status = await player.getOutputStatus();
if (!status.extendedLinearActive) {
  debugPrint(
    'Erika output fallback: '
    '${status.fallbackReason.label} (${status.fallbackReason.nativeValue})',
  );
}
```

`ErikaOutputStatus` 有 13 个字段：`requestedMode`、`activeEncoding`、
`surfaceFormat`、`nativeDataSpace`、`requestedHeadroom`、`activeHeadroom`、
`activeHeadroomKnown`、`extendedLinearActive`、`fallbackReason`、
`fallbackCount`、`dataSpaceFailures`、`headroomUpdates`、
`extendedLinearFrames`。Android scRGB 真正激活时为
`androidExtendedLinearScRgb + sixteenBitFloat + nativeDataSpace 406913024`。
API 34+ 上，Android 暴露有效 ratio 时，`activeHeadroom` 是当前显示器 HDR/SDR ratio，
`activeHeadroomKnown` 为 true；ratio 不可用时，该值只是 fallback，
`activeHeadroomKnown` 为 false。只有 known 状态或 ratio 真实变化时，`headroomUpdates`
才增长；重复 listener 通知会被忽略。

`ErikaOutputFallbackReason` 是稳定 ABI 数值：

| 码 | Dart 值 | 稳定 label |
|----|---------|------------|
| 0 | `none` | `none` |
| 1 | `displayHdrUnsupported` | `display_hdr_unsupported` |
| 2 | `hybridCompositionRequired` | `hybrid_composition_required` |
| 3 | `wgpuBackendNotVulkan` | `wgpu_backend_not_vulkan` |
| 4 | `rgba16FloatSurfaceFormatUnavailable` | `rgba16float_surface_format_unavailable` |
| 5 | `nativeWindowDataSpaceApiUnavailable` | `native_window_dataspace_api_unavailable` |
| 6 | `scrgbDataSpaceVerificationFailed` | `scrgb_dataspace_verification_failed` |
| 7 | `surfaceConfigureFailed` | `surface_configure_failed` |
| 8 | `legacyAppleEdrUnsupported` | `legacy_apple_edr_unsupported` |

`player.screenshot()` 返回当前合成帧（视频 + 字幕 + 弹幕）的原始 SDR RGBA8；即使显示为
Apple EDR 或 Android extended-linear 也一样。Metal 与 Android/wgpu 已实现截图；当前
Windows D3D11 Flutter 路径不返回截图字节。

非 HDR 模拟器/设备覆盖验证的是明确 SDR 回退及其 reason。Active extended-linear 尚不宣称
已通过真机验证；仍需在 API 35 HDR 真机上验收 `Rgba16Float + SCRGB_LINEAR`、旋转/前后台
恢复、动态 HDR/SDR ratio 更新、多 player 和 SDR 截图。

## Upscaler

可以在创建时选择 ArtCNN，也可以在运行时切换：

```dart
final player = ErikaPlayer(upscaler: ErikaUpscalerMode.artCnnC4F16);
await player.setUpscaler(ErikaUpscalerMode.artCnnC4F16);
```

使用 `ErikaUpscalerMode.off` 关闭。`player.getUpscalerStatus()` 会返回请求模式、当前后端、fallback 次数、超分帧数和最近 GPU timing。Apple 使用 Metal；Android 对 planar 与 MediaCodec Surface 帧都使用 wgpu/Vulkan compute。GLES 3.0 会保持普通播放，并明确报告 `inactive` 回退。
