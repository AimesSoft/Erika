# 在原生宿主中接入 Erika

本指南讲解如何把 Erika 嵌入非 Flutter 宿主——C/C++/Swift 应用、Win32 窗口，或任何
带 C FFI 的运行时。它使用 **presenter（推送）模型**:Erika 负责解码、时序、音频、
overlay 和呈现,宿主提供一个 surface 并每帧调一次 `render_tick`。

前置:C ABI（[capi_reference.zh.md](capi_reference.zh.md)）和一个已构建好的 Erika 库
（[building.zh.md](building.zh.md)）。Flutter 请改用
[`erika_flutter`](../packages/erika_flutter) 插件,见
[flutter_embedding.zh.md](flutter_embedding.zh.md)。

> 英文版：[integration.md](integration.md)。

两个可运行参考随本指南提供:
[`examples/macos_native_demo`](../examples/macos_native_demo)（AppKit +
`CAMetalLayer`）和 [`examples/windows_native_demo`](../examples/windows_native_demo)
（Win32 + `HWND`）。它们直接驱动 Rust 的 `PresenterRuntime`;下文的 C ABI 调用是
一一对应的等价物。

## 1. 选择 handle 族

除非你有理由自己渲染,否则用 `ErikaPresenterHandle`。拉取模型的 `ErikaHandle` 适合
拥有自己合成器、只想要 Erika 解码/时序/状态的宿主。本指南其余部分都基于 presenter。

presenter 族仅在 **macOS、iOS、Windows** 上编译。

## 2. 生命周期

```
create ──▶ attach surface ──▶ open ──▶ play ──▶ (render_tick + poll_event 循环)
                                                       │
                               pause / seek / set_* ◀──┤
                                                       ▼
                         detach surface ──▶ destroy
```

`open` 是异步的。handle 经历 `Opening → Ready → Playing`;通过事件观察跃迁,而非阻塞
等待。你可以在 `open` 前或后 attach surface,但先 attach 能让空闲测试图样 / 首帧立即
出现。

## 3. 创建 presenter

```c
ErikaPresenterConfig cfg = {
    .output_mode  = ErikaPresenterOutputMode_Sdr,   // macOS/iOS 上可用 _AppleEdr
    .edr_headroom = 1.0f,                            // 仅 Apple EDR 用到
    .luma_upscaler = ErikaLumaUpscalerMode_Off,      // 或 ArtCnnC4F16 / C4F32
};
ErikaPresenterHandle *p = erika_presenter_create_with_config(cfg);
if (!p) { /* 读取 erika_last_error_message() */ }
```

`erika_presenter_create()` 用默认值（SDR、无超分）。神经亮度超分是 Metal compute
特性;在 D3D11/wgpu 后端上 `set_upscaler` 是被接受的 no-op 回退。

## 4. Attach 一个 surface

Erika 直接渲染进你拥有的平台 surface。宽高单位是**物理像素**,`scale` 是 DPI/backing
因子。

### macOS / iOS —— `CAMetalLayer`

创建一个 `CAMetalLayer`,设好尺寸,再把它的指针交给 Erika:

```c
// `layer` 是 CAMetalLayer*（如来自你的 NSView/UIView 宿主 layer）
erika_presenter_attach_metal_layer(p, (uint64_t)(uintptr_t)layer,
                                   pixel_w, pixel_h, backing_scale);
```

