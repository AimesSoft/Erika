# Flutter Embedding

[中文](flutter_embedding.zh.md) | [English](flutter_embedding.md) | [日本語](flutter_embedding.ja.md)

Erika は Flutter の動画レンダラーではありません。Flutter はあくまで任意の host UI です。再生コアが decode、timing、native rendering、字幕、弾幕、音声、HDR presentation を担当します。

## API Families

C ABI の entrypoint family は 2 つあります。

- `ErikaHandle`: control と event API。host が自前の presenter loop を持つ場合や、再生の probe / control だけを行いたい場合に使います。
- `ErikaPresenterHandle`: presenter-owned API。Erika が `Player + renderer + audio output` を所有し、host は native surface と display tick callback だけを提供する場合に使います。

どちらも `crates/erika_capi/include/erika.h` に定義されています。

## Apple Surface Strategies

Apple HDR path は Flutter Texture ではなく native Metal-backed surface を使います。Flutter plugin は macOS / iOS ともに 2 つの native surface strategy を公開し、host が UI に合う composition model を選べるようにしています。

### ErikaVideoView (Platform View)

標準的な Flutter platform view です。macOS では `NSView`/`CAMetalLayer`、iOS では `UIView`/`CAMetalLayer` で実装されます。plugin は `erika_flutter/video_view` として登録された native video view を作り、presenter に attach し、display link から描画を駆動します。

simple embedder や診断用途には便利です。macOS では AppKit/Flutter platform view composition の都合で black flicker などの compositor artifact が出ることがあるため、production path としては推奨されません。

### ErikaWindowOverlayVideoView (Window Overlay)

HDR/EDR の推奨 path は、Flutter の platform-view compositor の外側に置かれる window-hosted native overlay です。

1. Dart の `ErikaWindowOverlayVideoView` が widget tree 上の rectangle を確保します。
2. platform plugin が window-level native view を作り、`CAMetalLayer` を Flutter host view の sibling / underlay として配置します。
3. Flutter は該当 widget 領域を transparent に描画し、native video 用の hole を残します。
4. widget は位置を追跡し、surface generation number 付きで geometry update を送るため、dispose 済み widget からの古い hide call が新しく attach された surface に影響しません。
5. attach retry は exponential backoff で window readiness のタイミングを吸収します。

この overlay path は NipaPlay や他の full-player UI に推奨です。video presentation は Erika/Metal が持ち、Flutter は control / layout layer に専念できます。iOS では `UIWindow` + sibling `UIView`/`CAMetalLayer`、macOS では host `NSWindow` + sibling `NSView`/`CAMetalLayer` を使います。

touch events は両方の native video strategy を通過するので、Flutter controls を video surface の上や周囲に置けます。

## Android Surface Strategies

Android では 2 つの video widget が同じ native-view selector を使います。SDR は実体のある
`TextureView` を使い、検証済みです。wgpu は Vulkan を優先し、bounded GLES fallback も
備えます。
`ErikaOutputMode.extendedLinear` を要求すると、FP16 scRGB を Flutter の texture-layer
compositor に通さないよう、`PlatformViewLink` と Hybrid Composition で `SurfaceView` を
作ります。surface は `Choreographer` で駆動し、lifecycle、resize、audio focus、output
fallback は plugin が管理します。

FP16 extended-linear scRGB は `Rgba16Float` negotiation と
`ADATASPACE_SCRGB_LINEAR` verification まで実装済みですが、active path はまだ実機検証済み
とは claim しません。最終 acceptance には API 35 HDR device が必要です。HDR 非対応 display、
GLES、`TextureView`、FP16 不在、dataspace verification failure では SDR playback を継続し、
query 可能な fallback reason と明示的な log を提供します。

## iOS Build Path

iOS plugin は CocoaPod script phase 経由で Erika C ABI static library を app にリンクし、対象 iOS architecture 向けに Rust の `erika_capi` crate をビルドします。

## Minimal Presenter Flow

```c
ErikaPresenterHandle *presenter = erika_presenter_create();
erika_presenter_attach_metal_layer(
    presenter,
    (uint64_t)cametal_layer,
    width,
    height,
    backing_scale);
erika_presenter_open(presenter, "/path/to/media.mp4");
erika_presenter_play(presenter);

// On every display tick:
ErikaPresenterStats stats;
erika_presenter_render_tick(presenter, host_time_seconds, &stats);

// On resize:
erika_presenter_resize_surface(presenter, width, height, backing_scale);

// On dispose:
erika_presenter_detach_surface(presenter);
erika_presenter_destroy(presenter);
```

