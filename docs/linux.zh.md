# Linux 原生接入（实验性）

Linux 现在可以从源码构建 Rust 播放器与 `liberika_capi.so`，通过 X11 或 Wayland
显示视频，通过 PulseAudio 输出音频。PipeWire 桌面需启用 `pipewire-pulse`；WSL2
使用 WSLg 提供的显示与音频服务。

当前范围：NVDEC/VA-API 硬件解码、FFmpeg 软件解码、wgpu 呈现、Linux Flutter
纹理插件及 Wayland 原生视频层、字幕/弹幕、音频播放、暂停/恢复、seek、窗口缩放。
已实现 HDR 表面协商，但真实显示器 HDR 输出与直接零拷贝尚未验收通过。
尚未提供 Linux 预编译发布包。
X11 当前使用 screen 0。原生 ARM64 构建路径已准备，但验收仅覆盖 x86_64。
WSLg 的 RTX 5070 已实测 NVDEC（H.264、HEVC 10-bit、AV1）与 Mesa D3D12 / OpenGL 渲染。
Intel/AMD 的 VA-API 路径已实现，尚无对应显卡的实机验收。

## 1. 安装依赖

以下命令适用于 **Ubuntu 26.04**。需要 Rust 1.92+ 和 **FFmpeg 8.x** 开发包；
Ubuntu 22.04/24.04 默认仓库中的旧版 FFmpeg 不满足此构建路径。

```bash
sudo apt update
sudo apt install -y \
  build-essential pkg-config clang libclang-dev rustc cargo rustfmt \
  curl ca-certificates meson ninja-build patch xz-utils python3 \
  libavdevice-dev libavfilter-dev libavformat-dev libavcodec-dev \
  libswresample-dev libswscale-dev libavutil-dev \
  libfreetype-dev libharfbuzz-dev libfribidi-dev libfontconfig-dev \
  libpulse-dev libx11-dev libwayland-dev libxkbcommon-dev libxkbcommon-x11-0 \
  libvulkan-dev mesa-vulkan-drivers libegl1 libgl1-mesa-dri \
  fonts-dejavu-core pulseaudio-utils
```

桌面环境应有可用的 Vulkan 或 EGL 驱动。`pactl info` 可检查音频服务。
WSLg 已提供 PulseAudio 服务，无需再启动另一个服务。

## 2. 构建与运行

在仓库根目录运行：

```bash
# 下载并校验固定版本 libass，应用 Erika 字体回退补丁，再构建静态 PIC 库。
bash scripts/build_linux_libass.sh
cargo build --locked -p linux_native_demo -p erika_capi

# 自动选择显示协议；也可明确传入 --x11 或 --wayland。
cargo run --locked -p linux_native_demo -- /path/to/video.mkv
```

不传视频路径时显示测试图案。空格暂停/继续，左右箭头前后跳转 5 秒，Esc 退出。
输出文件为 `target/debug/linux_native_demo` 与 `target/debug/liberika_capi.so`；
发布构建加 `--release`，输出目录改为 `target/release`。

WSL 中仓库若位于 `/mnt/c`，建议把编译缓存放在 Linux 文件系统：

```bash
export CARGO_TARGET_DIR="$HOME/.cache/erika-target"
cargo run --locked -p linux_native_demo -- --wayland /mnt/c/path/to/video.mkv
```

这时所有产物位于 `$CARGO_TARGET_DIR/debug`。Windows 不能直接加载 Linux `.so`。
WSL 中可有 `/dev/dxg` 却仍使用 Mesa llvmpipe 软件渲染；请以 `vulkaninfo --summary`
或 `glxinfo -B` 显示的实际设备为准。

### WSLg 使用 GPU

在本机 Ubuntu 中运行：

```bash
bash scripts/run_wsl_gpu.sh --wayland /mnt/c/path/to/video.mkv
# X11 也已验证；不传视频文件时显示测试图案。
bash scripts/run_wsl_gpu.sh --x11
```

