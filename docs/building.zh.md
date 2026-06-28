# 构建 Erika

Erika 是一个 Rust workspace,它链接一组**静态构建的原生依赖**(FFmpeg,以及可选的
libass 字幕栈)。这些原生库不随仓库 vendoring——你用 `xtask` 编排器构建一次,它会把
产物安置到 `third_party/dist/` 下,Rust crate 再链接那个目录。

```
xtask deps build  ──▶  third_party/dist/<target>/<profile>/{ffmpeg,zlib,libass,…}
                                        │
                          erika_ffmpeg_sys/build.rs（自动发现 dist,运行 bindgen）
                                        │
                                  cargo build -p erika
```

> 英文版：[building.md](building.md)。

## 前置依赖

### Rust

- Rust **1.92+**(workspace edition 2024)。
- 交叉目标需安装对应 Rust std target,如
  `rustup target add aarch64-apple-ios` 或
  `rustup target add x86_64-pc-windows-msvc`。

### 构建工具 —— macOS / Unix 宿主

`tar`、`make`、`clang`、`cmake`、`pkg-config`、`python3`(带 `venv`)必须在 `PATH` 上。
构建完整字幕栈(`--all`)还需 `meson` 和 `ninja`(Intel 宿主上 FFmpeg 的 x86 汇编需
`nasm`)。macOS 上安装 Xcode Command Line Tools,再通过 Homebrew 装上述工具。

`erika_ffmpeg_sys` 运行 **bindgen**,需要 `libclang`。若未自动找到,设置 `LIBCLANG_PATH`。

### 构建工具 —— Windows(`x86_64-pc-windows-msvc`)

- **Visual Studio Build Tools**(MSVC)+ Windows SDK,以及 **CMake** 组件。
- 一个 **POSIX shell**(Git for Windows 或 MSYS2)——FFmpeg 的 `configure` 需要它。
- **GNU make**(MSYS2 `make` 或 MinGW `mingw32-make`)。
- FFmpeg 汇编需 `nasm`。
- `--all` 还需 **Python**(带 `venv`);`xtask` 会自动提供 `pkg-config` shim。

请在 MSVC 环境已激活的 shell 里运行命令(如 *"x64 Native Tools Command Prompt"*),
以便 `xtask` 定位工具链。

## 用 `xtask` 构建原生依赖

`xtask` 是一个 workspace 成员,用 `cargo run -p xtask -- …` 调用。

```sh
# 查看将要构建什么(无副作用)
cargo run -p xtask -- deps plan
cargo run -p xtask -- deps status

# 构建最小集(zlib + FFmpeg)—— LGPL profile
cargo run -p xtask -- deps build --profile lgpl

# 构建全部,含 libass 字幕栈
cargo run -p xtask -- deps build --all --profile lgpl
```

子命令:`plan`(打印计划)、`fetch`(只下载源)、`status`(已有/已构建)、`build`
(下载 + 编译)。

### 选项

| 标志 | 取值 | 默认 | 含义 |
|------|------|------|------|
| `--profile` | `lgpl`、`gpl-full` | `lgpl` | FFmpeg 许可证 profile(见下)。 |
| `--target` | 见目标表 | `host` | 交叉编译目标。 |
| `--all` | — | 关 | 同时构建 libass + FreeType + HarfBuzz + FriBidi(字幕渲染)。不加时只构建 zlib + FFmpeg。 |
| `--force` | — | 关 | 即使已是最新标记也重建。 |
| `--jobs N` | 整数 | 自动 | 原生构建的并行度。 |

### 目标

| `--target` | Triple | 备注 |
|------------|--------|------|
| `host` | 当前机器 | 默认。 |
| `aarch64-apple-darwin` | Apple Silicon macOS | |
| `x86_64-apple-darwin` | Intel macOS | |
| `aarch64-apple-ios` | iOS 设备 | |
| `aarch64-apple-ios-sim` | iOS 模拟器(Apple Silicon) | |
| `x86_64-apple-ios` | iOS 模拟器(Intel) | |
| `x86_64-pc-windows-msvc`(或 `windows-x64`) | Windows | FFmpeg 里把 VideoToolbox 换成 D3D11VA/DXVA2。 |

部署最低版本默认 macOS `11.0` / iOS `13.0`,可用
`MACOSX_DEPLOYMENT_TARGET` / `IPHONEOS_DEPLOYMENT_TARGET` 覆盖。

## 许可证 profile

原生构建按 profile 分割,使许可证边界明确:

