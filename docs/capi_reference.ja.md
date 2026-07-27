# Erika C ABI リファレンス

本書は `erika_capi` が公開する安定 C ABI を説明します。宣言は
[`crates/erika_capi/include/erika.h`](../crates/erika_capi/include/erika.h) にあります。
この ABI は Rust 以外のすべてのホスト（C、C++、Swift、Dart FFI、Win32 …）の唯一の
組み込み面です。Rust 組み込み側は `erika` crate を直接使ってください。本層は FFI の
ためだけに存在します。

組み込みの流れ（surface attach、レンダーループ、解放）は
[integration.ja.md](integration.ja.md) を、高レベル設計は
[architecture.ja.md](architecture.ja.md) を参照してください。

> 英語版：[capi_reference.md](capi_reference.md)。

## 2 つの handle ファミリー

Erika は独立した 2 つのエントリーポイントを公開します。組み込みごとに一方を選びます。

| Handle | モデル | 描画担当 | 用途 |
|--------|--------|----------|------|
| `ErikaHandle` | **プル** | ホスト | レンダーループを自分で持ち、デコード済みフレームを pull / 独自コンポジタを駆動する。 |
| `ErikaPresenterHandle` | **プッシュ** | Erika | native surface を Erika に渡し、表示フレームごとに `render_tick` を 1 回呼ぶ。Erika が decode・timing・audio・overlay・presentation を所有する。 |

`ErikaPresenterHandle` が推奨パスで、Flutter plugin と native demo もこれを使います。
コンパイルされるのは **macOS / iOS / Windows / Android**。他のターゲットでは
`erika_presenter_create` は export されますが `NULL` を返し、presenter ファミリーの
残りは存在しません——presenter の利用はプラットフォームでガードしてください。

2 つのファミリーは状態を共有しません。1 プロセスで両方使えますが、1 つのメディア
セッションは正確に 1 つの handle に属します。

## 規約

### ステータスコード

失敗しうる呼び出しはすべて `ErikaStatus` を返します。

| 値 | コード | 意味 |
|----|--------|------|
| `ErikaStatus_Ok` | 0 | 成功。スレッドローカルのエラーをクリア。 |
| `ErikaStatus_NullPointer` | 1 | 必須の handle / out ポインタが `NULL`（または surface ポインタが 0）。 |
| `ErikaStatus_InvalidUtf8` | 2 | `const char*` 引数が不正な UTF-8。 |
| `ErikaStatus_PlayerError` | 3 | エンジンが呼び出しを拒否。メッセージを読む（下記）。 |
| `ErikaStatus_Panic` | 4 | 境界で Rust panic を捕捉。panic 前に完了した分を除き呼び出しは無効。handle は疑わしい状態とみなす。 |
| `ErikaStatus_NoEvent` | 5 | `*_poll_event` のみ：キューが空（エラーではない）。 |

戻り値は必ず確認してください。非エラーは `Ok` と `NoEvent` だけです。

### panic 安全性

ABI は FFI 境界を unwind が越えることを決して許しません。各エントリーは本体を
`catch_unwind` で包み、panic は `ErikaStatus_Panic` になりメッセージが設定されます。
C++ の `noexcept`/SEH を気にせず境界を越えて呼べます。

### エラーメッセージ（スレッドローカル）

`Ok`/`NoEvent` 以外の結果では、Erika は可読メッセージを**スレッドローカル**スロットに
保存します。取得：

```c
char *msg = erika_last_error_message();   // ヒープ確保、NULL の可能性あり
if (msg) { fprintf(stderr, "erika: %s\n", msg); erika_string_free(msg); }
```

スレッドローカルなので、失敗した呼び出しを行った**同じスレッド**で、かつそのスレッドの
次の呼び出しより前に読んでください（その後の `Ok` がクリアします）。
`erika_last_error_message` は所有権を持つコピーを返します——`erika_string_free` で解放。

### 文字列の所有権

Erika が返す `char*` はすべてヒープ確保で呼び出し側の所有です。

- 単独の文字列（例：`erika_last_error_message`）→ `erika_string_free` で解放。
- `ErikaTrackInfo` 内の文字列 → `erika_track_info_free(&track)` でレコード全体を解放
  （内部文字列をすべて解放）。
- `ErikaDanmakuTrackInfo` 内の文字列 → `erika_danmaku_track_info_free(&track)` で解放。

これらを libc の `free()` で解放しないでください。常に対応する Erika の解放関数を使い、
確保が同じ allocator 上で ABI を越えるようにします。

