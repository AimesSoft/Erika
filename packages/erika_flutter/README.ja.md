# erika_flutter

[中文](README.zh.md) | [English](README.md) | [日本語](README.ja.md)

Erika メディア再生エンジン向けの Flutter plugin です。

この plugin は Dart を hot path から外します。

- Dart は低頻度の player command と event stream だけを公開します。
- native plugin は 2 種類の surface を提供します。推奨は `ErikaWindowOverlayVideoView`（macOS/iOS は Metal、Windows は D3D11 swapchain）、互換用は `ErikaVideoView` です。
- macOS plugin は Erika の dynamic library を読み込みます。
- iOS plugin は Erika の static library を link します。
- Windows plugin は Erika C ABI DLL を build して link します。
- Erika は `ErikaPresenterHandle` を通じて playback、rendering、audio、timing、overlay を担当します。

## Video Surfaces

フルプレイヤーの macOS/iOS UI では `ErikaWindowOverlayVideoView` を使うのが推奨です。Flutter の layout では矩形領域を予約しつつ、plugin が横に native `CAMetalLayer` を持ち、video を Flutter platform-view compositor の外に置きます。

Windows では `ErikaWindowOverlayVideoView` が window-level の Direct3D 11 swapchain を sibling surface として host し、同じ overlay モデルに従います。

標準的な Flutter platform view が必要な場合は `ErikaVideoView` を使います。

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

## Output Mode

`ErikaPlayer()` は macOS plugin に現在の screen と environment から SDR か Apple EDR を選ばせます。Dart から EDR を強制するには：

```dart
final player = ErikaPlayer(
  outputMode: ErikaOutputMode.appleEdr,
  edrHeadroom: 4.0,
);
```

`ErikaOutputMode.sdr` で SDR 出力を強制できます。

## Upscaler

player 作成後に Dart から ArtCNN upscaling を有効にできます。

```dart
await player.setUpscaler(ErikaUpscalerMode.artCnnC4F16);
```

`ErikaUpscalerMode.off` で無効化します。`player.getUpscalerStatus()` では要求モード、実行 backend、fallback 回数、upscaled frame 数、最近の GPU timing を確認できます。

