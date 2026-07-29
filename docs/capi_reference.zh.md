# Erika C ABI 参考手册

本文档描述 `erika_capi` 导出的稳定 C ABI，声明在
[`crates/erika_capi/include/erika.h`](../crates/erika_capi/include/erika.h)。该
ABI 是所有非 Rust 宿主（C、C++、Swift、Dart FFI、Win32……）的唯一接入面。Rust
嵌入方应直接使用 `erika` crate；本层仅为 FFI 而存在。

嵌入流程（surface attach、渲染循环、释放）见 [integration.zh.md](integration.zh.md)。
高层引擎设计见 [architecture.zh.md](architecture.zh.md)。

> 英文版：[capi_reference.md](capi_reference.md)。

## 两个 handle 族

Erika 暴露两个相互独立的入口，每个集成选其一。

| Handle | 模型 | 谁渲染 | 适用 |
|--------|------|--------|------|
| `ErikaHandle` | **拉取（Pull）** | 宿主 | 你拥有渲染循环、自己拉取解码帧 / 驱动自己的合成器。 |
| `ErikaPresenterHandle` | **推送（Push）** | Erika | 你把一个原生 surface 交给 Erika，每个显示帧调一次 `render_tick`。Erika 负责解码、时序、音频、overlay 和呈现。 |

`ErikaPresenterHandle` 是推荐路径，也是 Flutter 插件和原生 demo 所用的。它只在
**macOS、iOS、Windows、Android** 上编译。其它平台上 `erika_presenter_create` 仍然
导出但返回 `NULL`，presenter 族的其余函数不存在——请按平台守卫 presenter 用法。

两个族不共享状态；一个进程可同时使用两者，但单个媒体会话只活在一个 handle 里。

## 约定

### 状态码

每个可失败的调用都返回 `ErikaStatus`：

| 值 | 码 | 含义 |
|----|----|------|
| `ErikaStatus_Ok` | 0 | 成功。清空线程局部错误。 |
| `ErikaStatus_NullPointer` | 1 | 必需的 handle 或 out 指针为 `NULL`（或 surface 指针为 0）。 |
| `ErikaStatus_InvalidUtf8` | 2 | 某个 `const char*` 参数不是合法 UTF-8。 |
| `ErikaStatus_PlayerError` | 3 | 引擎拒绝该调用；读取错误信息（见下）。 |
| `ErikaStatus_Panic` | 4 | 在边界处捕获到 Rust panic。该调用除 panic 前已完成的部分外无效；handle 应视为可疑状态。 |
| `ErikaStatus_NoEvent` | 5 | 仅 `*_poll_event`：队列为空（非错误）。 |

务必检查返回值。`Ok` 和 `NoEvent` 是仅有的非错误结果。

### panic 安全

ABI 永不让 unwind 穿越 FFI 边界。每个入口都把函数体包在 `catch_unwind` 里；panic
会变成 `ErikaStatus_Panic` 并设置错误信息。你可以跨边界调用，无需担心 C++
`noexcept`/SEH。

### 错误信息（线程局部）

任何非 `Ok`/`NoEvent` 的结果，Erika 会把一条可读信息存进**线程局部**槽。读取：

```c
char *msg = erika_last_error_message();   // 堆分配，可能为 NULL
if (msg) { fprintf(stderr, "erika: %s\n", msg); erika_string_free(msg); }
```

因为是线程局部的，要**在发起失败调用的同一线程**上读取，且在该线程下一次调用之前读取
（之后一次 `Ok` 会清空它）。`erika_last_error_message` 返回的是你拥有的副本——用
`erika_string_free` 释放。

### 字符串所有权

Erika 返回的任何 `char*` 都是堆分配、归调用方所有：

- 独立字符串（如 `erika_last_error_message`）→ 用 `erika_string_free` 释放。
- 内嵌在 `ErikaTrackInfo` 中的字符串 → 用 `erika_track_info_free(&track)`
  释放整条记录（会释放每个内部字符串）。
- 内嵌在 `ErikaDanmakuTrackInfo` 中的字符串 → 用
  `erika_danmaku_track_info_free(&track)` 释放。

切勿用 libc `free()` 释放这些；始终用配套的 Erika 释放函数，使分配在同一 allocator
上跨越 ABI。