渡す `const char*` 引数は呼び出し中のみ借用され、Erika は必要分をコピーします。NUL
終端の UTF-8 でなければなりません。

### カウント配列イディオム

リスト getter（`erika_tracks`、`erika_presenter_tracks`、
`erika_presenter_danmaku_tracks`）は呼び出し側確保のバッファを使います。

```c
size_t total = 0;
erika_presenter_tracks(p, NULL, 0, &total);          // 1) 件数を問い合わせ
ErikaTrackInfo *buf = calloc(total, sizeof *buf);
erika_presenter_tracks(p, buf, total, &total);        // 2) 充填
for (size_t i = 0; i < total; i++) { /* buf[i] を使用 */ }
for (size_t i = 0; i < total; i++) erika_track_info_free(&buf[i]);
free(buf);
```

`out_len` は**常に**利用可能なレコードの総数に設定されます。書き込まれるのは最大
`capacity` 件。`capacity == 0`（`NULL` バッファ付き）はサイズ取得の正式な方法です。
実際に書き込まれたレコードのみ解放すべき文字列を保持します。

### Surface ジオメトリと scale

`attach_*` と `resize_surface` の `width`/`height` は**物理ピクセル**、`scale` は
backing/DPI 係数（Retina の `2.0`、Windows のモニタ倍率など）です。surface ポインタが
`0` だと `NullPointer` で拒否されます。

### スレッド

単一 handle は**内部同期されません**。同じ handle を複数スレッドから同時に呼ばないで
ください。自分で直列化する（または handle を 1 スレッドに閉じ込める）こと。presenter の
`render_tick` は表示タイマー / surface を持つスレッドから駆動します。異なる handle は
異なるスレッドで独立です。エラーメッセージはスレッドローカルである点に注意。

## `ErikaHandle` —— プルモデル

ホストが自分で描画を駆動し、状態/イベントを pull します。

### ライフサイクル

```c
ErikaHandle *erika_create(void);
void         erika_destroy(ErikaHandle *handle);
char        *erika_last_error_message(void);   // スレッドローカル、呼び出し側が解放
void         erika_string_free(char *value);
```

`erika_create` は失敗しません（有効な handle を返す）。`erika_destroy(NULL)` は no-op。
handle の破棄は再生を止め全リソースを解放します。

### 再生制御

```c
ErikaStatus erika_open(ErikaHandle *handle, const char *uri);   // ファイルパスまたは URL
ErikaStatus erika_open_with_headers(ErikaHandle *handle, const char *uri,
                                    const ErikaHttpHeader *headers, uintptr_t header_count);
ErikaStatus erika_play(ErikaHandle *handle);
ErikaStatus erika_pause(ErikaHandle *handle);
ErikaStatus erika_stop(ErikaHandle *handle);
ErikaStatus erika_close(ErikaHandle *handle);
ErikaStatus erika_seek(ErikaHandle *handle, uint64_t position_micros);
```

`uri` はローカルパスまたは HTTP(S) URL。`erika_open_with_headers` は HTTP(S) 再生用の
header を設定します。`headers` は呼び出し中だけ読み取られ、戻り値の後に解放できます。
`header_count` が 0 より大きい場合、`headers` は NULL にできません。header は HEAD、Range
GET、prefetch request に使用されます。
認証情報と Cookie は Erika の log に書き込まれません。`seek` は
マイクロ秒。`open` と `play` は
非同期にキューへ投入されます。ホスト UI スレッドをブロックせず、`StateChanged`、
`DurationChanged`、`Error` イベントで最終結果を確認してください。

### トラックと字幕

```c
ErikaStatus erika_add_external_subtitle(ErikaHandle *, const char *uri, int64_t *out_track_id);
ErikaStatus erika_remove_subtitle_track(ErikaHandle *, int64_t track_id);
ErikaStatus erika_select_audio_track(ErikaHandle *, int64_t track_id);
ErikaStatus erika_select_subtitle_track(ErikaHandle *, int64_t track_id);
ErikaStatus erika_track_selection(ErikaHandle *, ErikaTrackSelection *out_selection);
ErikaStatus erika_tracks(ErikaHandle *, ErikaTrackInfo *out_tracks, uintptr_t capacity, uintptr_t *out_len);
void        erika_track_info_free(ErikaTrackInfo *track);
```

