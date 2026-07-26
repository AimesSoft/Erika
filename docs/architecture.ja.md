# Erika Architecture

[中文](architecture.zh.md) | [English](architecture.md) | [日本語](architecture.ja.md)

Erika は埋め込み可能な Rust メディア再生ライブラリです。ホストアプリは Rust API、C ABI (`erika_capi`)、または Flutter バインディング (`erika_flutter`) から呼び出せます。動画フレーム、字幕、弾幕はすべてエンジン内部に留まり、レンダラー内で合成され、ホストの描画パイプラインは経由しません。

## システム概要

```text
Rust Player Core
  source abstraction ─── file + HTTP range
  FFmpeg wrappers ────── custom AVIO, probe, demux, decode, seek, audio resample
  playback engine ────── video/audio tick, clock, frame scheduler
  video decode ───────── VideoToolbox (macOS/iOS), D3D11VA (Windows), software fallback
  audio output ───────── CoreAudio (macOS), AudioQueue (iOS), WASAPI (Windows), ring buffer
  overlay timeline ───── subtitle + danmaku composition
  renderer core ──────── color state, render graph, tone map, scaler policy
  Metal renderer ─────── zero-copy NV12/P010, HDR/EDR, subtitle/danmaku pass
  D3D11 renderer ─────── zero-copy D3D11VA, HDR10, subtitle/danmaku pass (Windows)
  wgpu renderer ──────── cross-platform video, overlays, capture, Android scRGB
  presenter runtime ──── ties player + renderer + audio + overlays
  C ABI ──────────────── 73 exported functions, two handle families
  Flutter plugin ─────── macOS + iOS + Windows + Android native view embedding
```

## ネイティブ依存関係

`xtask` は固定の upstream からネイティブ依存関係をダウンロード・ビルド・インストールし、`third_party/` に配置します。既定 profile は `lgpl` です。

| 依存関係 | バージョン | 目的 |
|----------|-----------|------|
| FFmpeg | 7.1.1 | Demux、decode、audio resample、プラットフォーム HW decode |
| dav1d | 1.5.1 | Android AV1 software fallback（8-bit / high bit depth） |
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

Decoder availability は session invariant です。video track が選択されている場合、play / seek / video-frame pump の各入口には active video decoder が必要です。MediaCodec seek reopen や Surface→ByteBuffer/software fallback のような破壊的 transition では、先に decoder-unavailable reason を記録します。最終 software decoder の open まで失敗した場合、各入口はその明示的 error を返して media の reopen を要求し、audio-only の偽 `Playing` state には入りません。

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
- **libass renderer**: static link で既定有効。ASS script を受け取り、`ass_render_frame` を呼び、alpha plane を Erika の overlay system に取り込みます。macOS では CoreText font provider、Windows では DirectWrite を使います。それ以外の target は system font provider 無しで動作するため（vendored libass は fontconfig を無効化）、全 platform で memory font として登録される内蔵の Droid Sans Fallback が libass の既定 family になります。
- **字幕スタイル**: custom な font family・font file・font size・縁取り幅・文字/縁取りの色は fallback として働き、script が指定していない部分と plain-text 字幕の styling を埋めます（サイズと縁取りには字幕 scale がさらに掛かります）。`force_override` を立てると libass の selective style override に昇格し、ASS dialogue が要求する styling も置き換えます。
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
- Neural luma upscaler（`LumaUpscalerMode`）: ArtCNN C4F16/C4F32 2x doubler を Metal compute pass として decoded Y plane に適用し、render pass と同じ command buffer で実行します（`renderer/metal/upscaler.rs`）。Chroma は source resolution のままです。動画が source resolution より大きく表示される場合のみ動作し、network output は decoded frame ごとに cache されるため、同じ frame の繰り返し vsync tick では compute を再実行しません。weights は upstream ONNX release（`assets/artcnn/`）から変換し、`tests/artcnn_upscaler.rs` の onnxruntime reference で検証しています。backend は `simdgroup_matrix` matmul（Apple Silicon default）と scalar texture fallback の 2 つで、どちらも background thread で compile され、準備完了までは未拡大で再生を続けます。blob validation、model layout、execution policy、frame-token cache は platform-neutral な `renderer/artcnn.rs` にあり、wgpu backend も共有します。
- Subtitle overlay: RGBA plane upload と alpha blending。
- Danmaku: atlas からの instanced glyph quad drawing（shadow → outline → fill）。
- Presentation layout は source aspect ratio を保ちます。

