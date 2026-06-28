# ネイティブホストへの Erika 組み込み

本ガイドは Erika を非 Flutter ホスト——C/C++/Swift アプリ、Win32 ウィンドウ、C FFI を
持つ任意のランタイム——に組み込む手順です。**presenter（プッシュ）モデル**を使います。
Erika が decode・timing・audio・overlay・presentation を所有し、ホストは surface を提供して
フレームごとに `render_tick` を 1 回呼びます。

前提：C ABI（[capi_reference.ja.md](capi_reference.ja.md)）とビルド済みの Erika ライブラリ
（[building.ja.md](building.ja.md)）。Flutter では代わりに
[`erika_flutter`](../packages/erika_flutter) プラグインを使い、
[flutter_embedding.ja.md](flutter_embedding.ja.md) を参照してください。

> 英語版：[integration.md](integration.md)。

実行可能な参考が 2 つ付属します：
[`examples/macos_native_demo`](../examples/macos_native_demo)（AppKit + `CAMetalLayer`）と
[`examples/windows_native_demo`](../examples/windows_native_demo)（Win32 + `HWND`）。これらは
Rust の `PresenterRuntime` を直接駆動します。以下の C ABI 呼び出しは 1:1 の等価物です。

## 1. handle ファミリーを選ぶ

自分で描画する理由がなければ `ErikaPresenterHandle` を使います。プルモデルの
`ErikaHandle` は、独自コンポジタを持ち Erika の decode/timing/state だけが欲しいホスト
向けです。本ガイドの残りは presenter ベースです。

presenter ファミリーは **macOS / iOS / Windows のみ**でコンパイルされます。

## 2. ライフサイクル

```
create ──▶ attach surface ──▶ open ──▶ play ──▶ (render_tick + poll_event ループ)
                                                       │
                               pause / seek / set_* ◀──┤
                                                       ▼
                         detach surface ──▶ destroy
```

`open` は非同期です。handle は `Opening → Ready → Playing` と遷移します。ブロックせず
イベントで遷移を観測してください。surface は `open` の前後どちらでも attach できますが、
先に attach するとアイドルのテストパターン / 最初のフレームがすぐ表示されます。

## 3. presenter を作る

```c
ErikaPresenterConfig cfg = {
    .output_mode  = ErikaPresenterOutputMode_Sdr,   // macOS/iOS では _AppleEdr も
    .edr_headroom = 1.0f,                            // Apple EDR のみ使用
    .luma_upscaler = ErikaLumaUpscalerMode_Off,      // または ArtCnnC4F16 / C4F32
};
ErikaPresenterHandle *p = erika_presenter_create_with_config(cfg);
if (!p) { /* erika_last_error_message() を読む */ }
```

`erika_presenter_create()` は既定値（SDR、アップスケーラ無し）。神経輝度アップスケーラは
Metal compute 機能で、D3D11/wgpu backend では `set_upscaler` は受理される no-op
フォールバックです。

## 4. surface を attach する

Erika は所有する surface に直接描画します。幅・高さは**物理ピクセル**、`scale` は
DPI/backing 係数です。

### macOS / iOS —— `CAMetalLayer`

`CAMetalLayer` を作りサイズを設定し、そのポインタを Erika に渡します：

```c
// `layer` は CAMetalLayer*（例：NSView/UIView のホスト layer から）
erika_presenter_attach_metal_layer(p, (uint64_t)(uintptr_t)layer,
                                   pixel_w, pixel_h, backing_scale);
```

macOS で推奨の構成は、コンテンツビューの sibling / underlay となる window-hosted layer
で、映像を AppKit のビューコンポジタの外に保ちます（Flutter プラグインと同じモデル。
[flutter_embedding.ja.md](flutter_embedding.ja.md) 参照）。

### Windows —— `HWND`

```c
HWND hwnd = /* あなたのウィンドウ */;
HINSTANCE hinst = GetModuleHandleW(NULL);
UINT dpi = GetDpiForWindow(hwnd);
double scale = dpi ? (double)dpi / 96.0 : 1.0;
RECT rc; GetClientRect(hwnd, &rc);
uint32_t w = max(1, rc.right - rc.left), h = max(1, rc.bottom - rc.top);

erika_presenter_attach_windows_hwnd(p, (uint64_t)(uintptr_t)hwnd,
                                    (uint64_t)(uintptr_t)hinst, w, h, scale);
```

`attach_windows_hwnd` は `attach_wgpu_surface` + kind `WindowsHwnd` の便利ラッパです。
既定の presenter 設定では、この surface は**ネイティブ Direct3D 11** レンダラ（D3D11VA
ゼロコピー、HDR10）を駆動します。wgpu フォールバックが本当に必要なときだけ設定で渡します。

### 汎用 —— `attach_wgpu_surface`

X11/Wayland/Android、または surface 種別を明示したい場合は、対応する
`ErikaWgpuSurfaceKind` とプラットフォームハンドルを付けて
`erika_presenter_attach_wgpu_surface(p, kind, raw_window, raw_display, w, h, scale)`
を使います。

## 5. open して play

```c
if (erika_presenter_open(p, "/path/to/video.mkv") != ErikaStatus_Ok) { /* ログ */ }
erika_presenter_play(p);
```

`uri` はローカルパスまたは HTTP(S) URL。

## 6. レンダーループ

surface の表示タイマー——`CADisplayLink`（iOS）/ `CVDisplayLink` または `CADisplayLink`
（macOS）/ Windows のフレームスケジューラ——から `render_tick` を駆動します。そのフレームの
**プレゼンテーション時刻（秒）**を単調なホストクロックから渡します。Erika は vsync 量子化
スケジューリングに使うので、差分ではなく絶対タイムスタンプを渡します。

