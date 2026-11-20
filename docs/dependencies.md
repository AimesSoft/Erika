# Cargo 依赖与本地联调

仓库内 `Cargo.toml` 的 Bern 固定到已合入 `master` 的 Bern `0.2.0` 提交 `2f68b23`（包含默认骨架图系统、内置窗口截图指令、完整内置控件体系及 `NativeView` 等能力，以及此前的页面持有查询、动画 SVG 引擎、沉浸窗口指针隐藏、原生液态玻璃容器与 macOS 统一顶栏改进）；liberika 用 Git 依赖，固定到提交。GitHub 或另一台电脑检出 Erika 后，Cargo 会下载同一份源码，无需本机的绝对路径。

liberika 当前固定到 `v0.3.6` 的提交 `e38a511`（弹幕彩色表情、系统字体优先，WebUI 用宿主的回退字体）。后续升级时更新版本或 `rev`，以及 `Cargo.lock`。

## 构建工具 bern

`bern` 命令与 `Cargo.toml` 里的 Bern 同一版本：命令带着生成各平台工程的模板，与应用依赖的 Bern 一致才能构建。

```sh
cargo install --locked --git https://github.com/AimesSoft/Bern --rev 2f68b2361bbba93a8900fcee1a6d704c0a76c886 bern-cli
```

升级 Bern 时，根目录 `Cargo.toml` 的 `bern`、`erika-core/Cargo.toml` 的 `bern-script`、`.github/workflows/release.yml` 的 `BERN_REV` 与这条命令一起换，再装一次。之后在 Erika 根目录运行 `bern run`、`bern build --target <目标>`、`bern package --target <目标>`；`bern doctor` 列出本机构建各目标还缺什么。

## 本地使用相邻源码

目录排列如下：

```text
RustProject/
  Bern/
  liberika/
  NipaPlay-Next/     # Erika 仓库，也可以使用其他目录名
```

在 Erika 根目录执行一次：

```sh
mkdir -p .cargo
cp .cargo/config.local.example.toml .cargo/config.toml
```

此后直接运行普通 `cargo` 或 `bern` 命令。Cargo 自动读取 `.cargo/config.toml` 中的 `[patch]`：liberika、FFmpeg Rust 绑定和 Bern 改用相邻工作目录，修改后无需推送就能联调。Bern 只需替换 `bern` 一个 crate：`bern` 命令看到 Cargo 实际用的是本地检出，生成的宿主也从这个检出取其余 Bern crate。这时的 `bern` 也要来自这个检出（在 Bern 目录里 `cargo install --path crates/bern-cli`，或直接运行它 `target/debug/` 下的 `bern`），模板才与本地改动一致。路径相对于 Erika 根目录；源码放在其他位置时修改这个文件即可。

`.cargo/config.toml` 已被 Git 忽略，只在本机生效。GitHub 构建不复制它，自动使用 `Cargo.toml` 的 Git 版本。不要把本地路径写回公共清单。

本地 patch 会让 Cargo 调整 `Cargo.lock` 中的来源；该文件仍应提交远程 Git 来源的版本。发布或更新锁文件时，在没有本地配置的干净检出中执行 `cargo generate-lockfile`，再执行 `cargo check --locked`。本地联调用普通 `cargo check`，不加 `--locked`。

## BitTorrent 下载库补丁

`erika-torrent` 通过仓库内相对路径固定使用 `third_party/librqbit` 的 8.1.1（保留原有 `rust-tls` 特性），`third_party/librqbit` 又通过相对路径使用 `third_party/librqbit-dht` 的 5.3.1。直接路径依赖也会传递到 Bern 生成的独立宿主工作区，不依赖根清单的 `[patch]`。上游版本、提交、Apache-2.0 许可证和补丁移除条件见 [ERIKA-PATCHES.md](../third_party/librqbit/ERIKA-PATCHES.md) 与 [ERIKA-PATCHES.md](../third_party/librqbit-dht/ERIKA-PATCHES.md)。