### Direct3D 11 Renderer（Windows）

Windows のネイティブ renderer（`renderer/d3d11.rs`）：

- ゼロコピー D3D11VA デコードテクスチャ相互運用：デコードされた `ID3D11Texture2D` surface を render device に共有し、CPU を経由しません。
- YCbCr サンプリングと色空間変換（HLSL shader）、Metal と同じ pipeline model。
- HDR10 出力：`R10G10B10A2_UNORM` swapchain + `DXGI_HDR_METADATA_HDR10`、SDR（`BGRA8`）fallback あり。
- 字幕 overlay の alpha-atlas upload と blending、atlas からの instanced danmaku glyph quad 描画。
- `render_tick` で駆動される window-hosted swapchain。

### wgpu Renderer（cross-platform）

移植性向けの第二 backend です。

- `wgpu` dependency と device / surface / pipeline creation。
- NV12/P010 video frame upload と WGSL YCbCr conversion shader。
- Android MediaCodec Surface を AHardwareBuffer/Vulkan で import。native interop が使えない場合は ByteBuffer/CPU upload に明示的に fallback します。
- 共通の `renderer/frame.rs` boundary は geometry/color metadata と、FFmpeg decoded frame または独立した prepared AHardwareBuffer を保持します。MediaCodec Surface の AVFrame は decoder callback context が有効な playback worker 上で解放され、presenter と GPU recovery は decoder-owned AVFrame を保持しません。
- 色空間変換と tone mapping（Metal と同じ pipeline model）。
- bounded feature texture と source-sized packed DepthToSpace output を使う tiled ArtCNN C4F16/C4F32 compute（`renderer/wgpu_artcnn.rs`）。native luma と Android の converted nonlinear RGB の両方を扱い、`rgb + (Y_sr - Y)` で chroma を保持します。GLES 3.0 は compute を試さず、`Inactive` と `native_luma_sampling` fallback を明示します。
- subtitle/danmaku composite、frame capture、headless testing 用 offscreen target。display surface が extended-linear の場合も screenshot は常に SDR RGBA8 target へ offscreen render し、未マップの scRGB 値を SDR pixel として返しません。
- surface handle model は macOS NSView、iOS UIView、Windows HWND、X11/Wayland、Android native window をカバーします。
- Android は bounded Vulkan/GLES backend recovery と import/capability/quality/device-failure diagnostics を備えます。high-headroom output は FP16 **extended-linear scRGB** であり HDR10/PQ ではありません。renderer は `Rgba16Float`、Vulkan extended-sRGB-linear color space を使い、configure/reconfigure ごとに `ANativeWindow` の `ADATASPACE_SCRGB_LINEAR`（`0x18410000`）を検証します。Android scRGB は BT.709 primaries、`1.0 = 80 nit` で、PQ や HDR10 static metadata は出力しません。
- Extended-linear が active になる条件は、`ExtendedLinear` の明示要求、HDR 対応 display/surface、Flutter Hybrid Composition の `SurfaceView`、Vulkan wgpu backend、surface の `Rgba16Float` 対応、`SCRGB_LINEAR` dataspace readback 成功です。どれかが欠けると通常の SDR surface を直ちに選び、安定 ABI の fallback reason `0..8` を記録します。そのため GLES と `TextureView` は SDR path です。
- API 34+ では Android host が `Display.registerHdrSdrRatioChangedListener` で display を監視し、実際の変化を `erika_presenter_set_output_headroom` で Erika に publish します。wgpu は surface を reattach せず、後続 frame の effective content headroom と queryable output status を更新します。ratio が available なら `activeHeadroomKnown` は true で、known flag または ratio が実際に変わった場合だけ `headroomUpdates` が増えます。
- Flutter extended-linear player で `edrHeadroom` を明示しない場合、content ceiling は default 4x、`SurfaceView` の desired headroom は system auto を意味する `0` です。明示値は content ceiling となり、API 35 では per-`SurfaceView` desired headroom にもなります。global Window は変更しません。
- emulator / non-HDR device では明示的な SDR fallback branch を検証済みです。API 35 HDR 実機で `Rgba16Float + SCRGB_LINEAR` の acceptance が完了するまで、active extended-linear path を実機検証済みとは表現しません。