```c
// 表示フレームごと:
ErikaPresenterStats stats;
erika_presenter_render_tick(p, host_time_seconds, &stats);   // out_stats は NULL 可

// 同じ反復でイベントをドレイン:
ErikaEvent ev;
while (erika_presenter_poll_event(p, &ev) == ErikaStatus_Ok) {
    handle_event(&ev);
}
```

drawable サイズや scale が変わったら（ウィンドウリサイズ、モニタ DPI 変更、デバイス回転）、
次の tick の**前に** `erika_presenter_resize_surface(p, w, h, scale)` を呼びます。Windows
demo は毎フレーム `GetClientRect` + `GetDpiForWindow` をポーリングし、変化時にリサイズします。

`render_tick` はすぐ返ります。それ自体は vsync でブロックしません——リズムは表示タイマーが
与えます。表示コールバック上にいない場合（スモークテスト等）、反復ごとに `~16 ms` の
sleep で 60 Hz を近似できます。

## 7. イベント処理

`poll_event` は非ブロッキングで、キューが空なら `NoEvent` を返します。`ev.kind` で分岐：

| Kind | 意味 | 読む |
|------|------|------|
| `StateChanged` | 再生状態が遷移 | `ev.state` |
| `DurationChanged` | 尺が判明/更新 | `ev.duration_micros` |
| `PositionChanged` | 周期的な位置 tick | `ev.position_micros` |
| `TracksChanged` | トラックリスト変化 | `erika_presenter_tracks` を再取得 |
| `TrackSelectionChanged` | 選択変化 | `erika_presenter_track_selection` |
| `BufferingChanged` | バッファリング切替 | `ev.buffering` |
| `VideoParamsChanged` | 解像度 / 色メタデータ | `ev.video` |
| `Error` | 失敗発生 | `ev.status` + `erika_last_error_message` |

## 8. ランタイム制御

以下はすべて tick の合間にライブで安全に呼べます：

- **トランスポート:** `play` / `pause` / `stop` / `seek(position_micros)` /
  `set_playback_rate(rate)`。
- **オーディオ:** `set_volume(0.0–1.0)`。
- **トラック:** `erika_presenter_tracks`（カウント配列イディオム）、`select_audio_track` /
  `select_subtitle_track`（id `-1` で字幕無効）、`add_external_subtitle`、
  `remove_subtitle_track`、`set_subtitle_scale`。
- **弾幕:** トラックの読み込み（`load_danmaku_file` / `_json` またはマルチトラック
  `add_danmaku_track_*`）、トグル（`set_danmaku_enabled`）、`set_danmaku_config` で調整、
  トラックのオフセット、フォント設定、ブロックワード設定。
  [danmaku_architecture.md](danmaku_architecture.md) 参照。
- **アップスケーラ:** `set_upscaler(mode)`。`get_upscaler_status` で確認。

## 9. 解放

```c
erika_presenter_detach_surface(p);   // 先に surface への描画を止める
erika_presenter_destroy(p);          // 再生を止め、すべて解放
```

ウィンドウ/layer を破棄する前に detach し、Erika が surface に触れるのを止めます。
`destroy` は `NULL` handle に対して安全です。

## 10. スレッドモデル

handle は**内部同期されません**。最もシンプルで正しい設計：presenter を 1 つのスレッド
——表示タイマーを動かすスレッド——で所有し、すべての呼び出し（`render_tick`、トランス
ポート、トラック変更）をそこから行います。別スレッドから呼ぶ必要がある場合（UI スレッドが
`seek` を発行する等）、同じ handle で 2 つの呼び出しが重ならないよう自分のロックで直列化
します。エラーメッセージはスレッドローカルなので、失敗した呼び出しを行ったスレッドで
`erika_last_error_message` を読みます。

## 言語別メモ

### C / C++

`erika.h` を include し、ライブラリをリンク（[building.ja.md](building.ja.md) 参照）すれば
完了——ABI は素の C です。C++ ではデストラクタで `erika_presenter_destroy` を呼ぶ RAII 型に
handle を包み、返された文字列 / `ErikaTrackInfo` レコードは対応する Erika の解放関数で解放
し、`delete` は使いません。

### Swift

bridging header か `erika.h` への module map で C ABI を取り込みます。`CAMetalLayer` は
`unsafeBitCast(layer, to: UInt64.self)` または `UInt64(UInt(bitPattern: ...))` でキャスト。
`CADisplayLink`/`CVDisplayLink` のコールバックから `erika_presenter_render_tick` を駆動します。
macOS/iOS の Flutter Swift プラグインも同じ C ABI 上でこれを行っています。

### Dart FFI

`dart:ffi` でシンボルをバインド（dylib/dll は `DynamicLibrary.open`、静的リンクはプロセス
シンボル）。すべての FFI 呼び出しを 1 つの isolate に置き、文字列は `toNativeUtf8`/`free` で
マーシャリングします。高レベルの `erika_flutter` パッケージが既にこれを行っているので、
カスタム組み込みでない限りそちらを優先してください。

## チェックリスト

- [ ] presenter を作成（ディスプレイに合った出力モードで）。
- [ ] **物理ピクセル**サイズと正しい scale で surface を attach。
- [ ] open してから play。ブロックせずイベントで準備完了を観測。
- [ ] 表示フレームごとに `render_tick(absolute_time_seconds)`。イベントをドレイン。
- [ ] サイズ/scale 変化のたびに `resize_surface`。
- [ ] handle ごとに 1 スレッド、または呼び出しを直列化。
- [ ] 返された文字列 / `ErikaTrackInfo` をすべて解放。`detach` してから `destroy`。