- **`lgpl`**(默认)—— FFmpeg 配置为 `--disable-gpl --enable-version3`,静态,无网络,
  仅 file 协议,一组精选的 demuxer/decoder/parser,启用 zlib,外加 VideoToolbox(Apple)
  或 D3D11VA/DXVA2(Windows)。
- **`gpl-full`** —— 同一集合加 `--enable-gpl`。仅当你接受产物的 GPL 条款时使用。

Rust workspace 本身是 MPL-2.0(见 [`LICENSE`](../LICENSE))。`xtask` 与你的
`cargo build` 之间保持 profile 一致。`cargo run -p xtask -- check license` 校验策略。

## `dist` 布局

构建后,库会落到(按 target + profile):

```
third_party/
  cache/                       下载的归档
  src/                         解压后的源
  build/<target>/<profile>/    out-of-tree 构建树
  dist/<target>/<profile>/     crate 链接的安装前缀:
    ffmpeg/{include,lib}
    zlib/    libass/    freetype/    harfbuzz/    fribidi/
```

对 `host` 目标,`<target>` 路径段省略(`third_party/dist/<profile>/…`)。

## crate 如何找到 `dist`

`erika_ffmpeg_sys/build.rs` 自动发现 FFmpeg 前缀:

1. `ERIKA_FFMPEG_DIR`(若设置,显式覆盖)。
2. 否则 `third_party/dist/$ERIKA_NATIVE_TARGET/$ERIKA_NATIVE_PROFILE/ffmpeg`
   (若 `ERIKA_NATIVE_TARGET` 已设)。
3. 否则 workspace 根下的 `third_party/dist/<profile>/ffmpeg`(为 iOS 构建时带 `ios/`
   段)。

相关环境变量:`ERIKA_NATIVE_PROFILE`、`ERIKA_NATIVE_TARGET`、`ERIKA_FFMPEG_DIR`、
`ERIKA_ZLIB_DIR`、`LIBCLANG_PATH`,以及 `ERIKA_ALLOW_LEGACY_FFMPEG`(应急开关)。Erika
需要 FFmpeg **7.x**(`libavutil >= 59`);Windows 原生核心强制此点。仅在本地兼容性实验时
才设 `ERIKA_ALLOW_LEGACY_FFMPEG=1`。

## 编译与测试

```sh
cargo build -p erika                 # 核心库
cargo build -p erika_capi            # C ABI(产出 dylib/staticlib/dll)
cargo test --workspace               # 单元 + 集成测试
```

`erika_capi` 产出原生宿主链接的工件:

- macOS:`liberika_capi.dylib`(macOS Flutter 插件用 `dlopen` 加载;用 `ERIKA_CAPI_DYLIB`
  覆盖)。
- iOS:`liberika_capi.a`(静态)。
- Windows:`erika_capi.dll`(Flutter Windows 插件通过 `build_erika_runtime.cmake` 构建)。

## 验证播放路径

```sh
# macOS
export SAMPLE="/path/to/video.mp4"
cargo run -p macos_native_demo -- "$SAMPLE"
cargo run -p macos_native_demo -- --smoke-seconds 3 "$SAMPLE"

# Windows
cargo run -p windows_native_demo -- "%SAMPLE%"
cargo run -p windows_native_demo -- --smoke-seconds 3 --metrics-log out.jsonl "%SAMPLE%"
```

demo 会打印每帧流水线统计(解码/渲染帧、零拷贝 vs CPU 回退、HDR10 活动、音频
underflow)——快速确认硬解和零拷贝互操作已生效。

## 排错

- **"FFmpeg headers were not found …"** —— 你没为该 target/profile 跑 `xtask deps build`,
  或 `ERIKA_NATIVE_TARGET`/`ERIKA_NATIVE_PROFILE` 与你构建的不匹配。跑 `deps status` 看
  现有什么。
- **bindgen / libclang 报错** —— 把 `LIBCLANG_PATH` 设到你的 LLVM `lib` 目录。
- **Windows:configure 失败** —— 确保 POSIX shell(Git Bash/MSYS2)和 GNU make 在 `PATH`
  上,且你是从 MSVC 环境启动的。
- **旧版 FFmpeg 被拒** —— 安装/构建 7.x 包;别依赖系统 FFmpeg。
- **license 校验失败** —— 你的 profile 混了 GPL 与 LGPL 工件;用单一 `--profile` 重建 deps。

开发工作流见 [CONTRIBUTING.zh.md](../CONTRIBUTING.zh.md),各部分如何拼接见
[architecture.zh.md](architecture.zh.md)。