### Render Pipeline

`renderer::pipeline` は backend が消費する前に、Rust 側で描画判断を記述します。

- `SourceColorState` / `TargetColorState`: primaries、transfer、range。
- `VideoRenderPipeline`: gamut matrix、tone map operator、transfer functions。
- `renderer::output`: native renderer と wgpu が共有する requested mode、active encoding、surface format、dataspace/headroom state、stable fallback diagnostics。
- HDR metadata: mastering display、content light level、nominal peak nits。

## Presenter Runtime

`PresenterRuntime` は Player、MetalRenderer、OverlayTimeline、DanmakuEngine、audio output をつなぎます。host は native surface を提供し、display timer から `render_tick` を呼びます。

- video frame を pump し、overlay（subtitle + danmaku）を更新し、render して present します。
- danmaku plan generation は generation + media_time gate で video frame と同期します。
- seek、track selection、fallback、open/close など decoder transition は quiesce/ACK barrier を使い、frame output 停止、renderer cache/receiver 解放、transition 完了後の resume の順で処理します。playback generation は reopen をまたいで単調増加し、古い generation / MediaCodec route の import feedback は破棄されます。
- playback rate、volume、track selection、subtitle/danmaku configuration を runtime で変更できます。

## C ABI

`erika_capi` は 2 つの handle family で 73 関数を export します。

- **`ErikaHandle`**: player control と event polling。rendering は host 管理です。
- **`ErikaPresenterHandle`**: Erika が full stack を所有します。host は surface を渡して `render_tick` を呼びます。

create/destroy、open/play/pause/stop/seek、track selection、subtitle track add/remove、danmaku track management（add/remove/enable/offset/config）、surface attach/detach/resize、event polling、volume、playback rate、neural luma upscaler switching、upscaler diagnostics、`erika_presenter_get_output_status` が返す 13-field output status snapshot を含みます。

Header: `crates/erika_capi/include/erika.h`

## Flutter Plugin

`packages/erika_flutter` は macOS / iOS / Windows / Android の Flutter embedding を提供します。

- **Dart**: `ErikaPlayer`（commands + events）、`ErikaWindowOverlayVideoView`（推奨の window-hosted native surface——Apple では Metal、Windows では D3D11 swapchain）、`ErikaVideoView`（compatibility platform view）。
- **macOS Swift plugin**: `liberika_capi.dylib` を読み込み、`NSWindow` overlay または `NSView`/`CAMetalLayer` platform view surface を作成し、display link から `render_tick` を駆動します。
- **iOS Swift plugin**: `liberika_capi.a` を static link し、`UIWindow` overlay または `UIView`/`CAMetalLayer` platform view surface を作成し、同じ presenter model を使います。
- **Windows C++ plugin**（`ErikaFlutterPluginCApi`）: CMake（`build_erika_runtime.cmake`、cargo target `x86_64-pc-windows-msvc`）で `erika_capi.dll` をビルド・リンクし、window-level D3D11 swapchain を host し、frame scheduler から `render_tick` を駆動します。
- **Android Kotlin/JNI plugin**: Android ABI 向け Rust runtime をビルドし、player ごとに独立した native surface を持ちます。SDR は `TextureView`、extended-linear の要求時は Flutter Hybrid Composition の `SurfaceView` を使います。Activity surface lifecycle、audio focus、noisy-route policy、HDR eligibility/headroom を調整し、active player がある間だけ shared frame scheduler を駆動します。

embedding model と HDR strategy は `docs/flutter_embedding.md` を参照してください。

## Platform Support

| Platform | Decode | Render | Audio | Status |
|----------|--------|--------|-------|--------|
| macOS 14+ | VideoToolbox | Metal | CoreAudio | Available |
| iOS 16+ | VideoToolbox | Metal | AudioQueue | Available |
| Windows 10+ | D3D11VA | Direct3D 11 | WASAPI | Available |
| Linux | — | wgpu (planned) | — | Planned |
| Android 8+ | MediaCodec / software | wgpu Vulkan + GLES fallback | AAudio | Available。SDR は検証済み、extended-linear scRGB は API 35 HDR 実機 acceptance 待ち |