你传入的 `const char*` 参数仅在调用期间被借用；Erika 会复制所需内容。它们必须是
NUL 结尾的 UTF-8。

### 计数数组惯用法

列表 getter（`erika_tracks`、`erika_presenter_tracks`、
`erika_presenter_danmaku_tracks`）使用调用方分配的缓冲：

```c
size_t total = 0;
erika_presenter_tracks(p, NULL, 0, &total);          // 1) 查询数量
ErikaTrackInfo *buf = calloc(total, sizeof *buf);
erika_presenter_tracks(p, buf, total, &total);        // 2) 填充
for (size_t i = 0; i < total; i++) { /* 使用 buf[i] */ }
for (size_t i = 0; i < total; i++) erika_track_info_free(&buf[i]);
free(buf);
```

`out_len` **始终**被设为可用记录的总数。最多写入 `capacity` 条；传
`capacity == 0`（配 `NULL` 缓冲）是受支持的"问数量"方式。只有实际写入的记录才持有
需要释放的字符串。

### Surface 几何与 scale

`attach_*` 和 `resize_surface` 的 `width`、`height` 单位是**物理像素**，`scale`
是 backing/DPI 因子（如 Retina 的 `2.0`、Windows 的显示器缩放）。surface 指针为 `0`
会被拒绝并返回 `NullPointer`。

### 线程

单个 handle **没有内部同步**。不要从多个线程并发调用同一个 handle；请自行串行化
（或把 handle 限定在单线程内）。presenter 的 `render_tick` 应从拥有显示定时器 /
surface 的线程驱动。不同 handle 在不同线程上互相独立。记住错误信息是线程局部的。

## `ErikaHandle` —— 拉取模型

宿主自己驱动渲染，并拉取状态/事件。

### 生命周期

```c
ErikaHandle *erika_create(void);
void         erika_destroy(ErikaHandle *handle);
char        *erika_last_error_message(void);   // 线程局部，调用方释放
void         erika_string_free(char *value);
```

`erika_create` 永不失败（返回合法 handle）。`erika_destroy(NULL)` 是 no-op。销毁
handle 会停止播放并释放全部资源。

### 播放控制

```c
ErikaStatus erika_open(ErikaHandle *handle, const char *uri);   // 文件路径或 URL
ErikaStatus erika_open_with_headers(ErikaHandle *handle, const char *uri,
                                    const ErikaHttpHeader *headers, uintptr_t header_count);
ErikaStatus erika_play(ErikaHandle *handle);
ErikaStatus erika_pause(ErikaHandle *handle);
ErikaStatus erika_stop(ErikaHandle *handle);
ErikaStatus erika_close(ErikaHandle *handle);
ErikaStatus erika_seek(ErikaHandle *handle, uint64_t position_micros);
```

`uri` 是本地文件路径或 HTTP(S) URL。`erika_open_with_headers` 用于为 HTTP(S) 播放
设置请求头；`headers` 只在调用期间读取，调用返回后即可释放。`header_count` 大于零时
`headers` 不能为 NULL。请求头会用于 HEAD、Range GET 和预取请求。
认证信息和 Cookie 不会写入 Erika 日志。`seek` 单位为微秒。`open` 和 `play` 都会
异步入队；应观察 `StateChanged`、`DurationChanged` 和 `Error` 事件获取最终结果，
不要阻塞宿主 UI 线程。

### 轨道与字幕

```c
ErikaStatus erika_add_external_subtitle(ErikaHandle *, const char *uri, int64_t *out_track_id);
ErikaStatus erika_remove_subtitle_track(ErikaHandle *, int64_t track_id);
ErikaStatus erika_select_audio_track(ErikaHandle *, int64_t track_id);
ErikaStatus erika_select_subtitle_track(ErikaHandle *, int64_t track_id);
ErikaStatus erika_track_selection(ErikaHandle *, ErikaTrackSelection *out_selection);
ErikaStatus erika_tracks(ErikaHandle *, ErikaTrackInfo *out_tracks, uintptr_t capacity, uintptr_t *out_len);
void        erika_track_info_free(ErikaTrackInfo *track);
```

`erika_tracks` 遵循计数数组惯用法。`erika_track_selection` 报告当前选中的
视频/音频/字幕轨 id（`-1` 表示无）。选择字幕轨 id `-1` 即关闭字幕。

