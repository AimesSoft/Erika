# Native Linux support (experimental)

Linux supports source builds of the Rust engine and `liberika_capi.so`, with
NVDEC/VA-API hardware decoding, FFmpeg software decoding, wgpu SDR presentation
on X11/Wayland, a Linux Flutter texture plugin, subtitles/danmaku and PulseAudio
output (including PipeWire-Pulse and WSLg). Prebuilt Linux releases, zero-copy
video import and HDR output are not available yet.
X11 currently uses screen 0. Only x86_64 has been exercised locally.
WSLg hardware rendering on the RTX 5070 is validated via Mesa D3D12 / OpenGL;
NVDEC H.264, HEVC 10-bit and AV1 decoding have also passed real-device playback.
Intel/AMD VA-API is implemented but has not been tested on those physical cards.

## Build on Ubuntu 26.04

Rust 1.92+ and FFmpeg **8.x** development libraries are required. Ubuntu 22.04
and 24.04's default FFmpeg packages are too old for this build path.

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

bash scripts/build_linux_libass.sh
cargo build --locked -p linux_native_demo -p erika_capi
cargo run --locked -p linux_native_demo -- --wayland /path/to/video.mkv
```

Use `--x11` to select X11, or omit either flag for automatic selection. No media
argument displays a test pattern. Space pauses/resumes, arrows seek by five
seconds, and Esc exits. Build artifacts are in `target/debug`; add `--release`
for `target/release`. On WSL, set `CARGO_TARGET_DIR="$HOME/.cache/erika-target"`
to keep build output on the Linux filesystem. `pactl info` should find a server;
WSLg already provides one. A Vulkan/EGL driver is required; `/dev/dxg` alone
does not establish hardware acceleration (check `vulkaninfo --summary`).

### GPU rendering on WSLg

```bash
bash scripts/run_wsl_gpu.sh --wayland /path/to/video.mkv
# X11 is also supported:
bash scripts/run_wsl_gpu.sh --x11 /path/to/video.mkv
```

The launcher scopes `GALLIUM_DRIVER=d3d12`, `WGPU_BACKEND=gl`, and
`ERIKA_REQUIRE_HARDWARE_GPU=1` to Erika. Optionally select a card with
`MESA_D3D12_DEFAULT_ADAPTER_NAME=NVIDIA` (or AMD/Intel). WSL uses the Windows
GPU driver through `/dev/dxg`; a Linux NVIDIA kernel driver is not needed.
The pipeline is wgpu → EGL/OpenGL → Mesa D3D12 → Windows GPU driver.
Check the `adapter_selected` log with `displayBound: true` for the actual
window device; this machine reports `D3D12 (NVIDIA GeForce RTX 5070)` and
`backend: Gl`. Strict mode rejects CPU/software adapters instead of silently
falling back. Native Linux Vulkan remains available (`WGPU_BACKEND=vulkan`).

EGL is initialized on the actual host display and adapter selection checks
surface compatibility. Successful detach releases that display's EGL objects;
one CPU frame is retained so a paused image survives detach/reattach. NVDEC/VA-API
frames are downloaded using FFmpeg and uploaded to wgpu. This is hardware decode
with a CPU transfer, not zero-copy; statistics report these separately.

Erika retains WSL's D3D runtime until process exit to keep its thread-local
cleanup callbacks valid after EGL teardown. Audio uses the callback consumption
clock, consistent with the other ring-based outputs. WSLg/RDP device latency is
reported separately for draining/rate transitions; subtracting it from this
clock can starve the bounded audio prefetch window. End-device latency still
depends on the WSLg/RDP sink.

On WSLg, pause keeps the remote audio transport running with silence without
consuming the media ring. Resume continues media consumption without waiting
for the RDP sink to uncork. Ordinary Linux PulseAudio/PipeWire sessions still
cork on pause. PulseAudio cork/flush operations are submitted asynchronously.

Run `bash scripts/check_wsl_gpu.sh` for GPU readback/pixel tests, real audio,
X11/Wayland playback and C ABI lifecycle checks. Playback must reach at least
7.9 seconds of the 8-second fixture within 10 seconds, not merely issue draws.
The RTX 5070 passes all 27 wgpu tests, including readback and thread teardown.

## Integration

Linux defaults to CUDA/NVDEC, then VA-API. Set `ERIKA_HWDEC=cuda`, `vaapi`, or
`software` to select a backend. `ERIKA_CUDA_DEVICE=0` selects a CUDA ordinal;
`ERIKA_VAAPI_DEVICE=/dev/dri/renderD128` selects a render node. NVIDIA needs its
user-space CUDA/NVDEC libraries; Intel needs media-driver/iHD (or i965 on older
cards); AMD needs Mesa radeonsi VA-API. Supported codecs depend on the card and
driver. WSL Intel/AMD additionally depend on the Mesa D3D12 VA driver, not a
presumed `/dev/dri` node. This NVIDIA WSL machine has no `/dev/dri`.

Set `ERIKA_REQUIRE_HARDWARE_DECODE=1` to reject software fallback. This is separate
from the rendering adapter check `ERIKA_REQUIRE_HARDWARE_GPU=1`. Unsupported
videos fail explicitly in strict mode; otherwise transitions are logged.

For Flutter, build `erika_capi --release`, point `ERIKA_LIBRARY_DIR` at the folder
containing `liberika_capi.so`, and use a Linux Flutter SDK with a path dependency
on `packages/erika_flutter`. Both `ErikaVideoView` and `ErikaTextureVideoView` work
through GTK's pixel buffer texture. GPU compositing includes subtitles, danmaku
and HUD; RGBA readback incurs additional bandwidth at high resolutions. DMA-BUF
zero-copy, Linux HDR and MPRIS are not implemented. Bundle the matching library
and headers. Screenshot returns tightly packed RGBA bytes.
The plugin saves, unbinds and restores GTK's EGL/GLX context around native calls
to avoid `EGL_BAD_ACCESS`. Set `ERIKA_DEBUG_HUD=1` to show the diagnostic HUD.
Installed plugins resolve sibling shared libraries through `$ORIGIN`.

Rust consumers must enable the `wgpu` feature. See the
[native demo](../examples/linux_native_demo/src/main.rs). C consumers use
[`erika.h`](../crates/erika_capi/include/erika.h) and the shared library; see
[the C smoke example](../examples/linux_native_demo/capi_smoke.c).

For `erika_presenter_attach_wgpu_surface`, `XlibWindow` takes an X11 `Window` ID
and `Display*`; `WaylandSurface` takes `wl_surface*` and `wl_display*`. Cast
pointers through `uintptr_t` to `uint64_t`. Keep both host-owned handles alive
until detach or presenter destruction finishes. Attach/render/resize/detach on
the window event-loop thread, serialize calls per presenter, and drive
`render_tick` once per display frame. Width/height are physical pixels; scale is
an independent DPI factor. Resolve `.so` dependencies using rpath or
`LD_LIBRARY_PATH` and inspect them with `ldd`.

Linux uses system FFmpeg shared libraries and a static, PIC, **patched libass
0.17.5**, with system font libraries. The script verifies the upstream archive
SHA256 and applies Erika's ordered memory-font fallback patch. Stock libass
does not preserve that behavior. `ERIKA_LIBASS_DIR` can point to an equivalent
patched prefix. `ERIKA_FFMPEG_DIR` selects an existing static FFmpeg bundle;
`ERIKA_USE_SYSTEM_LIBS=0` preserves the original bundle lookup path but requires
all matching dependencies to be prepared separately. This guide validates the
system-library path and native GNU/Linux builds, not cross-compilation or musl.
The distro's FFmpeg codecs and licensing apply; `ERIKA_NATIVE_PROFILE=lgpl`
does not change its configuration. PulseAudio disconnections report an error;
automatic reconnection is not implemented.

## Validation

```bash
# Needs X11 and a PulseAudio-compatible server.
bash scripts/check_linux.sh
# Also exercise Wayland when a compositor is available:
ERIKA_TEST_WAYLAND=1 bash scripts/check_linux.sh
```

This runs unit tests, the real audio pause/resume/reconfigure test, header
alignment, playback smoke tests, and a separately linked C executable covering
pause, seek, resize and teardown. The `Linux native` workflow uses Ubuntu 26.04,
Xvfb, Mesa and a PulseAudio null sink; remote CI results must be checked after
submission. Local validation uses x86_64 Ubuntu 26.04 on WSLg, Rust 1.93.1,
FFmpeg 8.0.1, llvmpipe and an RTX 5070 through Mesa D3D12. X11, Wayland, the
C ABI pause/display recreation/seek/resize path, and rejection of CPU fallback
have passed locally. ARM64 and standalone desktop testing remain outstanding.
See the [Chinese guide](linux.zh.md) for more detail.

The installed NipaPlay 1.10.12 release also passed Chinese SRT display, progress
danmaku, RGBA thumbnail capture, seeking, fullscreen transitions and resuming
after a pause of several minutes. The native texture C API test passed a
12.5-second pause with 200 hardware frames, no software frames and no audio or
render errors. The final focused presenter suite passed 24 tests. NVDEC H.264,
HEVC Main10 and AV1 fixtures passed; invalid VA-API devices fail in strict mode.
These results do not establish physical Intel/AMD or native desktop coverage.
