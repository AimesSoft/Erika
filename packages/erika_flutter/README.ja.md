# erika_flutter

[中文](README.zh.md) | [English](README.md) | [日本語](README.ja.md)

Erika メディア再生エンジン向けの Flutter plugin です。

この plugin は Dart を hot path から外します。

- Dart は低頻度の player command と event stream だけを公開します。
- Apple plugin は 2 種類の native Metal surface を提供します。推奨は `ErikaWindowOverlayVideoView`、互換用は `ErikaVideoView` です。
- macOS plugin は Erika の dynamic library を読み込みます。
- iOS plugin は Erika の static library を link します。
- Erika は `ErikaPresenterHandle` を通じて playback、rendering、audio、timing、overlay を担当します。

## Video Surfaces

フルプレイヤーの macOS/iOS UI では `ErikaWindowOverlayVideoView` を使うのが推奨です。Flutter の layout では矩形領域を予約しつつ、plugin が横に native `CAMetalLayer` を持ち、video を Flutter platform-view compositor の外に置きます。

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