该启动脚本将 `GALLIUM_DRIVER=d3d12`、`WGPU_BACKEND=gl` 和
`ERIKA_REQUIRE_HARDWARE_GPU=1` 仅应用于 Erika 进程。多 GPU 机器可额外设置
`MESA_D3D12_DEFAULT_ADAPTER_NAME=NVIDIA`（也可指定 AMD / Intel）。
无需在 WSL 中安装 Linux NVIDIA 内核驱动；WSL 使用 Windows 驱动提供的 `/dev/dxg`。

GPU 路径为 `wgpu → OpenGL/EGL → Mesa D3D12 → Windows 显卡驱动`。
启动日志应显示 `name: D3D12 (NVIDIA GeForce RTX 5070)`、`backend: Gl`、
`hardware: true`；`displayBound: true` 表示窗口使用的实际适配器。
严格模式会拒绝 llvmpipe、CPU 适配器或 Microsoft Basic Render Driver，避免无声回退。
Linux 原生 Vulkan 仍使用默认路径，亦可通过 `WGPU_BACKEND=vulkan` 显式选择。

EGL 初始化绑定宿主的实际 display，并在 adapter 选择时检查窗口兼容性。
detach 成功时释放所有借用该 display 的 EGL 对象；保留一帧 CPU 像素以便暂停时重建窗口。
WSL OpenGL 路径中 NVDEC/VA-API 解码后的帧经 `av_hwframe_transfer_data` 下载，再上传到 wgpu。
该路径是硬件解码，但不是零拷贝；统计中的 `hardwareVideoFrames` 与
`cpuVideoFrameFallbacks` 会同时增加，`softwareVideoFrames` 不会因此增加。

WSL D3D 运行库会在线程退出时执行 TLS 清理；Erika 保留其加载引用直到进程退出，
避免 EGL 销毁后清理函数指向已卸载的库。音频同步采用回调消费时钟，与其它 ring-buffer
输出后端一致；WSLg/RDP 的额外设备延迟单独用于排空/倍速切换，不从此时钟扣除，
避免超过引擎的音频预取窗口而反复缓冲。远端音频设备的实际延迟仍取决于 WSLg/RDP。

WSLg 暂停时保留音频传输并输出静音，不消耗媒体 ring；恢复时直接继续消费媒体，
避免 RDP sink 的 uncork 延迟阻塞播放器。普通 Linux PulseAudio/PipeWire 仍使用 cork。
PulseAudio cork/flush 操作异步提交，不在播放器线程等待服务端回调。

完整 GPU 回归检查（需要 WSLg 的 X11、Wayland 和音频服务）：

```bash
bash scripts/check_wsl_gpu.sh
```

这会在 GPU 上执行图像读回测试、音频测试、两种窗口协议的完整 8 秒媒体播放，
并检查 C ABI 的暂停、显示连接重建、seek 与 resize。播放检查要求 10 秒内到达至少
7.9 秒媒体时间，避免只检测到渲染调用就把卡住的播放判定为成功。

## 硬件解码与显卡驱动

| 显卡 | Erika 后端 | Linux 驱动要求 |
|---|---|---|
| NVIDIA | `cuda` / NVDEC | 支持该显卡的 NVIDIA 用户态驱动、`libcuda.so.1`、`libnvcuvid.so.1` |
| Intel | `vaapi` | Intel media-driver (`iHD`)，旧显卡可使用 i965；可访问 `/dev/dri/renderD*` |
| AMD | `vaapi` | Mesa radeonsi VA-API 与可访问的 `/dev/dri/renderD*` |

默认先尝试 CUDA，再尝试 VA-API；驱动或视频格式不支持时会发出明确回退事件。
显卡代际、驱动和 FFmpeg 编译选项决定具体支持的编码、位深和分辨率。
WSL 中 Intel/AMD 的 VA-API 取决于 Mesa D3D12 VA 驱动；不能把普通 Linux 的
`/dev/dri` 路径当作 WSL 已提供的设备。本机只有 NVIDIA，且没有 `/dev/dri`。

