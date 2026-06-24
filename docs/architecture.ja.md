# Erika Architecture

[中文](architecture.zh.md) | [English](architecture.md) | [日本語](architecture.ja.md)

Erika は埋め込み可能な Rust メディア再生ライブラリです。ホストアプリは Rust API、C ABI (`erika_capi`)、または Flutter バインディング (`erika_flutter`) から呼び出せます。動画フレーム、字幕、弾幕はすべてエンジン内部に留まり、レンダラー内で合成され、ホストの描画パイプラインは経由しません。

## システム概要

```text
Rust Player Core
  source abstraction ─── file + HTTP range
  FFmpeg wrappers ────── custom AVIO, probe, demux, decode, seek, audio resample
  playback engine ────── video/audio tick, clock, frame scheduler
  video decode ───────── VideoToolbox (macOS/iOS), software fallback
  audio output ───────── CoreAudio (macOS), AudioQueue (iOS), ring buffer
  overlay timeline ───── subtitle + danmaku composition
  renderer core ──────── color state, render graph, tone map, scaler policy
  Metal renderer ─────── zero-copy NV12/P010, HDR/EDR, subtitle/danmaku pass
  wgpu renderer ──────── cross-platform video + danmaku rendering
  presenter runtime ──── ties player + renderer + audio + overlays
  C ABI ──────────────── 63 exported functions, two handle families
  Flutter plugin ─────── macOS + iOS native view embedding
```

## ネイティブ依存関係

`xtask` は固定の upstream からネイティブ依存関係をダウンロード・ビルド・インストールし、`third_party/` に配置します。既定 profile は `lgpl` です。

| 依存関係 | バージョン | 目的 |
|----------|-----------|------|
| FFmpeg | 7.1.1 | Demux、decode、audio resample、VideoToolbox |
| libass | 0.17.3 | ASS 字幕描画 |
| FreeType | 2.13.3 | フォントラスタライズ（libass 依存） |
| HarfBuzz | 10.4.0 | テキストシェーピング（libass 依存） |
| FriBidi | 1.0.16 | 双方向テキスト処理（libass 依存） |

すべて静的リンクです。libass とその依存関係は既定で有効です（`features = ["libass"]`）。

```sh
cargo run -p xtask -- deps build --all --profile lgpl
cargo run -p xtask -- deps status
```

## FFmpeg 統合

`erika_ffmpeg_sys` は build 時に bindgen で低レベルバインディングを生成します。`erika::ffmpeg` は安全な Rust ラッパーを提供します。

- **Demuxer**: `AVFormatContext` を保持し、`MediaSource` 由来の Rust-backed custom `AVIOContext` を使うこともできます。stream selection、reference-counted packets、timestamp-based seek をサポートします。
- **Decoder**: software と VideoToolbox hardware backend を持ちます。hardware frames は BT.2020/PQ metadata を保持し、`CVPixelBufferRef` を通じて Metal に zero-copy で渡せます。
- **AudioResampler**: `libswresample` を包み、interleaved f32 PCM（既定 48 kHz stereo）へ変換します。
- **SubtitleDecoder**: 埋め込みテキスト字幕と bitmap 字幕ストリームをデコードします。

## 再生エンジン

`PlaybackSession` は media を開き、track を選び、decode backend を設定し、video frame と PCM audio block を生成します。

`VideoPlaybackEngine` は clocked playback を追加します。

- Play / pause / stop / seek / playback rate control / EOF detection。
- `PlaybackClock`: audio-master clock discipline を持つ media-time anchor。
- `VideoFrameScheduler`: decoded video frame の present / wait / drop を決定します。
- `DisplaySyncState`: residual frame-duration error を持ち回る vsync quantizer です。

## 音声出力

- **macOS**: ring buffer と PTS-tracking clock snapshot を持つ CoreAudio 出力。presenter は snapshot を player worker に返し、audio-master clock discipline を維持します。
- **iOS**: 同じ ring buffer / clock snapshot model を持つ AudioQueue 出力。
- Ring buffer: interleaved f32、容量可変、overflow は oldest drop、volume control 対応。

## 字幕システム

- **Parsing**: SRT、WebVTT、ASS timeline parsing。embedded / external subtitle track を扱い、external track は runtime で追加・削除できます。
- **libass renderer**: static link で既定有効。ASS script を受け取り、`ass_render_frame` を呼び、alpha plane を Erika の overlay system に取り込みます。Apple platform では CoreText font provider を使います。
- **SubtitleRendererCore**: changed / unchanged frame を追跡し、不要な GPU upload を避ける renderer-facing boundary です。

## 弾幕システム

弾幕サブシステムは NipaPlay DFM+ の layout algorithm を Rust で native 実装しています。完全な設計は `docs/danmaku_architecture.md` を参照してください。