### 状态与事件

```c
ErikaStatus erika_state(ErikaHandle *, ErikaState *out_state);
ErikaStatus erika_poll_event(ErikaHandle *, ErikaEvent *out_event);
```

`erika_poll_event` 非阻塞：队列空时返回 `NoEvent`。在循环里把它抽干。见
[事件](#事件)。

### Surface attach（宿主管理）

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

`raw_layer` 是 `CAMetalLayer*` 转成的 `uint64_t`。对 `erika_attach_wgpu_surface`，
`raw_window`/`raw_display` 是该 `kind` 对应的平台窗口/显示句柄（如 `WindowsHwnd`
的 `HWND` + `HINSTANCE`，`XlibWindow` 的 xcb/Xlib window + display）。
`erika_attach_flutter_texture` 把一个外部纹理 id 注册进平台 texture registrar。
Android 原生宿主若要声明直接合成、具备 HDR eligibility 的 `SurfaceView`，必须使用
`_with_output_capabilities` 变体；短函数传入全 false/default capabilities，因此不能激活
Android extended-linear 输出。

## `ErikaPresenterHandle` —— 推送模型

Erika 拥有完整栈；宿主提供 surface 并调用 `render_tick`。
**macOS / iOS / Windows / Android。**

### 生命周期与配置

```c
ErikaPresenterHandle *erika_presenter_create(void);
ErikaPresenterHandle *erika_presenter_create_with_config(ErikaPresenterConfig config);
ErikaPresenterHandle *erika_presenter_create_with_output_mode(int32_t output_mode, float edr_headroom);
void                  erika_presenter_destroy(ErikaPresenterHandle *handle);
```

`ErikaPresenterConfig` 选择输出模式（`Sdr`、Apple `AppleEdr`、Android
`ExtendedLinear`）、请求的 EDR/scRGB 内容 headroom 上限和初始亮度超分。Android
`ExtendedLinear` 是 FP16 extended-linear scRGB，不是 HDR10/PQ。
`create_with_output_mode` 是简写；`create` 用默认值（SDR、无超分）。返回 `NULL` 表示
创建失败——检查 `erika_last_error_message`。

### 播放与运行时参数

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
ErikaStatus erika_presenter_set_output_headroom(ErikaPresenterHandle *, float headroom, bool known);
```

`set_playback_rate(1.0)` 为正常速度。`set_upscaler` 在运行时切换神经亮度超分（见
[`erika_presenter_get_upscaler_status`](#诊断与截图)）。Metal 与具备 compute 能力的
wgpu/Vulkan renderer 会执行 ArtCNN；其他后端保留原生 luma sampling，并明确报告
`Inactive` 回退。

`ErikaHttpHeader` 定义如下：

```c
typedef struct ErikaHttpHeader {
  const char *name;
  const char *value;
} ErikaHttpHeader;
```

例如：

```c
ErikaHttpHeader headers[] = {
    {"Authorization", "Bearer token"},
    {"Referer", "https://example.com/"},
};
erika_presenter_open_with_headers(presenter, "https://example.com/video.mp4",
                                  headers, 2);
```

请求头仅对 HTTP(S) source 生效；本地文件和 Android `content://` source 会忽略它们。
调用方负责保证 URI、header 名称和值在调用期间保持有效，接口不会接管这些字符串的所有权。

播放器自己生成的请求头会被拒绝而不是合并——底层 HTTP client 是追加而非替换重复的
header：`Range`、`Host`、`Content-Length`、`Transfer-Encoding`、`Connection`
（大小写不敏感匹配）都会让调用返回 `ERIKA_STATUS_PLAYER_ERROR`。不符合 HTTP token
规则的 header 名，或包含非法字符的 header 值，同样会失败——校验发生在 `open`，
而不是第一次 range 请求时。

请求头只作用于媒体 source。外挂字幕轨道和弹幕 sidecar 文件仍然不带这些请求头拉取，
因此能通过视频认证的 token 目前无法用于这些 URL。

`set_output_headroom` 用于发布显示器当前 HDR/SDR ratio。Android API 34+ 宿主应从
`Display.registerHdrSdrRatioChangedListener` 调用它：有效 ratio 传 `known = true`，测量
不可用时传 `(1.0f, false)`。数值会清洗到 `1.0..10000.0`。wgpu 会忽略重复状态、无需
重新 attach surface 即更新后续帧 target，并且只有 known 状态或 ratio 真实变化时才增加
`headroom_updates`。有效内容 target 受配置的内容上限、正数 surface
`desired_headroom` 和已知显示 ratio 共同约束。其他 renderer 可以忽略这个 advisory 更新。

### 轨道与字幕

```c
ErikaStatus erika_presenter_add_external_subtitle(ErikaPresenterHandle *, const char *uri, int64_t *out_track_id);
ErikaStatus erika_presenter_remove_subtitle_track(ErikaPresenterHandle *, int64_t track_id);
ErikaStatus erika_presenter_select_audio_track(ErikaPresenterHandle *, int64_t track_id);
ErikaStatus erika_presenter_select_subtitle_track(ErikaPresenterHandle *, int64_t track_id);
ErikaStatus erika_presenter_track_selection(ErikaPresenterHandle *, ErikaTrackSelection *out_selection);
ErikaStatus erika_presenter_tracks(ErikaPresenterHandle *, ErikaTrackInfo *out_tracks, uintptr_t capacity, uintptr_t *out_len);
```

语义同 `ErikaHandle` 的轨道函数。

### 弹幕

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

`load_danmaku_*` 用单条匿名轨替换当前弹幕；`add_danmaku_track_*` 构建多轨列表
（每轨带名字和时间偏移）。输入为 Bilibili XML（`*_file`，按路径/URL）或 JSON
（`*_json`，内联）。`offset_micros` 偏移某条轨的时间线；全局 offset 偏移所有轨。
`set_danmaku_config` / `_ptr` 应用完整 `ErikaDanmakuConfig`（`_ptr` 变体避免按值
传结构体）；`get_danmaku_config` 读回。布局引擎见
[danmaku_architecture.md](danmaku_architecture.md)。`set_danmaku_block_words_json`
接受一个字符串 JSON 数组用于过滤。

### Surface 与呈现

```c
ErikaStatus erika_presenter_attach_metal_layer(ErikaPresenterHandle *, uint64_t raw_layer, uint32_t w, uint32_t h, double scale);
ErikaStatus erika_presenter_attach_wgpu_surface(ErikaPresenterHandle *, ErikaWgpuSurfaceKind kind, uint64_t raw_window, uint64_t raw_display, uint32_t w, uint32_t h, double scale);
ErikaStatus erika_presenter_attach_wgpu_surface_with_output_capabilities(ErikaPresenterHandle *, ErikaWgpuSurfaceKind kind, uint64_t raw_window, uint64_t raw_display, uint32_t w, uint32_t h, double scale, ErikaSurfaceOutputCapabilities capabilities);
ErikaStatus erika_presenter_attach_windows_hwnd(ErikaPresenterHandle *, uint64_t hwnd, uint64_t hinstance, uint32_t w, uint32_t h, double scale);
ErikaStatus erika_presenter_resize_surface(ErikaPresenterHandle *, uint32_t w, uint32_t h, double scale);
ErikaStatus erika_presenter_detach_surface(ErikaPresenterHandle *);
```

macOS/iOS 用 `attach_metal_layer`（`CAMetalLayer*`），Windows 用
`attach_windows_hwnd`（它是 `attach_wgpu_surface` + kind `WindowsHwnd` 的便捷封装，
传 `HWND` + `HINSTANCE`）。绑定到 surface 的渲染后端（原生 Metal、原生 Direct3D 11
或 wgpu）由 presenter 配置决定，而非 attach 调用决定。drawable 尺寸或 scale 变化时
调 `resize_surface`。

Android extended-linear 应把 Flutter Hybrid Composition `SurfaceView` 对应的
`AndroidNativeWindow` 传给 `_with_output_capabilities`。只有在显示器/surface HDR 探测成功
后才设置 `extended_linear`，只有直接 `SurfaceView` 才设置
`direct_composition = true`，并把宿主探测失败保留在 `fallback_reason`。
`desired_headroom = 0` 表示系统 auto；正数是 surface 上限，可用于 API 35 per-`SurfaceView`
`setDesiredHdrHeadroom`。Erika 仍会自行
验证 Vulkan、`Rgba16Float` 和 `ADATASPACE_SCRGB_LINEAR`；任何一项失败都会回退 SDR，
且原因可查询。

### 渲染循环与事件

```c
ErikaStatus erika_presenter_render_tick(ErikaPresenterHandle *, double time_seconds, ErikaPresenterStats *out_stats);
ErikaStatus erika_presenter_get_stats(ErikaPresenterHandle *, ErikaPresenterStats *out_stats);
ErikaStatus erika_presenter_poll_event(ErikaPresenterHandle *, ErikaEvent *out_event);
```

每个显示帧调一次 `render_tick`（如来自 `CADisplayLink`、`CVDisplayLink` 或 Windows
帧调度器）。`time_seconds` 是该帧的宿主显示时钟（秒）——Erika 用它做 vsync 量化调度，
所以传**呈现时间戳**，不是 wall-clock 增量。若 `out_stats` 非 `NULL`，会填入流水线
计数器快照。`poll_event` 非阻塞，空闲时返回 `NoEvent`。

`get_stats` 填入同样的 `ErikaPresenterStats` 快照，但不渲染帧。当宿主采样计数器的
节奏与显示循环不同时用它；它不推进呈现。

### JSON 桥

对于平台通道本身就在序列化结构化参数的嵌入方（例如 HarmonyOS ArkTS 插件），
同一套 presenter 接口也以 JSON 形式提供：

```c
char *erika_presenter_invoke_json(ErikaPresenterHandle *, const char *method,
                                  const char *arguments_json);
char *erika_presenter_render_tick_json(ErikaPresenterHandle *, double time_seconds);
char *erika_presenter_poll_event_json(ErikaPresenterHandle *);
```

返回的字符串归 Erika 所有，必须用 `erika_string_free` 释放。`poll_event_json`
在无事件时返回 `NULL`——不是信封——所以 `NULL` 表示空闲而非错误。

三者的返回值都包在同一层信封里：

```json
{ "ok": true,  "status": 0, "value": <result> }
{ "ok": false, "status": 1, "error": "<message>" }
```

`arguments_json` 必须是 JSON 对象。`method` 选择操作，与 C 入口一一对应：
`open`、`play`、`pause`、`stop`、`close`、`seek`、`setPlaybackRate`、`setVolume`、
`setUpscaler`、`setSubtitleScale`、`getUpscalerStatus`、`getOutputStatus`、
`getPresenterStats`、`tracks`、`addExternalSubtitle`、`removeSubtitleTrack`、
`selectAudioTrack`、`selectSubtitleTrack`，以及弹幕系列（`loadDanmakuFile`、
`loadDanmakuJson`、`addDanmakuTrackFile`、`addDanmakuTrackJson`、
`removeDanmakuTrack`、`setDanmakuTrackEnabled`、`setDanmakuTrackOffset`、
`setDanmakuGlobalOffset`、`danmakuTracks`、`clearDanmaku`、`setDanmakuEnabled`、
`setDanmakuConfig`）。未知 method 返回 `ok: false`，不会中止。权威分发表见
`crates/erika_capi/src/presenter_json.rs`。

这层桥只是便利封装，不是第二套 API：它调用的就是上面这些函数，没有额外能力。
能在平台通道上传结构体的宿主应优先用类型化入口。

### 诊断与截图

```c
ErikaStatus erika_presenter_get_upscaler_status(ErikaPresenterHandle *, ErikaUpscalerStatus *out_status);
ErikaStatus erika_presenter_get_output_status(ErikaPresenterHandle *, ErikaOutputStatus *out_status);
ErikaStatus erika_presenter_capture_frame_rgba(ErikaPresenterHandle *, uint32_t width, uint32_t height,
                                               uint8_t *out_rgba, uintptr_t out_capacity);
```

`get_upscaler_status` 报告请求的超分模式、当前后端（off / inactive / building /
scalar / simdgroup-matrix）、fallback 次数、超分帧数，以及最近的 encode/GPU 耗时
（微秒）。

`get_output_status` 返回实际协商到的输出，而不只是请求值。13 个字段如下：

| 字段 | 含义 |
|------|------|
| `requested_mode` | 创建时请求的 `ErikaPresenterOutputMode`。 |
| `active_encoding` | 实际的 `SdrSrgb`、`AppleEdr`、`AndroidExtendedLinearScRgb` 或 `Hdr10Pq` encoding。 |
| `surface_format` | 实际 8-bit UNORM、10-bit UNORM 或 16-bit float surface class。 |
| `native_data_space` | Android `ANativeWindow` dataspace；`406913024` 为 `SCRGB_LINEAR`，`-1` 表示不可用/不适用。 |
| `requested_headroom` | 清洗后的请求内容 headroom 上限，最小 `1.0`。 |
| `active_headroom` | 已知时为当前显示器 HDR/SDR ratio；未知时为有效内容 fallback 值。 |
| `active_headroom_known` | `active_headroom` 是否来自权威平台 ratio；API 34+ ratio 可用时 Android 设为 true。 |
| `extended_linear_active` | FP extended-linear 呈现路径是否激活；用 `active_encoding` 区分 Apple EDR 与 Android scRGB。 |
| `fallback_reason` | 解释请求模式为何未激活的稳定 `ErikaOutputFallbackReason`。 |
| `fallback_count` | 已记录的输出回退 transition/failure 次数。 |
| `data_space_failures` | dataspace/输出色彩空间验证失败次数。 |
| `headroom_updates` | 运行时 headroom 状态真实变化次数；重复发布相同 ratio/known 不增长。 |
| `extended_linear_frames` | 通过 active extended-linear 路径呈现的帧数。 |

Fallback 数值是稳定 ABI；新增原因只能追加，不能重排 `0..8`：

| 码 | 枚举 | 稳定 label | 含义 |
|----|------|------------|------|
| 0 | `None` | `none` | 无回退。 |
| 1 | `DisplayHdrUnsupported` | `display_hdr_unsupported` | 显示器/surface HDR 能力探测失败。 |
| 2 | `HybridCompositionRequired` | `hybrid_composition_required` | Android surface 不是直接合成的 `SurfaceView`。 |
| 3 | `WgpuBackendNotVulkan` | `wgpu_backend_not_vulkan` | 当前 wgpu backend 不是 Vulkan（如 GLES）。 |
| 4 | `Rgba16FloatSurfaceFormatUnavailable` | `rgba16float_surface_format_unavailable` | Surface capabilities 没有 `Rgba16Float`。 |
| 5 | `NativeWindowDataSpaceApiUnavailable` | `native_window_dataspace_api_unavailable` | `ANativeWindow_*DataSpace` API 不可用（含 API 26/27）。 |
| 6 | `ScrgbDataSpaceVerificationFailed` | `scrgb_dataspace_verification_failed` | `SCRGB_LINEAR` set/readback 未通过验证。 |
| 7 | `SurfaceConfigureFailed` | `surface_configure_failed` | 请求的输出 surface configure 失败。 |
| 8 | `LegacyAppleEdrUnsupported` | `legacy_apple_edr_unsupported` | 在未实现 Apple EDR 的 backend 上请求了该模式。 |

`capture_frame_rgba` 是**截图**：把当前合成帧（视频 + 字幕 + 弹幕）离屏渲染进调用方
分配的 RGBA8 缓冲，按请求的 `width`×`height`（与显示 surface 尺寸无关）。
`out_capacity` 至少为 `width*height*4`。尚无可用帧时返回 `PlayerError`。Metal 与
wgpu（包括 Android）已实现截图；当前 D3D11 backend 尚未实现。截图始终使用 SDR RGBA8
离屏 target，并对 HDR/extended-linear 内容 tone-map，因此即使显示输出是 Apple EDR、
HDR10 或 Android extended-linear scRGB，返回字节仍是 SDR。

```c
uint32_t w = 1920, h = 1080;
uint8_t *rgba = malloc((size_t)w * h * 4);
if (erika_presenter_capture_frame_rgba(p, w, h, rgba, (uintptr_t)w * h * 4) == ErikaStatus_Ok) {
    /* rgba 为 w*h 紧排的 RGBA8 像素——可编码为 PNG 等 */
}
free(rgba);
```

## 枚举

| 枚举 | 取值 |
|------|------|
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

## 结构体

- **`ErikaPresenterConfig`** `{ int32 output_mode; float edr_headroom; int32 luma_upscaler; }` —
  按值传给 `create_with_config`。
- **`ErikaSurfaceOutputCapabilities`** `{ bool extended_linear; bool direct_composition; float desired_headroom; int32 fallback_reason; }` —— attach 时传入的 Android 宿主显示器/surface 探测结果；`desired_headroom == 0` 表示系统 auto。
- **`ErikaUpscalerStatus`** —— 请求模式、当前后端、fallback 次数、超分帧数、最近
  encode/GPU 微秒。
- **`ErikaOutputStatus`** —— 上文“诊断与截图”说明的 13 字段协商输出快照。
- **`ErikaDanmakuConfig`** —— 完整弹幕布局/外观配置（字号、不透明度、显示区域、滚动
  时序、碰撞/堆叠开关、屏蔽模式、阴影样式）。`font_size` 是 NipaPlay/Flutter 的*逻辑*
  字号；Erika 会乘以 surface scale 得到 glyph 像素。
- **`ErikaDanmakuTrackInfo`** `{ id, enabled, offset_micros, item_count, char *name, char *source }` —
  用 `erika_danmaku_track_info_free` 释放。
- **`ErikaVideoParams`** `{ width, height, primaries, transfer }` —— 通过
  `VideoParamsChanged` 上报的色彩元数据。
- **`ErikaTrackCounts`** / **`ErikaTrackSelection`** —— 各类轨道计数 / 选中 id
  （`-1` = 无）。
- **`ErikaTrackInfo`** —— 完整的每轨元数据；六个 `char*` 字段归调用方所有（用
  `erika_track_info_free` 释放）。
- **`ErikaEvent`** —— 用结构体表达的 tagged union：`kind` 决定哪些字段有意义
  （`state`、`duration_micros`、`position_micros`、`buffering`、`video`、`tracks`）；
  `Error` 事件由 `status` 携带状态码。
- **`ErikaPresenterStats`** —— 流水线计数器：解码/渲染帧数、推送音频帧、overlay/弹幕
  帧、硬解 vs 软解 vs 零拷贝帧数、HDR 源/HDR10 输出/SDR tonemap 计数、音频时钟
  read/queued/underflow 帧数，以及最近渲染耗时。

## 事件

每次循环迭代轮询并按 `kind` 分发：

```c
ErikaEvent ev;
while (erika_presenter_poll_event(p, &ev) == ErikaStatus_Ok) {
    switch (ev.kind) {
        case ErikaEventKind_StateChanged:    /* ev.state */                 break;
        case ErikaEventKind_DurationChanged: /* ev.duration_micros */        break;
        case ErikaEventKind_PositionChanged: /* ev.position_micros */        break;
        case ErikaEventKind_TracksChanged:   /* 重新查询 erika_*_tracks */    break;
        case ErikaEventKind_BufferingChanged:/* ev.buffering */              break;
        case ErikaEventKind_VideoParamsChanged: /* ev.video */               break;
        case ErikaEventKind_Error:           /* ev.status + last_error */    break;
        default: break;
    }
}
```

事件队列有界，靠轮询抽干；停止轮询的宿主只是停止观察状态。`position_micros` 在播放
中周期性发出。

## 最小 presenter 集成（C）

```c
#include "erika.h"

ErikaPresenterHandle *p = erika_presenter_create();
erika_presenter_attach_metal_layer(p, (uint64_t)layer, w, h, scale);  // 或 attach_windows_hwnd
if (erika_presenter_open(p, "/path/to/video.mkv") != ErikaStatus_Ok) {
    char *m = erika_last_error_message(); /* 记录 */ erika_string_free(m);
}
erika_presenter_play(p);

// 每个显示帧:
ErikaPresenterStats stats;
erika_presenter_render_tick(p, host_time_seconds, &stats);
ErikaEvent ev;
while (erika_presenter_poll_event(p, &ev) == ErikaStatus_Ok) { /* 分发 */ }

// 释放:
erika_presenter_detach_surface(p);
erika_presenter_destroy(p);
```

各平台的 surface 与显示定时器细节见 [integration.zh.md](integration.zh.md)，以及可运行的
[`macos_native_demo`](../examples/macos_native_demo) /
[`windows_native_demo`](../examples/windows_native_demo) 示例。
