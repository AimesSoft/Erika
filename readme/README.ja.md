[中文](../README.md) | [English](README.en.md) | [日本語](README.ja.md)

# Erika

> 「GOOD！私はErika、NipaPlayにおいてmdk、video player、libmpv、media kitに次ぐ5番目のプレイヤーカーネルです。」
> 「あなたを数えても、プレイヤーカーネルは4つだけ！」

**NipaPlay の自社開発再生コア。** Rust 実装、組み込み可能、デコードからレンダリングまで一手に引き受けます。

> 名前の由来は『うみねこのなく頃に』の探偵 **古戸ヱリカ**。
> そして [NipaPlay](https://github.com/AimesSoft/NipaPlay-Reload) は『ひぐらしのなく頃に』の古手梨花の口癖「にぱー☆」から——コミュニティではみんな「梨花」と呼んでいます。
> 一方は表舞台のプレイヤー、もう一方は舞台裏のエンジン。同じ世界から生まれた、表裏一体の存在です。

ホストアプリケーションはレンダリングサーフェスの提供と再生コマンドの送信のみを行い、デコード、タイミング同期、映像レンダリング、字幕、弾幕、音声出力はすべて Erika 内部で完結します。

現在のリリースは **0.2.1** です。[ドキュメント一覧](../docs/README.md) からガイドを選べます。

## 機能

- VideoToolbox、D3D11VA、MediaCodec、AVCodec によるハードウェアデコードとソフトウェアフォールバック。Linux は実験的に NVDEC / VA-API に対応。
- Metal、D3D11、wgpu による映像・字幕・弾幕の合成、HDR/EDR、Dolby Vision マッピング、ArtCNN 輝度アップスケーリング。
- ローカルファイルと HTTP(S)、カスタムヘッダー、先読みとリワインドキャッシュ。
- 音声マスタークロック、再生・一時停止・シーク・倍速、トラック切替と外部字幕。
- Rust、C ABI、Flutter、SwiftPM、OpenHarmony ArkTS から組み込み可能。

## 組み込み

| ホスト | インストール / 入口 | ガイド |
|---|---|---|
| Flutter | `flutter pub add erika_flutter`（0.2.1） | [プラグイン](../packages/erika_flutter/README.ja.md) |
| Swift / Apple | Xcode に `https://github.com/AimesSoft/ErikaSwift` を追加、0.2.1 以降 | [ErikaSwift](https://github.com/AimesSoft/ErikaSwift) |
| OpenHarmony / ArkTS | `ohpm install erika` | [ArkTS SDK](../packages/erika_ohos/README.md) |
| C / C++ | 同じバージョンの C ABI バンドルと `erika.h` | [ネイティブ組み込み](../docs/integration.ja.md) |
| Rust | Git 依存を `v0.2.1` に固定 | [ビルド](../docs/building.ja.md) |
| Linux | システム FFmpeg 8 とパッチ済み libass、ソースビルド | [Linux](../docs/linux.md) |

2026-10-02 時点の OHPM 公開版は 0.1.9、0.2.1 は審査に提出済みです。
ネイティブバンドルは [GitHub Releases](https://github.com/AimesSoft/Erika/releases/tag/v0.2.1) にあります。

### Flutter

```dart
import 'package:erika_flutter/erika_flutter.dart';

final player = ErikaPlayer();
await player.open('/path/to/video.mp4');
await player.play();

// Widget ツリーに追加。Apple の全画面プレイヤーにはネイティブ overlay も選べます。
ErikaVideoView(player: player)
```

Flutter のクリップやカラーフィルターには `ErikaTextureVideoView`、Apple EDR や
Windows HDR10 にはネイティブ映像サーフェスを使います。選択とライフサイクルは
プラグインガイドにまとめています。

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

先にネイティブ依存を準備します。`Player` は制御とフレーム購読、`PresenterRuntime`
はレンダリングと音声を担当します。C の入口は `ErikaHandle` と `ErikaPresenterHandle`、
宣言は [erika.h](../crates/erika_capi/include/erika.h) にあります。

## プラットフォーム

| プラットフォーム | デコード | レンダリング | 音声 | 配布 |
|---|---|---|---|---|
| macOS 11+ | VideoToolbox / software | Metal | CoreAudio | バンドル、Flutter、SwiftPM |
| iOS 13+ | VideoToolbox / software | Metal | AudioQueue | XCFramework、Flutter、SwiftPM |
| tvOS 13+ | VideoToolbox / software | Metal | AudioQueue | XCFramework、Flutter、SwiftPM |
| Windows 10+ | D3D11VA / software | D3D11 | WASAPI | x64 / ARM64 バンドル、Flutter |
| Android 8+ | MediaCodec / software | wgpu Vulkan / GLES | AAudio | 4 ABI バンドル、Flutter |
| OpenHarmony API 18+ | AVCodec / software | wgpu | OHAudio | arm64 バンドル、Flutter、OHPM |
| Linux（実験的） | NVDEC / VA-API / software | wgpu X11 / Wayland | PulseAudio / pipewire-pulse | Rust、C ABI、Flutter ソースビルド |

サーフェスと HDR の対応は [プラットフォーム表](../docs/platform_matrix.zh.md) にまとめています。
Web の再生バックエンドは未実装です。

## 開発

```sh
# Apple / Windows / Android / OpenHarmony のネイティブ依存
cargo run -p xtask -- deps build --all --profile lgpl
cargo build -p erika_capi
```

クロスビルドでは target triple を指定します。Linux はシステムライブラリを使います。
[ビルド](../docs/building.ja.md)、[リリース](../docs/releasing.ja.md)、
[開発者ガイド](../CONTRIBUTING.ja.md) に手順をまとめています。

## リポジトリ

| ディレクトリ | 内容 |
|---|---|
| `crates/erika` | 再生、デコード、クロック、字幕、弾幕、レンダリング |
| `crates/erika_capi` | C ABI と公開ヘッダー |
| `crates/erika_ffmpeg_sys` | FFmpeg bindings |
| `packages/erika_flutter` | Flutter プラグイン |
| `packages/erika_ohos` | OpenHarmony ArkTS SDK |
| `examples` | ネイティブ、Flutter、ArkTS サンプル |
| `xtask` | ネイティブ依存のビルド |
| `docs` | ガイドと設計。過去の調査・検証記録は `investigations/` |

変更履歴は [CHANGELOG](../CHANGELOG.md) にあります。

## ライセンス

Rust workspace は [MPL-2.0](../LICENSE) です。ネイティブ依存のプロファイルとライセンスは
[ビルドガイド](../docs/building.ja.md#ライセンス-profile) と
[第三者ライセンス](../packaging/THIRD_PARTY_NOTICES.md) に記載しています。
