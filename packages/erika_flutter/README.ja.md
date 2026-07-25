# erika_flutter

[中文](README.zh.md) | [English](README.md) | [日本語](README.ja.md)

Erika メディア再生エンジン向けの Flutter plugin です。

この plugin は Dart を hot path から外します。

- Dart は低頻度の player command と event stream だけを公開します。
- native plugin は 2 種類の surface を提供します。推奨は `ErikaWindowOverlayVideoView`（macOS/iOS は Metal、Windows は D3D11 swapchain）、platform view 用は `ErikaVideoView` です。Android では両方が同じ native-view selector を使い、SDR は実体のある `TextureView`、extended-linear request は Hybrid Composition `SurfaceView` になります。
- macOS plugin は Erika の dynamic library を読み込みます。
- iOS plugin は Erika の static library を link します。
- Windows plugin は Erika C ABI DLL を build して link します。
- Android plugin は ABI ごとに `liberika_capi.so` を build し、`Choreographer` から native surface を駆動します。
- Erika は `ErikaPresenterHandle` を通じて playback、rendering、audio、timing、overlay を担当します。

## Video Surfaces

フルプレイヤーの macOS/iOS UI では `ErikaWindowOverlayVideoView` を使うのが推奨です。Flutter の layout では矩形領域を予約しつつ、plugin が横に native `CAMetalLayer` を持ち、video を Flutter platform-view compositor の外に置きます。

Windows では `ErikaWindowOverlayVideoView` が window-level の Direct3D 11 swapchain を sibling surface として host し、同じ overlay モデルに従います。

標準的な Flutter platform view が必要な場合は `ErikaVideoView` を使います。Android の SDR video surface は native `TextureView` です。`ErikaOutputMode.extendedLinear` player は `PlatformViewLink`/Hybrid Composition の `SurfaceView` を作ります。scRGB を Flutter texture-layer composition に通さないためです。plugin は borrowed `Surface`、lifecycle、resize、audio focus、HDR eligibility、vsync tick を Erika に接続します。

## macOS Setup

ローカル開発では macOS plugin が `dlopen` で Erika を読み込みます。`ERIKA_CAPI_DYLIB` で dynamic library path を上書きできます。未設定時は app bundle、実行ファイルディレクトリ、`$WORKSPACE/target/debug/liberika_capi.dylib` の順で探します。

dynamic library を build するには：

```sh
cargo run -p xtask -- deps build --all --profile lgpl
cargo build -p erika_capi
```

## iOS Setup

iOS の CocoaPod script phase が、Xcode build 中に Erika の native dependency と C ABI static library を自動 build します。対応する iOS target の Rust toolchain が必要です。

- `rustup target add aarch64-apple-ios`

## Windows Setup

Windows plugin（`ErikaFlutterPluginCApi`）は CMake build 中に `build_erika_runtime.cmake` で Erika C ABI runtime（`erika_capi.dll`）を build し、`x86_64-pc-windows-msvc` target に対して cargo を呼び出し、DLL を app の隣に配置します。必要なもの：

- MSVC target の Rust toolchain（`rustup target add x86_64-pc-windows-msvc`）
- Visual Studio Build Tools (MSVC) + Windows SDK
- `third_party/dist/x86_64-pc-windows-msvc/` に build 済みの native dependency（リポジトリの `xtask deps build` フロー）

plugin が Erika checkout を自動検出できない場合は `ERIKA_REPO_ROOT` を設定してください。

## Android Setup

Android Gradle build は Erika の `xtask` で native dependency を構築し、選択した ABI 向けに Cargo で `erika_capi` を build します。Android API 26 以降、Android NDK、対応する Rust target が必要です。生成される `jniLibs` には `liberika_capi.so` と ABI に対応する NDK の `libc++_shared.so` が含まれます。既定は arm64 と x86_64 で、`-PerikaAndroidAbis=arm64-v8a,x86_64` または `ERIKA_ANDROID_ABIS` で変更できます。

Android の `content://` media/subtitle URI は `ContentResolver` で開いて detach し、provider の offset/length を含む所有権付き `fd://` source として Erika に渡します。

Android minimum は API 26 のままです。Extended-linear は native-window dataspace API
（API 28+）も必要で、API 26/27 は SDR playback を継続して該当 fallback を報告します。
API 34+ では plugin が `Display.registerHdrSdrRatioChangedListener` を監視し、実際の ratio
change を Erika に publish します。wgpu は surface を reattach せず後続 frame target と
output status を更新します。API 35 では host の global Window を変更せず、`SurfaceView`
ごとに desired HDR headroom も設定します。

## HTTP ヘッダー

HTTP(S) video を再生する場合は、`httpHeaders` で request header を渡せます：

```dart
await player.open(
  'https://example.com/video.mp4',
  httpHeaders: <String, String>{
    'Authorization': 'Bearer token',
    'Referer': 'https://example.com/',
  },
);
```

header は HEAD、Range GET、prefetch request とともに送信され、HTTP(S) URL にだけ適用されます。
`Range` は playback engine が管理するため、`httpHeaders` で上書きしないでください。
`content://` と local file の再生では header は無視されます。Authorization や Cookie などの
機密値を application log に出力しないでください。