```bash
# 要求硬解，失败即报错；不会静默使用 CPU 解码。
export ERIKA_REQUIRE_HARDWARE_DECODE=1
export ERIKA_HWDEC=auto       # auto（默认）、cuda/nvdec、vaapi、software
# 多显卡可选：
export ERIKA_CUDA_DEVICE=0
export ERIKA_VAAPI_DEVICE=/dev/dri/renderD128
```

`ERIKA_REQUIRE_HARDWARE_GPU` 管渲染设备，`ERIKA_REQUIRE_HARDWARE_DECODE` 管解码器，
两者独立。开启后者时，显卡不支持的视频会明确失败。关闭后可按日志回退软解。

## Flutter Linux 接入

使用 Linux Flutter SDK 构建宿主，依赖本地 `packages/erika_flutter`，并设置：

```bash
cargo build --locked --release -p erika_capi
export ERIKA_LIBRARY_DIR="${CARGO_TARGET_DIR:-$PWD/target}/release"
# 在 Flutter 宿主目录：
flutter build linux --release
```

插件通过 GTK `FlPixelBufferTexture` 将 GPU 合成的视频、字幕、弹幕与 HUD 交给 Flutter。
`ErikaVideoView` 和 `ErikaTextureVideoView` 均可用于 Linux；纹理支持 Flutter 遮罩与 UI 叠层。
这条兼容路径会从 GPU 读回 RGBA，4K/高帧率有额外带宽开销。Wayland 可显式使用
`ErikaWindowOverlayVideoView`：视频通过 `wl_subsurface` 放在透明 Flutter UI 下，
由 Linux 合成器合成，不经过 Flutter 的逐帧 RGBA8 回读。宿主须将视频区域留透明；
原生层仅支持不透明 `srcOver`，不支持 Flutter 对视频的裁剪或颜色滤镜。X11 继续使用纹理。
MPRIS 系统媒体控制尚未实现。显式截图仍返回紧密排列的 RGBA 像素。播放、暂停、seek、音轨/字幕切换和弹幕
走同一 C ABI，插件必须与同次构建的头文件和 `.so` 一起使用。
插件在原生调用前后保存、解绑和恢复 GTK 的 EGL/GLX 上下文，避免 `EGL_BAD_ACCESS`。
`ERIKA_DEBUG_HUD=1` 可开启诊断 HUD；安装库使用 `$ORIGIN` 查找同目录依赖。

### GPU 帧导入与 HDR 的实际范围