macOS 上推荐的安排是一个 window-hosted layer,作为内容视图的 sibling / underlay,让
视频留在 AppKit 视图合成器之外(与 Flutter 插件同款模型,见
[flutter_embedding.zh.md](flutter_embedding.zh.md)）。

### Windows —— `HWND`

```c
HWND hwnd = /* 你的窗口 */;
HINSTANCE hinst = GetModuleHandleW(NULL);
UINT dpi = GetDpiForWindow(hwnd);
double scale = dpi ? (double)dpi / 96.0 : 1.0;
RECT rc; GetClientRect(hwnd, &rc);
uint32_t w = max(1, rc.right - rc.left), h = max(1, rc.bottom - rc.top);

erika_presenter_attach_windows_hwnd(p, (uint64_t)(uintptr_t)hwnd,
                                    (uint64_t)(uintptr_t)hinst, w, h, scale);
```

`attach_windows_hwnd` 是 `attach_wgpu_surface` + kind `WindowsHwnd` 的便捷封装。在默认
presenter 配置下,该 surface 驱动**原生 Direct3D 11** 渲染器（D3D11VA 零拷贝、HDR10）;
只有当你确实需要时才在配置里传 wgpu 回退渲染器。

### 通用 —— `attach_wgpu_surface`

对 X11/Wayland/Android,或想显式指定 surface 类型,用
`erika_presenter_attach_wgpu_surface(p, kind, raw_window, raw_display, w, h, scale)`,
配对应的 `ErikaWgpuSurfaceKind` 和平台句柄。

## 5. 打开并播放

```c
if (erika_presenter_open(p, "/path/to/video.mkv") != ErikaStatus_Ok) { /* 记录 */ }
erika_presenter_play(p);
```

`uri` 是本地路径或 HTTP(S) URL。

## 6. 渲染循环

从 surface 的显示定时器驱动 `render_tick`——`CADisplayLink`（iOS）/ `CVDisplayLink`
或 `CADisplayLink`（macOS）/ Windows 的帧调度器。传该帧的**呈现时间(秒)**,取自单调
宿主时钟;Erika 用它做 vsync 量化调度,所以传绝对时间戳,不是增量。

```c
// 每个显示帧:
ErikaPresenterStats stats;
erika_presenter_render_tick(p, host_time_seconds, &stats);   // out_stats 可为 NULL

// 同一次迭代里把事件抽干:
ErikaEvent ev;
while (erika_presenter_poll_event(p, &ev) == ErikaStatus_Ok) {
    handle_event(&ev);
}
```

任何 drawable 尺寸或 scale 变化时(窗口缩放、显示器 DPI 改变、设备旋转),在下一次 tick
**之前**调 `erika_presenter_resize_surface(p, w, h, scale)`。Windows demo 每帧轮询
`GetClientRect` + `GetDpiForWindow`,变化时 resize。

`render_tick` 很快返回;它自身不在 vsync 上阻塞——节奏由你的显示定时器提供。若你不在
显示回调里(如冒烟测试),每次迭代 `~16 ms` 的 sleep 可近似 60 Hz。

## 7. 处理事件

`poll_event` 非阻塞,队列空时返回 `NoEvent`。按 `ev.kind` 分发:

| Kind | 含义 | 读取 |
|------|------|------|
| `StateChanged` | 播放状态变化 | `ev.state` |
| `DurationChanged` | 时长已知/更新 | `ev.duration_micros` |
| `PositionChanged` | 周期性位置 tick | `ev.position_micros` |
| `TracksChanged` | 轨道列表变化 | 重新查询 `erika_presenter_tracks` |
| `TrackSelectionChanged` | 选择变化 | `erika_presenter_track_selection` |
| `BufferingChanged` | 缓冲切换 | `ev.buffering` |
| `VideoParamsChanged` | 分辨率 / 色彩元数据 | `ev.video` |
| `Error` | 发生失败 | `ev.status` + `erika_last_error_message` |

## 8. 运行时控制

以下都可在两次 tick 之间安全实时调用:

- **传输:** `play` / `pause` / `stop` / `seek(position_micros)` /
  `set_playback_rate(rate)`。
- **音频:** `set_volume(0.0–1.0)`。
- **轨道:** `erika_presenter_tracks`(计数数组惯用法)、`select_audio_track` /
  `select_subtitle_track`(id `-1` 关闭字幕)、`add_external_subtitle`、
  `remove_subtitle_track`、`set_subtitle_scale`。
- **弹幕:** 加载一条轨(`load_danmaku_file` / `_json` 或多轨 `add_danmaku_track_*`)、
  开关(`set_danmaku_enabled`)、用 `set_danmaku_config` 调参、偏移轨、设字体、设屏蔽词。
  见 [danmaku_architecture.md](danmaku_architecture.md)。
- **超分:** `set_upscaler(mode)`;用 `get_upscaler_status` 检视。

## 9. 释放

```c
erika_presenter_detach_surface(p);   // 先停止往 surface 绘制
erika_presenter_destroy(p);          // 停止播放,释放一切
```

在拆掉窗口/layer 之前先 detach,让 Erika 停止触碰 surface。`destroy` 对 `NULL` handle
安全。

## 10. 线程模型

handle **没有内部同步**。最简单的正确设计:在一个线程上拥有 presenter——就是跑显示
定时器的那个——并从那里发起所有调用(`render_tick`、传输、轨道变更)。若必须从另一个
线程调用(如 UI 线程发 `seek`),用你自己的锁串行化,使同一 handle 上两次调用绝不重叠。
错误信息是线程局部的,所以在发起失败调用的线程上读 `erika_last_error_message`。

## 分语言注意点

### C / C++

include `erika.h`,链接库(见 [building.zh.md](building.zh.md)),就这么简单——ABI 是
纯 C。C++ 里把 handle 包进一个在析构时调 `erika_presenter_destroy` 的 RAII 类型,并
用配套的 Erika 释放函数释放返回的字符串 / `ErikaTrackInfo`,绝不用 `delete`。

### Swift

通过 bridging header 或对 `erika.h` 的 module map 引入 C ABI。用
`unsafeBitCast(layer, to: UInt64.self)` 或 `UInt64(UInt(bitPattern: ...))` 转
`CAMetalLayer`。从 `CADisplayLink`/`CVDisplayLink` 回调驱动
`erika_presenter_render_tick`。macOS/iOS 的 Flutter Swift 插件就是在同一 C ABI 上这么做。

### Dart FFI

用 `dart:ffi` 绑定符号（`DynamicLibrary.open` 加载 dylib/dll,或静态链接用进程符号）。
把所有 FFI 调用放在一个 isolate;用 `toNativeUtf8`/`free` 编排字符串。高层
`erika_flutter` 包已经做好了这些——除非你在做自定义嵌入,否则优先用它。

## 检查清单

- [ ] 创建 presenter(用与你显示器匹配的输出模式)。
- [ ] 用**物理像素**尺寸和正确的 scale attach surface。
- [ ] 先 open 再 play;别阻塞——通过事件观察就绪。
- [ ] 每个显示帧 `render_tick(absolute_time_seconds)`;抽干事件。
- [ ] 每次尺寸/scale 变化都 `resize_surface`。
- [ ] 每个 handle 一个线程,或串行化调用。
- [ ] 释放每个返回的字符串 / `ErikaTrackInfo`;先 `detach` 再 `destroy`。