- **入力**: Bilibili XML、JSON、JSON-lines parsing。
- **DanmakuSession**: multi-track 管理、track ごとの enable/disable、track offset、global offset。
- **DFM+ layout core**: prepare / frame-query 分離。prepare は measurement、filtering、duplicate merge、collision avoidance、lane allocation を一括で処理します。frame query は指定 media time の positioned items を返します。
- **Text rasterizer**: fill / outline alpha mask を持つ glyph atlas と、GPU texture reuse 用の version tracking。
- **Render plan**: `DanmakuRenderPlan` は screen rect、atlas tex rect、色、outline、shadow を持つ glyph instances を運びます。Metal と wgpu は atlas から instanced quad を描画します。

## レンダラー

### Metal Renderer（macOS/iOS）

Apple platform の主 renderer です。

- `CVMetalTextureCache` 経由で CVPixelBuffer → MTLTexture を zero-copy import。
- YCbCr sampling、transfer decode、gamut mapping（BT.2020→BT.709、Display P3→BT.709）。
- Tone mapping: Mobius、Reinhard、clip with absolute nits。
- SDR output（`BGRA8Unorm`）と Apple EDR output（`RGBA16Float` + EDR headroom）。
- Neural luma upscaler（`LumaUpscalerMode`）: ArtCNN C4F16/C4F32 2x doubler を Metal compute pass として decoded Y plane に適用し、render pass と同じ command buffer で実行します（`renderer/metal/upscaler.rs`）。Chroma は source resolution のままです。動画が source resolution より大きく表示される場合のみ動作し、network output は decoded frame ごとに cache されるため、同じ frame の繰り返し vsync tick では compute を再実行しません。weights は upstream ONNX release（`assets/artcnn/`）から変換し、`tests/artcnn_upscaler.rs` の onnxruntime reference で検証しています。backend は `simdgroup_matrix` matmul（Apple Silicon default）と scalar texture fallback の 2 つで、どちらも background thread で compile され、準備完了までは未拡大で再生を続けます。
- Subtitle overlay: RGBA plane upload と alpha blending。
- Danmaku: atlas からの instanced glyph quad drawing（shadow → outline → fill）。
- Presentation layout は source aspect ratio を保ちます。

### wgpu Renderer（cross-platform）

移植性向けの第二 backend です。

- `wgpu` dependency と device / surface / pipeline creation。
- NV12/P010 video frame upload と WGSL YCbCr conversion shader。
- 色空間変換と tone mapping（Metal と同じ pipeline model）。
- Danmaku glyph atlas rendering。
- headless testing 用 offscreen render target。
- surface handle model は macOS NSView、iOS UIView、Windows HWND、X11/Wayland、Android native window をカバーします。
- VideoToolbox zero-copy import と HDR/EDR output は未実装です。

### Render Pipeline

`renderer::pipeline` は backend が消費する前に、Rust 側で描画判断を記述します。

- `SourceColorState` / `TargetColorState`: primaries、transfer、range。
- `VideoRenderPipeline`: gamut matrix、tone map operator、transfer functions。
- HDR metadata: mastering display、content light level、nominal peak nits。

## Presenter Runtime

`PresenterRuntime` は Player、MetalRenderer、OverlayTimeline、DanmakuEngine、audio output をつなぎます。host は native surface を提供し、display timer から `render_tick` を呼びます。

- video frame を pump し、overlay（subtitle + danmaku）を更新し、render して present します。
- danmaku plan generation は generation + media_time gate で video frame と同期します。
- playback rate、volume、track selection、subtitle/danmaku configuration を runtime で変更できます。

## C ABI

`erika_capi` は 2 つの handle family で 63 関数を export します。

- **`ErikaHandle`**: player control と event polling。rendering は host 管理です。
- **`ErikaPresenterHandle`**: Erika が full stack を所有します。host は surface を渡して `render_tick` を呼びます。

create/destroy、open/play/pause/stop/seek、track selection、subtitle track add/remove、danmaku track management（add/remove/enable/offset/config）、surface attach/detach/resize、event polling、volume、playback rate、neural luma upscaler switching、upscaler backend status diagnostics を含みます。

Header: `crates/erika_capi/include/erika.h`

## Flutter Plugin

`packages/erika_flutter` は macOS / iOS の Flutter embedding を提供します。

- **Dart**: `ErikaPlayer`（commands + events）、`ErikaWindowOverlayVideoView`（推奨の window-hosted native Metal surface）、`ErikaVideoView`（compatibility platform view）。
- **macOS Swift plugin**: `liberika_capi.dylib` を読み込み、`NSWindow` overlay または `NSView`/`CAMetalLayer` platform view surface を作成し、display link から `render_tick` を駆動します。
- **iOS Swift plugin**: `liberika_capi.a` を static link し、`UIWindow` overlay または `UIView`/`CAMetalLayer` platform view surface を作成し、同じ presenter model を使います。

embedding model と HDR strategy は `docs/flutter_embedding.md` を参照してください。

## Platform Support

| Platform | Decode | Render | Audio | Status |
|----------|--------|--------|-------|--------|
| macOS 14+ | VideoToolbox | Metal | CoreAudio | Available |
| iOS 16+ | VideoToolbox | Metal | AudioQueue | Available |
| Windows | — | wgpu (planned) | — | Planned |
| Linux | — | wgpu (planned) | — | Planned |
| Android | — | wgpu (planned) | — | Planned |

