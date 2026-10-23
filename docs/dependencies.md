# Cargo 依赖与本地联调

仓库内 `Cargo.toml` 的 Bern 和 liberika 使用 Git 依赖，并固定到提交。GitHub 或另一台电脑检出 Erika 后，Cargo 会下载同一份源码，无需本机的绝对路径。

liberika 当前固定到 [PR #168](https://github.com/AimesSoft/liberika/pull/168) 的提交 `27bab58d9816a3b9dda67d22dcb16507d46835b0`，因为播放器需要这批新增 API。这个提交已推送到公开仓库，不依赖 PR 是否合并。后续升级时更新 `rev` 和 `Cargo.lock`。

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

此后直接运行普通 `cargo` 或 `bern` 命令。Cargo 自动读取 `.cargo/config.toml` 中的 `[patch]`：liberika、FFmpeg Rust 绑定和 Bern 改用相邻工作目录，修改后无需推送就能联调。路径相对于 Erika 根目录；源码放在其他位置时修改这个文件即可。

`.cargo/config.toml` 已被 Git 忽略，只在本机生效。GitHub 构建不复制它，自动使用 `Cargo.toml` 的 Git 版本。不要把本地路径写回公共清单。

本地 patch 会让 Cargo 调整 `Cargo.lock` 中的来源；该文件仍应提交远程 Git 来源的版本。发布或更新锁文件时，在没有本地配置的干净检出中执行 `cargo generate-lockfile`，再执行 `cargo check --locked`。本地联调用普通 `cargo check`，不加 `--locked`。

## 原生播放依赖

Cargo 管理 Rust 源码，FFmpeg、libass 等原生库仍需按 liberika 的构建文档准备。下面的命令自动定位 Cargo 实际选中的源码，Git 下载和本地 patch 都适用：

```sh
ERIKA_LIBERIKA_ROOT="$(python3 web/dependencies.py)"
cargo run --manifest-path "$ERIKA_LIBERIKA_ROOT/Cargo.toml" -p xtask -- \
  deps build --all --profile lgpl --target aarch64-apple-darwin
cargo check --workspace
```

其他目标的原生依赖准备见 [liberika 构建说明](https://github.com/AimesSoft/liberika/blob/feat/bern-web-playback-20261023/docs/building.md)。

Web 构建、测试素材生成和静态站点测试也通过 Cargo 定位 liberika。只有显式设置 `LIBERIKA_DIR` 时才使用指定目录；不再默认依赖 `../liberika`。
