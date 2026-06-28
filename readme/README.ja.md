[中文](../README.md) | [English](README.en.md) | [日本語](README.ja.md)

# Erika

> 「GOOD！私はErika、NipaPlayにおいてmdk、video player、libmpv、media kitに次ぐ5番目のプレイヤーカーネルです。」
> 「あなたを数えても、プレイヤーカーネルは4つだけ！」

**NipaPlay の自社開発再生コア。** Rust 実装、組み込み可能、デコードからレンダリングまで一手に引き受けます。

> 名前の由来は『うみねこのなく頃に』の探偵 **古戸ヱリカ**。
> そして [NipaPlay](https://github.com/AimesSoft/NipaPlay-Reload) は『ひぐらしのなく頃に』の古手梨花の口癖「にぱー☆」から——コミュニティではみんな「梨花」と呼んでいます。
> 一方は表舞台のプレイヤー、もう一方は舞台裏のエンジン。同じ世界から生まれた、表裏一体の存在です。

ホストアプリケーションはレンダリングサーフェスの提供と再生コマンドの送信のみを行い、デコード、タイミング同期、映像レンダリング、字幕、弾幕、音声出力はすべて Erika 内部で完結します。

## 機能

- **ハードウェアアクセラレーション** -- VideoToolbox (macOS/iOS)、D3D11VA (Windows)、他プラットフォームバックエンドへ拡張可能
- **ゼロコピーレンダリング** -- CVPixelBuffer から MTLTexture (Apple) / D3D11VA テクスチャ相互運用 (Windows)、CPU メモリを経由しない
- **HDR/EDR 出力** -- Apple EDR ネイティブサポート、Windows HDR10 スワップチェーン、PQ (BT.2020) メタデータ保持とトーンマッピング
- **Metal ネイティブレンダラー** -- YCbCr サンプリング、色空間変換、トーンマッピング、字幕/弾幕合成を単一レンダーパスで実行 (macOS/iOS)
- **Direct3D 11 ネイティブレンダラー** -- Windows: D3D11VA ゼロコピーテクスチャ相互運用、YCbCr サンプリング、HDR10 出力、字幕/弾幕 overlay 合成
- **ニューラル超解像** -- ArtCNN によるアニメ輝度 2x 超解像、Metal compute カーネル、simdgroup_matrix / スカラーの二重バックエンド、輝度プレーンのみゼロコピーでレンダーパイプラインに統合
- **音声出力** -- CoreAudio (macOS) / AudioQueue (iOS) / WASAPI (Windows)、f32 PCM リングバッファ、音声クロック同期
- **字幕** -- SRT / WebVTT / ASS パーサー、libass レンダリング（静的リンク）、埋め込みおよび外部字幕トラック
- **弾幕** -- Bilibili XML / JSON パーサー、DFM+ 衝突回避レーン配置エンジン、グリフアトラスによるネイティブ GPU レンダリング
- **再生エンジン** -- play / pause / stop / seek / 再生速度制御、音声マスタークロック同期、vsync 量子化フレームスケジューリング
- **C ABI** -- 69 のエクスポート関数、不透明ハンドル設計、C / C++ / Swift / Dart FFI / 任意の FFI 対応言語から呼び出し可能
- **Flutter プラグイン** -- macOS + iOS + Windows ネイティブビュー組み込み、HDR ネイティブレイヤーパスサポート
- **wgpu バックエンド** -- クロスプラットフォームレンダリング基盤構築済み (Linux / Android 方向)

## クイックスタート

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

// ディスプレイティック毎に:
ErikaPresenterStats stats;
erika_presenter_render_tick(presenter, host_time, &stats);
```

### Flutter

```dart
final player = ErikaPlayer();
await player.open('/path/to/video.mp4');
await player.play();

// 推奨: フルプレイヤー UI では Erika のネイティブ Metal レイヤーを使います。
ErikaWindowOverlayVideoView(player: player)

// 互換/診断: Flutter platform view 埋め込みも引き続き利用できます。
ErikaVideoView(player: player)
```

## C ABI インターフェースファミリー

異なる組み込みシナリオに対応する二つの C ABI エントリーポイントファミリーを提供します：

| ファミリー | ユースケース | レンダリング |
|-----------|-------------|-------------|
| `ErikaHandle` | ホストが独自のレンダーループを管理 | ホストがフレームデータを取得 |
| `ErikaPresenterHandle` | Erika が再生スタック全体を管理 | ホストはサーフェスを提供し `render_tick` を駆動 |

ヘッダー: [`crates/erika_capi/include/erika.h`](../crates/erika_capi/include/erika.h)

## プラットフォームサポート

| プラットフォーム | デコード | レンダリング | 音声 | 状態 |
|----------------|---------|-------------|------|------|
| macOS 14+ | VideoToolbox | Metal | CoreAudio | **利用可能** |
| iOS 16+ | VideoToolbox | Metal | AudioQueue | **利用可能** |
| Windows 10+ | D3D11VA | Direct3D 11 | WASAPI | **利用可能** |
| Linux | -- | wgpu (計画中) | -- | 計画中 |
| Android | -- | wgpu (計画中) | -- | 計画中 |

## リポジトリ構成

```
crates/erika              コア再生ライブラリ
crates/erika_capi         C ABI エクスポート層
crates/erika_ffmpeg_sys   FFmpeg 低レベルバインディング
packages/erika_flutter    Flutter プラグイン (macOS + iOS + Windows)
examples/                 検証・デモプログラム
xtask/                    ネイティブ依存関係ビルドオーケストレーション
docs/                     アーキテクチャと組み込みドキュメント
```

## ドキュメント

- [アーキテクチャ](../docs/architecture.ja.md) — エンジン設計、レンダーバックエンド、プラットフォーム対応
- [C ABI リファレンス](../docs/capi_reference.ja.md) — 全エクスポート関数、ステータスコード、所有権とスレッド規約
- [組み込みガイド](../docs/integration.ja.md) — C/C++/Win32/Swift など非 Flutter ホストへの組み込み
- [ビルドガイド](../docs/building.ja.md) — xtask、native 依存、クロスコンパイル
- [Flutter 組み込み](../docs/flutter_embedding.ja.md) ・ [弾幕アーキテクチャ](../docs/danmaku_architecture.ja.md)
- [コントリビュート / 開発者ガイド](../CONTRIBUTING.ja.md) — リポジトリ構成、スレッドモデル、プラットフォームバックエンドの追加

## ビルド

### 前提条件

- Rust 1.92+
- Xcode Command Line Tools (macOS/iOS)
- MSVC ツールチェーン + Windows SDK (Windows、ターゲット `x86_64-pc-windows-msvc`)
- CMake, pkg-config

### ネイティブ依存関係のビルド

```sh
# FFmpeg のビルド (LGPL プロファイル)
cargo run -p xtask -- deps build --profile lgpl

# 全依存関係のビルド (libass/FreeType/HarfBuzz/FriBidi 含む)
cargo run -p xtask -- deps build --all --profile lgpl

# 依存関係の状態確認
cargo run -p xtask -- deps status
```

### コンパイルとテスト

```sh
cargo build -p erika
cargo test --workspace
```

### 再生パスの検証

```sh
# macOS
export SAMPLE="/path/to/video.mp4"
cargo run -p macos_native_demo -- "$SAMPLE"
cargo run -p macos_native_demo -- --smoke-seconds 3 "$SAMPLE"

# Windows
cargo run -p windows_native_demo -- "%SAMPLE%"
```

## ライセンス

Rust ワークスペース: [MPL-2.0](../LICENSE)

ネイティブ依存関係のビルドプロファイルとライセンス境界は `xtask` を通じて独立管理されます。