## Flutter Texture Path

Flutter Texture は機能が限定された compatibility path です。

用途:
- SDR fallback。
- native view composition がまだ使えない platform。
- test surface や制約の強い embedding 環境。

HDR/EDR の推奨 path ではありません。video が Flutter compositor に入ってしまうためです。C ABI はこの path のために `erika_attach_flutter_texture` を確保しています。

## wgpu と Android

Apple HDR path は native Metal、Windows は native Direct3D 11 renderer（D3D11VA
zero-copy decode、HDR10 output）を使います。Android では wgpu が実 renderer です。Vulkan は
MediaCodec Surface frame を AHardwareBuffer 経由で import し、software frame には明示的な
CPU upload fallback があります。video、subtitle、danmaku、capture、ArtCNN compute はこの
path を共有します。Vulkan は FP16 extended-linear scRGB を negotiate でき、GLES または
capability negotiation failure は明示的に SDR へ fallback します。Android SDR は検証済み、
API 35 HDR device の active-path acceptance は未完了です。Linux support は引き続き計画中です。

## Dart API

```dart
final player = ErikaPlayer(
  outputMode: ErikaOutputMode.appleEdr,  // optional: force EDR
  edrHeadroom: 4.0,                      // optional: EDR headroom
);

await player.open('/path/to/video.mp4');
await player.play();

// Preferred for full-player UIs on macOS/iOS:
ErikaWindowOverlayVideoView(player: player)

// Compatibility/diagnostic platform-view path:
ErikaVideoView(player: player)

// Playback control
await player.pause();
await player.seek(Duration(seconds: 30));
await player.setVolume(0.8);
await player.setPlaybackRate(1.5);

// Neural upscaler (anime luma 2x; Apple Metal / Android Vulkan)
await player.setUpscaler(ErikaUpscalerMode.artCnnC4F16); // off / artCnnC4F16 / artCnnC4F32
final status = await player.getUpscalerStatus();

// Track management
final tracks = await player.tracks();
await player.selectAudioTrack(trackId);
await player.selectSubtitleTrack(trackId);
await player.addExternalSubtitle('/path/to/subtitle.srt');
await player.setSubtitleScale(1.2);
// 字幕の fallback font と色（0xRRGGBBAA）。forceOverride は ASS script 自身の
// styling も置き換えます。
await player.setSubtitleStyle(
  fontFamily: 'Source Han Sans SC',
  primaryColorRgba: 0xFFFFFFFF,
  outlineColorRgba: 0x0000007F,
  fontSize: 48,
  outlineWidth: 2,
);

// Danmaku
await player.loadDanmakuFile('/path/to/danmaku.xml');
await player.addDanmakuTrackJson(jsonString, name: 'source', offset: Duration.zero);
await player.setDanmakuConfig(fontSize: 30, displayArea: 0.5);

// Events
player.events.listen((event) {
  // event.kind, event.state, event.position, event.duration, ...
});

await player.dispose();
```

## Neural Upscaler Status

`setUpscaler` はモードを要求するだけで、kernel は background thread で compile されます。そのため host は `getUpscalerStatus` を poll して UI を更新する必要があります。

| `activeBackend` | 意味 |
|-----------------|------|
| `off` | どの mode も要求されていない。 |
| `building` | kernel を compile 中（その mode の初回使用）。準備完了まで frame は未拡大で描画される。 |
| `inactive` | mode は要求されたが、この frame では適用されていない。たとえば video の表示サイズが source resolution を超えていない、または source が HDR（upscaler は SDR luma のみ処理）など。 |
| `scalar` | Metal scalar または wgpu compute backend で動作中。 |
| `simdgroupMatrix` | `simdgroup_matrix` backend で動作中（Apple Silicon の既定）。 |

upscaler は drawable が source resolution より大きく video を表示している場合にのみ有効に
なります。そのため、1080p source を 1080p（またはそれ以下）の view に出している間は
`inactive` のままです。C4F16 は realtime の推奨です。Apple の C4F32 は 1080p input で通常
M-Pro/Max クラスの GPU を必要とします。Android では両 model が Vulkan compute を使い、
GLES は明示的な `inactive` fallback を報告します。renderer 側の設計は
`docs/architecture.md` を参照してください。

## Ownership Rule

Flutter は layout と controls を担当し、Erika は video plane、subtitle plane、danmaku plane、audio、timing を担当します。plugin は `MethodChannel` を通じて command と event を橋渡しし、rendering は Dart を経由しません。