## Output Mode

`ErikaPlayer()` は Apple plugin に現在の screen と environment から SDR か Apple EDR を
選ばせ、Android は SDR が default です。Dart から Apple EDR を強制するには：

```dart
final player = ErikaPlayer(
  outputMode: ErikaOutputMode.appleEdr,
  edrHeadroom: 4.0,
);
```

`ErikaOutputMode.sdr` で SDR 出力を強制できます。

Android の high-headroom mode は FP16 **extended-linear scRGB** で、HDR10/PQ ではありません。

```dart
final player = ErikaPlayer(
  outputMode: ErikaOutputMode.extendedLinear,
  edrHeadroom: 4.0,
);
```

`edrHeadroom` は content-headroom ceiling です。extended-linear player で省略すると Erika
は default 4x content ceiling を使い、`SurfaceView` の desired headroom は `0`（system auto）
になります。明示値は API 35 の per-`SurfaceView` desired headroom にも使います。current
display HDR/SDR ratio が available なら wgpu effective target をさらに制限します。

display/surface が HDR capable、view が Hybrid Composition `SurfaceView`、wgpu が Vulkan、
surface が `Rgba16Float` を公開し、configured native window の readback が
`ADATASPACE_SCRGB_LINEAR`（`406913024`、`0x18410000`）の場合だけ active になります。
GLES、`TextureView`、FP16 不在、dataspace verification failure は SDR に明示 fallback します。
Android scRGB は BT.709 primaries、`1.0 = 80 nit` で、PQ/HDR10 metadata は使いません。

request ではなく negotiated state を必ず確認します。

```dart
final status = await player.getOutputStatus();
if (!status.extendedLinearActive) {
  debugPrint(
    'Erika output fallback: '
    '${status.fallbackReason.label} (${status.fallbackReason.nativeValue})',
  );
}
```

`ErikaOutputStatus` の 13 field は `requestedMode`、`activeEncoding`、
`surfaceFormat`、`nativeDataSpace`、`requestedHeadroom`、`activeHeadroom`、
`activeHeadroomKnown`、`extendedLinearActive`、`fallbackReason`、
`fallbackCount`、`dataSpaceFailures`、`headroomUpdates`、
`extendedLinearFrames` です。active Android scRGB は
`androidExtendedLinearScRgb + sixteenBitFloat + nativeDataSpace 406913024` です。
API 34+ で Android が valid ratio を公開すると、`activeHeadroom` は current display HDR/SDR
ratio、`activeHeadroomKnown` は true です。ratio unavailable の場合、この値は fallback
のみで `activeHeadroomKnown` は false です。known state または ratio が実際に変わった
場合だけ `headroomUpdates` が増え、duplicate listener notification は無視されます。

`ErikaOutputFallbackReason` は stable ABI code です。

| Code | Dart value | Stable label |
|------|------------|--------------|
| 0 | `none` | `none` |
| 1 | `displayHdrUnsupported` | `display_hdr_unsupported` |
| 2 | `hybridCompositionRequired` | `hybrid_composition_required` |
| 3 | `wgpuBackendNotVulkan` | `wgpu_backend_not_vulkan` |
| 4 | `rgba16FloatSurfaceFormatUnavailable` | `rgba16float_surface_format_unavailable` |
| 5 | `nativeWindowDataSpaceApiUnavailable` | `native_window_dataspace_api_unavailable` |
| 6 | `scrgbDataSpaceVerificationFailed` | `scrgb_dataspace_verification_failed` |
| 7 | `surfaceConfigureFailed` | `surface_configure_failed` |
| 8 | `legacyAppleEdrUnsupported` | `legacy_apple_edr_unsupported` |

`player.screenshot()` は current composited frame（video + subtitle + danmaku）の raw SDR
RGBA8 を返し、display が Apple EDR / Android extended-linear の場合も SDR のままです。
Metal と Android/wgpu は capture 実装済みですが、現在の Windows D3D11 Flutter path は
screenshot byte を返しません。

non-HDR emulator/device coverage は明示的 SDR fallback と reason を検証します。Active
extended-linear はまだ実機検証済みとは claim せず、API 35 HDR device で
`Rgba16Float + SCRGB_LINEAR`、live HDR/SDR-ratio update、rotation/background recovery、
multiple player、SDR screenshot の acceptance が必要です。

## Upscaler

作成時に ArtCNN を選択することも、runtime で切り替えることもできます。

```dart
final player = ErikaPlayer(upscaler: ErikaUpscalerMode.artCnnC4F16);
await player.setUpscaler(ErikaUpscalerMode.artCnnC4F16);
```

`ErikaUpscalerMode.off` で無効化します。`player.getUpscalerStatus()` では要求モード、実行 backend、fallback 回数、upscaled frame 数、最近の GPU timing を確認できます。Apple は Metal、Android は planar と MediaCodec Surface frame の両方で wgpu/Vulkan compute を使います。GLES 3.0 は通常再生を維持し、明示的な `inactive` fallback を報告します。