`erika_tracks` はカウント配列イディオムに従います。`erika_track_selection` は現在選択中の
video/audio/subtitle トラック id（`-1` は無し）を報告します。字幕トラック id `-1` の選択で
字幕が無効になります。

### 状態とイベント

```c
ErikaStatus erika_state(ErikaHandle *, ErikaState *out_state);
ErikaStatus erika_poll_event(ErikaHandle *, ErikaEvent *out_event);
```

`erika_poll_event` は非ブロッキングで、キューが空なら `NoEvent` を返します。ループ内で
ドレインしてください。[イベント](#イベント) 参照。

### Surface attach（ホスト管理）

```c
ErikaStatus erika_attach_metal_layer(ErikaHandle *, uint64_t raw_layer, uint32_t w, uint32_t h, double scale);
ErikaStatus erika_attach_wgpu_surface(ErikaHandle *, ErikaWgpuSurfaceKind kind,
                                      uint64_t raw_window, uint64_t raw_display,
                                      uint32_t w, uint32_t h, double scale);
ErikaStatus erika_attach_wgpu_surface_with_output_capabilities(
                                      ErikaHandle *, ErikaWgpuSurfaceKind kind,
                                      uint64_t raw_window, uint64_t raw_display,
                                      uint32_t w, uint32_t h, double scale,
                                      ErikaSurfaceOutputCapabilities capabilities);
ErikaStatus erika_attach_flutter_texture(ErikaHandle *, ErikaFlutterTextureKind kind,
                                         int64_t texture_id, uint32_t w, uint32_t h, double scale);
ErikaStatus erika_detach_surface(ErikaHandle *);
```

`raw_layer` は `CAMetalLayer*` を `uint64_t` にキャストしたもの。
`erika_attach_wgpu_surface` では `raw_window`/`raw_display` がその `kind` のプラット
フォームの window/display ハンドル（例：`WindowsHwnd` なら `HWND` + `HINSTANCE`、
`XlibWindow` なら xcb/Xlib window + display）。`erika_attach_flutter_texture` は外部
テクスチャ id をプラットフォーム texture registrar に登録します。Android native host
が direct-composited かつ HDR-eligible な `SurfaceView` を宣言する場合は
`_with_output_capabilities` variant が必要です。短い関数は all-false/default
capabilities を渡すため Android extended-linear output は active になりません。

## `ErikaPresenterHandle` —— プッシュモデル

Erika がフルスタックを所有し、ホストは surface を提供して `render_tick` を呼びます。
**macOS / iOS / Windows / Android。**

### ライフサイクルと設定

```c
ErikaPresenterHandle *erika_presenter_create(void);
ErikaPresenterHandle *erika_presenter_create_with_config(ErikaPresenterConfig config);
ErikaPresenterHandle *erika_presenter_create_with_output_mode(int32_t output_mode, float edr_headroom);
void                  erika_presenter_destroy(ErikaPresenterHandle *handle);
```

`ErikaPresenterConfig` は出力モード（`Sdr`、Apple `AppleEdr`、Android
`ExtendedLinear`）、requested EDR/scRGB content-headroom ceiling、初期輝度アップスケーラを選びます。
Android `ExtendedLinear` は FP16 extended-linear scRGB で HDR10/PQ ではありません。
`create_with_output_mode` は短縮形、`create` は既定値（SDR、アップスケーラ無し）。
`NULL` が返れば作成失敗——`erika_last_error_message` を確認。

### 再生とランタイムパラメータ

```c
ErikaStatus erika_presenter_open(ErikaPresenterHandle *, const char *uri);
ErikaStatus erika_presenter_open_with_headers(ErikaPresenterHandle *, const char *uri,
                                              const ErikaHttpHeader *headers,
                                              uintptr_t header_count);
ErikaStatus erika_presenter_play(ErikaPresenterHandle *);
ErikaStatus erika_presenter_pause(ErikaPresenterHandle *);
ErikaStatus erika_presenter_stop(ErikaPresenterHandle *);
ErikaStatus erika_presenter_close(ErikaPresenterHandle *);
ErikaStatus erika_presenter_seek(ErikaPresenterHandle *, uint64_t position_micros);
ErikaStatus erika_presenter_set_playback_rate(ErikaPresenterHandle *, double rate);
ErikaStatus erika_presenter_set_volume(ErikaPresenterHandle *, double volume);   // 0.0–1.0
ErikaStatus erika_presenter_set_upscaler(ErikaPresenterHandle *, int32_t mode);  // ErikaLumaUpscalerMode
ErikaStatus erika_presenter_set_subtitle_scale(ErikaPresenterHandle *, double scale);
ErikaStatus erika_presenter_set_subtitle_font(ErikaPresenterHandle *, const char *family, const char *file_path);
ErikaStatus erika_presenter_set_subtitle_style(ErikaPresenterHandle *, uint32_t primary_rgba, uint32_t outline_rgba, double font_size, double outline_width, bool force_override);
ErikaStatus erika_presenter_set_output_headroom(ErikaPresenterHandle *, float headroom, bool known);
```

`set_playback_rate(1.0)` が通常速度。`set_upscaler` はランタイムで神経輝度アップ
スケーラを切り替えます（[`erika_presenter_get_upscaler_status`](#診断とスクリーンショット)
参照）。Metal と compute-capable な wgpu/Vulkan renderer は ArtCNN を実行し、
それ以外の backend は native luma sampling を維持して `Inactive` fallback を明示します。

`set_subtitle_font` と `set_subtitle_style` は字幕の **fallback** の見た目を設定します。
family や path が空ならその指定を解除します。色は `0xRRGGBBAA` で、既定は不透明な白の
文字（`0xFFFFFFFF`）と半透明な黒の縁取り（`0x0000007F`）です。`font_size`（既定 `48`、
`8..400` にクランプ）と `outline_width`（既定 `2`、`0..32` にクランプ）は ASS script 単位で、
`set_subtitle_scale` がさらに掛かります。container の ASS script は自身の styling を保ち、
これらは script が指定していない部分、system が解決できない font、そして plain-text
（SRT/WebVTT）字幕の見た目を埋めるだけです。`force_override = true` を渡すと、自前の
styling を持つ ASS dialogue にも font・サイズ・縁取り・色を適用します。

`ErikaHttpHeader` は次のように定義されます：

```c
typedef struct ErikaHttpHeader {
  const char *name;
  const char *value;
} ErikaHttpHeader;
```

例：

```c
ErikaHttpHeader headers[] = {
    {"Authorization", "Bearer token"},
    {"Referer", "https://example.com/"},
};
erika_presenter_open_with_headers(presenter, "https://example.com/video.mp4",
                                  headers, 2);
```

header は HTTP(S) source にだけ適用され、local file と Android の `content://` source では
無視されます。呼び出し側は URI、header 名、値を呼び出し中有効なまま保持してください。
この API はこれらの文字列の所有権を取得しません。

player 自身が生成する header は merge されず reject されます。下層の HTTP client は重複を
置き換えず追加するためです：`Range`、`Host`、`Content-Length`、`Transfer-Encoding`、
`Connection`（大文字小文字を区別せず一致）はいずれも呼び出しを
`ERIKA_STATUS_PLAYER_ERROR` で失敗させます。HTTP token として不正な header 名や、field
value に使えない文字を含む値も同様です。検証は最初の range request ではなく `open` の
時点で行われます。

header が適用されるのは media source だけです。外部 subtitle track と danmaku sidecar は
まだ header なしで取得されるため、video を認証する token はこれらの URL にはまだ適用
されません。

`set_output_headroom` は display の current HDR/SDR ratio を publish します。Android API 34+
host は `Display.registerHdrSdrRatioChangedListener` から呼び、valid ratio は `known = true`、
measurement unavailable は `(1.0f, false)` を渡します。値は `1.0..10000.0` に sanitize
されます。wgpu は duplicate state を無視し、surface reattach 無しで後続 frame target を
更新し、known state または ratio が実際に変わった場合だけ `headroom_updates` を増やします。
effective content target は configured content ceiling、正の surface `desired_headroom`、known
display ratio で制限されます。他 renderer はこの advisory update を無視できます。

### トラックと字幕

```c
ErikaStatus erika_presenter_add_external_subtitle(ErikaPresenterHandle *, const char *uri, int64_t *out_track_id);
ErikaStatus erika_presenter_remove_subtitle_track(ErikaPresenterHandle *, int64_t track_id);
ErikaStatus erika_presenter_select_audio_track(ErikaPresenterHandle *, int64_t track_id);
ErikaStatus erika_presenter_select_subtitle_track(ErikaPresenterHandle *, int64_t track_id);
ErikaStatus erika_presenter_track_selection(ErikaPresenterHandle *, ErikaTrackSelection *out_selection);
ErikaStatus erika_presenter_tracks(ErikaPresenterHandle *, ErikaTrackInfo *out_tracks, uintptr_t capacity, uintptr_t *out_len);
```

`ErikaHandle` のトラック関数と同じ意味論です。

### 弾幕（ダンマク）

```c
ErikaStatus erika_presenter_load_danmaku_file(ErikaPresenterHandle *, const char *uri);
ErikaStatus erika_presenter_load_danmaku_json(ErikaPresenterHandle *, const char *json);
ErikaStatus erika_presenter_add_danmaku_track_file(ErikaPresenterHandle *, const char *uri, const char *name, int64_t offset_micros, uint64_t *out_track_id);
ErikaStatus erika_presenter_add_danmaku_track_json(ErikaPresenterHandle *, const char *json, const char *name, int64_t offset_micros, uint64_t *out_track_id);
ErikaStatus erika_presenter_remove_danmaku_track(ErikaPresenterHandle *, uint64_t track_id);
ErikaStatus erika_presenter_set_danmaku_track_enabled(ErikaPresenterHandle *, uint64_t track_id, bool enabled);
ErikaStatus erika_presenter_set_danmaku_track_offset(ErikaPresenterHandle *, uint64_t track_id, int64_t offset_micros);
ErikaStatus erika_presenter_set_danmaku_global_offset(ErikaPresenterHandle *, int64_t offset_micros);
ErikaStatus erika_presenter_danmaku_tracks(ErikaPresenterHandle *, ErikaDanmakuTrackInfo *out_tracks, uintptr_t capacity, uintptr_t *out_len);
ErikaStatus erika_presenter_clear_danmaku(ErikaPresenterHandle *);
ErikaStatus erika_presenter_set_danmaku_enabled(ErikaPresenterHandle *, bool enabled);
ErikaStatus erika_presenter_set_danmaku_config(ErikaPresenterHandle *, ErikaDanmakuConfig config);
ErikaStatus erika_presenter_set_danmaku_config_ptr(ErikaPresenterHandle *, const ErikaDanmakuConfig *config);
ErikaStatus erika_presenter_get_danmaku_config(ErikaPresenterHandle *, ErikaDanmakuConfig *out_config);
ErikaStatus erika_presenter_set_danmaku_font(ErikaPresenterHandle *, const char *family, const char *file_path);
ErikaStatus erika_presenter_set_danmaku_block_words_json(ErikaPresenterHandle *, const char *json);
```

`load_danmaku_*` は弾幕を 1 つの匿名トラックで置き換え、`add_danmaku_track_*` は
マルチトラックリスト（各トラックに名前と時間オフセット）を構築します。入力は Bilibili
XML（`*_file`、パス/URL）または JSON（`*_json`、インライン）。`offset_micros` は 1 つの
トラックのタイムラインをずらし、global offset は全トラックをずらします。
`set_danmaku_config` / `_ptr` は完全な `ErikaDanmakuConfig` を適用（`_ptr` 版は構造体の
値渡しを避ける）、`get_danmaku_config` で読み戻します。レイアウトエンジンは
[danmaku_architecture.md](danmaku_architecture.md) を参照。
`set_danmaku_block_words_json` はフィルタ用の文字列 JSON 配列を取ります。

### Surface とプレゼンテーション

```c
ErikaStatus erika_presenter_attach_metal_layer(ErikaPresenterHandle *, uint64_t raw_layer, uint32_t w, uint32_t h, double scale);
ErikaStatus erika_presenter_attach_wgpu_surface(ErikaPresenterHandle *, ErikaWgpuSurfaceKind kind, uint64_t raw_window, uint64_t raw_display, uint32_t w, uint32_t h, double scale);
ErikaStatus erika_presenter_attach_wgpu_surface_with_output_capabilities(ErikaPresenterHandle *, ErikaWgpuSurfaceKind kind, uint64_t raw_window, uint64_t raw_display, uint32_t w, uint32_t h, double scale, ErikaSurfaceOutputCapabilities capabilities);
ErikaStatus erika_presenter_attach_windows_hwnd(ErikaPresenterHandle *, uint64_t hwnd, uint64_t hinstance, uint32_t w, uint32_t h, double scale);
ErikaStatus erika_presenter_resize_surface(ErikaPresenterHandle *, uint32_t w, uint32_t h, double scale);
ErikaStatus erika_presenter_detach_surface(ErikaPresenterHandle *);
```

macOS/iOS は `attach_metal_layer`（`CAMetalLayer*`）、Windows は `attach_windows_hwnd`
（`attach_wgpu_surface` + kind `WindowsHwnd` の便利ラッパで、`HWND` + `HINSTANCE` を渡す）。
surface に紐づくレンダラ backend（native Metal、native Direct3D 11、wgpu）は attach 呼び
出しではなく presenter 設定で決まります。drawable サイズや scale が変わったら
`resize_surface` を呼びます。

Android extended-linear では Flutter Hybrid Composition `SurfaceView` の
`AndroidNativeWindow` を `_with_output_capabilities` に渡します。display/surface HDR probe
成功時だけ `extended_linear` を、direct `SurfaceView` の場合だけ
`direct_composition = true` を設定し、host probe failure は `fallback_reason` に保持します。
`desired_headroom = 0` は system auto、正の値は surface ceiling で、API 35 per-`SurfaceView`
`setDesiredHdrHeadroom` に使用できます。
Erika は Vulkan、`Rgba16Float`、`ADATASPACE_SCRGB_LINEAR` も検証します。どれかが失敗
すると SDR に fallback し、その理由を取得できます。

### レンダーループとイベント

```c
ErikaStatus erika_presenter_render_tick(ErikaPresenterHandle *, double time_seconds, ErikaPresenterStats *out_stats);
ErikaStatus erika_presenter_poll_event(ErikaPresenterHandle *, ErikaEvent *out_event);
```

表示フレームごとに `render_tick` を 1 回呼びます（`CADisplayLink`、`CVDisplayLink`、
Windows のフレームスケジューラなど）。`time_seconds` はそのフレームのホスト表示クロック
（秒）——Erika は vsync 量子化スケジューリングに使うので、wall-clock 差分ではなく
**プレゼンテーションタイムスタンプ**を渡します。`out_stats` が非 `NULL` なら、パイプ
ラインカウンタのスナップショットが書き込まれます。`poll_event` は非ブロッキングで、
アイドル時は `NoEvent` を返します。

### 診断とスクリーンショット

```c
ErikaStatus erika_presenter_get_upscaler_status(ErikaPresenterHandle *, ErikaUpscalerStatus *out_status);
ErikaStatus erika_presenter_get_output_status(ErikaPresenterHandle *, ErikaOutputStatus *out_status);
ErikaStatus erika_presenter_capture_frame_rgba(ErikaPresenterHandle *, uint32_t width, uint32_t height,
                                               uint8_t *out_rgba, uintptr_t out_capacity);
```

`get_upscaler_status` は要求アップスケーラモード、現在の backend（off / inactive /
building / scalar / simdgroup-matrix）、フォールバック回数、アップスケール済みフレーム数、
直近の encode/GPU 時間（マイクロ秒）を報告します。

`get_output_status` は request だけでなく実際に negotiated された output を返します。
13 field は次のとおりです。

| Field | 意味 |
|-------|------|
| `requested_mode` | 作成時に要求した `ErikaPresenterOutputMode`。 |
| `active_encoding` | 実際の `SdrSrgb`、`AppleEdr`、`AndroidExtendedLinearScRgb`、`Hdr10Pq` encoding。 |
| `surface_format` | 実際の 8-bit UNORM、10-bit UNORM、16-bit float surface class。 |
| `native_data_space` | Android `ANativeWindow` dataspace。`406913024` は `SCRGB_LINEAR`、`-1` は unavailable/not applicable。 |
| `requested_headroom` | sanitize 済み requested content-headroom ceiling。最小 `1.0`。 |
| `active_headroom` | known なら current display HDR/SDR ratio、unknown なら effective-content fallback value。 |
| `active_headroom_known` | `active_headroom` が authoritative platform ratio 由来か。API 34+ ratio available なら Android は true。 |
| `extended_linear_active` | FP extended-linear presentation path が active か。Apple EDR と Android scRGB は `active_encoding` で区別。 |
| `fallback_reason` | requested mode が active でない理由を示す stable `ErikaOutputFallbackReason`。 |
| `fallback_count` | 記録された output fallback transition/failure の回数。 |
| `data_space_failures` | dataspace/output color-space validation failure 回数。 |
| `headroom_updates` | runtime headroom state の実変化回数。duplicate ratio/known publish では増えない。 |
| `extended_linear_frames` | active extended-linear path で present した frame 数。 |

Fallback value は ABI-stable です。新しい reason は末尾へ追加し、`0..8` を renumber しません。

| Code | Enum | Stable label | 意味 |
|------|------|--------------|------|
| 0 | `None` | `none` | fallback なし。 |
| 1 | `DisplayHdrUnsupported` | `display_hdr_unsupported` | display/surface HDR capability probe failure。 |
| 2 | `HybridCompositionRequired` | `hybrid_composition_required` | Android surface が direct `SurfaceView` composition ではない。 |
| 3 | `WgpuBackendNotVulkan` | `wgpu_backend_not_vulkan` | active wgpu backend が Vulkan ではない（例：GLES）。 |
| 4 | `Rgba16FloatSurfaceFormatUnavailable` | `rgba16float_surface_format_unavailable` | surface capabilities に `Rgba16Float` がない。 |
| 5 | `NativeWindowDataSpaceApiUnavailable` | `native_window_dataspace_api_unavailable` | `ANativeWindow_*DataSpace` API がない（API 26/27 を含む）。 |
| 6 | `ScrgbDataSpaceVerificationFailed` | `scrgb_dataspace_verification_failed` | `SCRGB_LINEAR` set/readback verification failure。 |
| 7 | `SurfaceConfigureFailed` | `surface_configure_failed` | requested output surface configure failure。 |
| 8 | `LegacyAppleEdrUnsupported` | `legacy_apple_edr_unsupported` | Apple EDR 未実装 backend でこの mode を要求。 |

`capture_frame_rgba` は**スクリーンショット**です。現在の合成フレーム（映像 + 字幕 +
弾幕）を、要求した `width`×`height`（表示 surface サイズとは独立）で呼び出し側確保の
RGBA8 バッファにオフスクリーン描画します。`out_capacity` は少なくとも `width*height*4`。
フレームがまだ無いときは `PlayerError` を返します。Metal と wgpu（Android を含む）は
capture を実装済みで、現在の D3D11 backend は未実装です。capture は常に SDR RGBA8
offscreen target を使い、HDR/extended-linear content を tone-map します。したがって display
output が Apple EDR、HDR10、Android extended-linear scRGB の場合も返す byte は SDR です。

```c
uint32_t w = 1920, h = 1080;
uint8_t *rgba = malloc((size_t)w * h * 4);
if (erika_presenter_capture_frame_rgba(p, w, h, rgba, (uintptr_t)w * h * 4) == ErikaStatus_Ok) {
    /* rgba は w*h の密に詰まった RGBA8 ピクセル——PNG へエンコードなど */
}
free(rgba);
```

## 列挙

| 列挙 | 値 |
|------|----|
| `ErikaState` | `Idle` `Opening` `Ready` `Playing` `Paused` `Stopped` `Closed` `Error` |
| `ErikaEventKind` | `None` `StateChanged` `DurationChanged` `PositionChanged` `TracksChanged` `BufferingChanged` `VideoParamsChanged` `SurfaceAttached` `SurfaceDetached` `Error` `TrackSelectionChanged` |
| `ErikaTrackKind` | `Video` `Audio` `Subtitle` |
| `ErikaTrackSource` | `Embedded` `External` |
| `ErikaWgpuSurfaceKind` | `Unknown` `MacOsNsView` `MacOsCaMetalLayer` `IosUiView` `WindowsHwnd` `XlibWindow` `WaylandSurface` `AndroidNativeWindow` |
| `ErikaFlutterTextureKind` | `Unknown` `MacOsTextureRegistrar` `IosTextureRegistrar` `AndroidSurfaceTexture` `WindowsTextureRegistrar` `LinuxTextureRegistrar` |
| `ErikaPresenterOutputMode` | `Sdr` `AppleEdr` `ExtendedLinear` |
| `ErikaActiveOutputEncoding` | `SdrSrgb` `AppleEdr` `AndroidExtendedLinearScRgb` `Hdr10Pq` |
| `ErikaOutputSurfaceFormat` | `EightBitUnorm` `TenBitUnorm` `SixteenBitFloat` |
| `ErikaOutputFallbackReason` | `None` `DisplayHdrUnsupported` `HybridCompositionRequired` `WgpuBackendNotVulkan` `Rgba16FloatSurfaceFormatUnavailable` `NativeWindowDataSpaceApiUnavailable` `ScrgbDataSpaceVerificationFailed` `SurfaceConfigureFailed` `LegacyAppleEdrUnsupported` |
| `ErikaLumaUpscalerMode` | `Off` `ArtCnnC4F16` `ArtCnnC4F32` |
| `ErikaUpscalerBackendStatus` | `Off` `Inactive` `Building` `Scalar` `SimdgroupMatrix` |

## 構造体

- **`ErikaPresenterConfig`** `{ int32 output_mode; float edr_headroom; int32 luma_upscaler; }` —
  `create_with_config` に値で渡す。
- **`ErikaSurfaceOutputCapabilities`** `{ bool extended_linear; bool direct_composition; float desired_headroom; int32 fallback_reason; }` —— attach 時に渡す Android host の display/surface probe result。`desired_headroom == 0` は system auto。
- **`ErikaUpscalerStatus`** —— 要求モード、現在の backend、フォールバック回数、
  アップスケール済みフレーム数、直近の encode/GPU マイクロ秒。
- **`ErikaOutputStatus`** —— 上記「診断とスクリーンショット」の 13-field negotiated
  output snapshot。
- **`ErikaDanmakuConfig`** —— 弾幕レイアウト/外観の全設定（フォントサイズ、不透明度、
  表示領域、スクロールタイミング、衝突/スタックフラグ、ブロックモード、影スタイル）。
  `font_size` は NipaPlay/Flutter の*論理*サイズで、Erika が surface scale を掛けて
  グリフピクセルにします。
- **`ErikaDanmakuTrackInfo`** `{ id, enabled, offset_micros, item_count, char *name, char *source }` —
  `erika_danmaku_track_info_free` で解放。
- **`ErikaVideoParams`** `{ width, height, primaries, transfer }` —— `VideoParamsChanged`
  で通知される色メタデータ。
- **`ErikaTrackCounts`** / **`ErikaTrackSelection`** —— 種別ごとの件数 / 選択 id
  （`-1` = 無し）。
- **`ErikaTrackInfo`** —— トラックごとの全メタデータ。6 つの `char*` フィールドは
  呼び出し側の所有（`erika_track_info_free` で解放）。
- **`ErikaEvent`** —— 構造体による tagged union：`kind` がどのフィールドが有効かを選ぶ
  （`state`、`duration_micros`、`position_micros`、`buffering`、`video`、`tracks`）。
  `Error` イベントは `status` がコードを運ぶ。
- **`ErikaPresenterStats`** —— パイプラインカウンタ：デコード/描画フレーム数、push した
  オーディオフレーム、overlay/弾幕フレーム、ハード vs ソフト vs ゼロコピーフレーム数、
  HDR ソース/HDR10 出力/SDR トーンマップ数、オーディオクロックの read/queued/underflow
  フレーム数、直近の描画時間。

## イベント

ループの各反復でポーリングし `kind` で分岐します。

```c
ErikaEvent ev;
while (erika_presenter_poll_event(p, &ev) == ErikaStatus_Ok) {
    switch (ev.kind) {
        case ErikaEventKind_StateChanged:    /* ev.state */                 break;
        case ErikaEventKind_DurationChanged: /* ev.duration_micros */        break;
        case ErikaEventKind_PositionChanged: /* ev.position_micros */        break;
        case ErikaEventKind_TracksChanged:   /* erika_*_tracks を再取得 */    break;
        case ErikaEventKind_BufferingChanged:/* ev.buffering */              break;
        case ErikaEventKind_VideoParamsChanged: /* ev.video */               break;
        case ErikaEventKind_Error:           /* ev.status + last_error */    break;
        default: break;
    }
}
```

イベントキューは有界で、ポーリングでドレインします。ポーリングをやめたホストは状態の
観測をやめるだけです。`position_micros` は再生中に周期的に発行されます。

## 最小 presenter 組み込み（C）

```c
#include "erika.h"

ErikaPresenterHandle *p = erika_presenter_create();
erika_presenter_attach_metal_layer(p, (uint64_t)layer, w, h, scale);  // または attach_windows_hwnd
if (erika_presenter_open(p, "/path/to/video.mkv") != ErikaStatus_Ok) {
    char *m = erika_last_error_message(); /* ログ */ erika_string_free(m);
}
erika_presenter_play(p);

// 表示フレームごと:
ErikaPresenterStats stats;
erika_presenter_render_tick(p, host_time_seconds, &stats);
ErikaEvent ev;
while (erika_presenter_poll_event(p, &ev) == ErikaStatus_Ok) { /* 分岐 */ }

// 解放:
erika_presenter_detach_surface(p);
erika_presenter_destroy(p);
```

プラットフォームごとの surface と表示タイマーの詳細は
[integration.ja.md](integration.ja.md)、実行可能な
[`macos_native_demo`](../examples/macos_native_demo) /
[`windows_native_demo`](../examples/windows_native_demo) の例も参照してください。