补丁不改变 tracker、代理、节点标识或画质选择，只修正影响可用性与连接调度的底层缺陷：

- 分段握手补读（`third_party/librqbit/src/read_buf.rs`）：在原有总超时内补读完整 68 字节握手并保留同包后续消息；
- 状态感知的读空闲超时、并发拨号槽位分离与请求配额上限（`peer_connection.rs`、`chunk_tracker.rs`、`torrent_state/live/mod.rs`）：持有所需分片等待放行的节点覆盖 BEP 3 的 120 秒 Keep-Alive 窗口，空壳节点快速释放，避免 `Unchoke/Choke` 抖动溢出 128 请求配额，拨号超时不挤占活跃槽位；
- Windows UDP DHT 抗瞬态 ICMP 差错（`third_party/librqbit-dht/src/dht.rs`）：`DhtWorker::framer` 忽略 Windows Winsock 的 `10052`/`10054`/`10040` 等单包 ICMP 重置，防止 DHT 工作线程启动后因连接重置退出。

运行协议与 DHT 回归：

```sh
cargo test --manifest-path third_party/librqbit-dht/Cargo.toml --lib
cargo test --manifest-path third_party/librqbit/Cargo.toml --no-default-features --features rust-tls --lib
```

应用封装另运行 `cargo test -p erika-torrent`。以上修复不保证特定冷门资源必然有做种者，实际恢复仍需核对校验分片与下载进度。

## 原生播放依赖

Cargo 管理 Rust 源码，FFmpeg、libass 等原生库仍需按 liberika 的构建文档准备。下面的命令自动定位 Cargo 实际选中的源码，Git 下载和本地 patch 都适用：

```sh
ERIKA_LIBERIKA_ROOT="$(python3 web/dependencies.py)"
cargo run --manifest-path "$ERIKA_LIBERIKA_ROOT/Cargo.toml" -p xtask -- \
  deps build --all --profile lgpl --target aarch64-apple-darwin
cargo check --workspace
```

Windows 在 “x64 Native Tools Command Prompt”（或先运行 Visual Studio 的 `vcvars64.bat`）里构建，另需 Git for Windows 或 MSYS2（`sh` 与 `make`，MSYS2 不在 `C:\msys64` 时设 `MSYS2_ROOT`）、NASM、Python 3、LLVM（`bindgen` 用的 `libclang`）和 Visual Studio 的 CMake 组件。Bern 的 Windows 应用静态链接 C 运行库，原生库也要静态链接（`--crt static`）：

```bat
cargo run --manifest-path ..\liberika\Cargo.toml -p xtask -- deps build --all --profile lgpl --target x86_64-pc-windows-msvc --crt static
```

Linux 用发行版的 FFmpeg（8.x 或更新，`pkg-config` 要能找到 `libav*` 的开发文件；Arch 已是 9.x，同样可用）、PulseAudio（PipeWire 桌面装 `pipewire-pulse`）、Vulkan 头文件与 X11 / Wayland 的开发文件；libass 用 liberika 的脚本打补丁后编成静态库，在 liberika 的源码目录里运行 `bash scripts/build_linux_libass.sh`。liberika 的 Linux 渲染器要用它仓库里的 `third_party/wgpu-hal`：本地配置的 `[patch.crates-io]` 指向相邻检出里的这一份，`erika-core` 把 `wgpu-hal` 锁在同一个版本，否则 Cargo 会选 crates.io 上更新的发布版而不用补丁。Bern 生成的宿主是单独的 Cargo 工作区，不沿用 Erika 清单里的 `[patch]`；发行脚本 `.github/scripts/native-deps.sh` 将补丁写入根 `.cargo/config.toml`，让生成宿主同样读取它。

其他目标的原生依赖准备见 [liberika 构建说明](https://github.com/AimesSoft/liberika/blob/feat/bern-web-playback-20261023/docs/building.md)。

Web 构建、测试素材生成和静态站点测试也通过 Cargo 定位 liberika。只有显式设置 `LIBERIKA_DIR` 时才使用指定目录；不再默认依赖 `../liberika`。