实验性 Vulkan 桥通过 FFmpeg 映射 VA-API 帧，再在 GPU 上复制 NV12/P010 平面到
wgpu 纹理；驱动支持时不经过主机像素缓冲，但仍有 GPU copy，不计为直接零拷贝。
需要外部显存/信号量、timeline semaphore、synchronization2 及兼容的设备/格式。
Intel/AMD 实卡导入尚未验收。FFmpeg 8 的 CUDA→Vulkan 失败清理会崩溃，因此该组合
在调用前被拒绝；普通 NVDEC 硬解仍可通过 CPU 平面传递播放。
[FFmpeg 上游修复](https://github.com/FFmpeg/FFmpeg/commit/c29d710cd5d0f80bbb56f7ec1f35d5fb7ed44d05)
已进入 FFmpeg 9，但本集成使用该版本重建仍需验证。

- `ERIKA_REQUIRE_GPU_FRAMES=1`：任何需要 CPU 上传的解码帧都报错。
- `ERIKA_REQUIRE_ZERO_COPY=1`：当前 Linux 路径明确报错，包括 GPU copy 和软解帧，不冒充零拷贝。
- `ERIKA_LINUX_HDR=auto`：跟随 HDR/SDR 片源；`on` 请求扩展线性输出，`off` 选择 SDR。
- `ERIKA_REQUIRE_HDR=1`：HDR 片源遇到 SDR 表面或 Flutter RGBA8 路径时报错。

HDR 使用原生视频层的 FP16 scRGB；仅接受 Vulkan WSI 实际提供的
`R16G16B16A16_SFLOAT + EXTENDED_SRGB_LINEAR_EXT`，仅有 10-bit SDR 格式不算 HDR。
合成器负责将 HDR 视频与 SDR Flutter UI 合成并转换到显示器输出。
默认 12.5 headroom 是 1000/80 nit 内容目标，不是屏幕亮度测量；没有真实测量时
`activeHeadroomKnown` 仍为 false。本实现不含 HDR10+ 或 Dolby Vision 透传。

本机隔离构建的 Mesa Dozen 26.0.8 已将 RTX 5070 识别为 Vulkan 硬件设备。
本地格式查询实验进一步到达 CUDA 外部显存导入，但驱动返回 `CUDA_ERROR_NOT_SUPPORTED`。
当前 WSLg 也未提供 HDR 颜色管理/WSI 能力。这不影响已实测的 OpenGL/Vulkan 硬件渲染；
限制发生在互操作与显示链路。实验性 Mesa 构建不是发布依赖，启动脚本不会自动安装它。

## 3. Rust 与 C/C++ 接入

Rust 依赖需要启用 `erika` 的 `wgpu` feature，参考
[`examples/linux_native_demo/src/main.rs`](../examples/linux_native_demo/src/main.rs)。
C 接入使用 [`erika.h`](../crates/erika_capi/include/erika.h)，默认 C ABI 构建已启用 wgpu。
动态加载时需让加载器找到 `.so` 与当前发行版的 FFmpeg、PulseAudio、字体库依赖；
可配置 rpath 或 `LD_LIBRARY_PATH`，并用 `ldd` 检查。

`erika_presenter_attach_wgpu_surface` 参数映射如下：

| surface kind | raw_window | raw_display |
|---|---|---|
| `ErikaWgpuSurfaceKind_XlibWindow` | X11 `Window` 整数 ID | `Display*` |
| `ErikaWgpuSurfaceKind_WaylandSurface` | `wl_surface*` | `wl_display*` |

指针经 `uintptr_t` 转为 `uint64_t`。窗口与 display 由宿主持有，必须在
`detach_surface` 或 presenter 销毁完成后才能释放；窗口管理、attach、render、resize、
detach 在宿主窗口事件线程执行。每个 presenter 的调用应串行进行。
宽高为物理像素，`scale` 单独提供 DPI 比例，不乘到宽高上。

典型顺序：`create → attach → open → play → render_tick → close → detach → destroy`。
宿主驱动约 60 Hz 的 `render_tick`，处理 resize，并转发暂停、继续、seek。
可编译的 C 示例：[`capi_smoke.c`](../examples/linux_native_demo/capi_smoke.c)。

## 4. 依赖选择与许可

- Linux 默认通过 pkg-config 动态链接系统 FFmpeg 8 与 PulseAudio。
- libass 使用 Erika 补丁版本 0.17.5；系统 libass 缺少有序内存字体回退补丁，不能直接替代。
  脚本将其安装到 `third_party/dist/<rust-host-triple>/system/libass`。
  `ERIKA_LIBASS_DIR` 可指定兼容的补丁构建前缀。
- `ERIKA_FFMPEG_DIR` 可指定已有的静态 FFmpeg bundle。`ERIKA_USE_SYSTEM_LIBS=0`
  保留原有 bundle 查找路径，需要另行准备全部匹配的原生依赖；本指南仅验收系统库路径。
- 系统 FFmpeg 的编解码器与许可由发行版构建配置决定。
  设置 `ERIKA_NATIVE_PROFILE=lgpl` **不会改变系统 FFmpeg 的配置或许可**。
- 构建脚本使用本机 GNU/Linux 工具链；跨编译与 musl 不在本次验收范围内。
- PulseAudio 服务断开会报告音频错误；自动重连尚未实现。

## 5. 验证

在有 X11 与音频服务的会话中：

```bash
bash scripts/check_linux.sh
# 若同时有 Wayland（例如 WSLg），也验证 Wayland 播放：
ERIKA_TEST_WAYLAND=1 bash scripts/check_linux.sh
```

脚本验证核心/C ABI 单元测试、真实音频暂停恢复与重配置、头文件对齐、X11 播放、
可选 Wayland 播放，并编译独立 C 程序加载 `.so` 检查播放、暂停、seek、resize 与销毁。
新增的 `Linux native` workflow 在 Ubuntu 26.04 容器中使用 Xvfb、Mesa 软件渲染与
PulseAudio null sink 运行 X11 路径；提交后需以实际 CI 结果为准。

本地验收环境：Ubuntu 26.04 / WSL2 / WSLg，x86_64，Rust 1.93.1，FFmpeg 8.0.1，
Mesa llvmpipe 以及 Mesa D3D12 / NVIDIA GeForce RTX 5070。独立 Linux 桌面与 ARM64 仍需另行验收。

2026-09-29 本地验证记录（上游基线 `97bbb84`）：

| 检查 | 结果 |
|---|---|
| `erika` 核心单元测试 | 657 通过，1 项音频集成测试默认忽略 |
| `erika_capi` 单元测试 | 41 通过 |
| 单独启用真实 PulseAudio 集成测试 | 1 通过 |
| C API / package header 对齐 | 5 通过，三份头文件 SHA256 一致 |
| X11 / Wayland 视频与音频 smoke | 均通过，渲染和音频错误数均为 0 |
| 独立 C 程序链接 `.so`，暂停/seek/resize/销毁 | 通过，渲染和音频错误数均为 0 |
| 格式、shell 语法、diff 空白、动态依赖解析 | 通过，无缺失动态库 |
| RTX 5070 X11 / Wayland 视频与音频 smoke | 均通过，实际 adapter 为 D3D12 RTX 5070，错误数均为 0 |
| RTX 5070 C ABI 暂停、关闭 display 后重新 attach、seek、resize | 通过，暂停画面保留，音频恢复正常 |
| GPU 严格模式下强制 llvmpipe | 按预期拒绝启动 |
| RTX 5070 GPU 图像读回 / wgpu 测试 | 27 通过，包含像素值比较与线程退出 |
| RTX 5070 NVDEC H.264 / HEVC Main10 / AV1 | 均通过，严格硬解；错误数均为 0 |
| 无效 VA-API 设备 + 严格硬解模式 | 明确失败，未回退软解 |
| 原生 Flutter texture C API 长暂停（12.5 秒） | 200 硬解帧、0 软解帧、0 渲染/音频错误，恢复后音频时钟继续推进 |
| 最终 presenter 定向单元测试 | 24 通过 |
| NipaPlay 1.10.12 Linux release 实际 UI | 播放、数分钟暂停后恢复、seek、全屏进入/退出、中文字幕和进度弹幕通过 |
| NipaPlay 原始 RGBA 截图及缩略图保存 | 427×240、409920 字节，保存成功 |
| 安装包动态链接与版本 | 从安装目录解析 Erika，SHA256 与 release 构建一致 |

原生 Wayland 视频层更新后另行验证：NipaPlay 1.11.9 / Flutter 3.47.0-0.3.pre
的 Rust 与 Flutter release 构建成功，Weston 截图确认透明控件、视频、中文字幕及
进度弹幕正确合成；暂停、全屏进入/退出和缩略图捕获通过。独立原生层 probe 在
OpenGL 与实验性 Dozen Vulkan 均完成暂停、缩放、detach/reattach、seek，暂停画面保留。
最终定向检查：29 项 wgpu 测试（含 2 项 HDR 协商）、5 项 API/头文件测试、C/C++
严格警告检查、2 项禁止 CPU 帧绕过严格模式的测试均通过；C ABI 生命周期播放
得到 316 帧、0 渲染/音频错误。完整 NipaPlay 会话的远端音频 underflow 计数非零，
尚不能据此保证原生 Linux 桌面的音频流畅度。上述结果不构成物理 HDR 或解码零拷贝验收。
