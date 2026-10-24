# 按模块编译 Erika

使用相邻 Bern 仓库的新 CLI：

```sh
(cd ../Bern && cargo build -p bern-cli)
../Bern/target/debug/bern build --target android --exclude-module downloads
```

`run`、`check`、`package`、`ir` 同样接受 `--exclude-module`。多个 ID 可用逗号分隔或重复传参。不传则编译全部模块。

当前模块为 `account`、`danmaku`、`enhance`、`anime`、`library`、`sync`、`sources`、`remote`、`downloads`。排除组合示例：

```sh
# 移除追番及依赖追番的同步。
../Bern/target/debug/bern build --target tvos --exclude-module anime,sync

# 移除媒体库及依赖媒体库的网络媒体源、远程访问。
../Bern/target/debug/bern build --target tvos --exclude-module library,sources,remote

# 移除账号及依赖账号的同步。
../Bern/target/debug/bern build --target tvos --exclude-module account,sync

# 仅保留外壳与基础播放。
../Bern/target/debug/bern build --target tvos --exclude-module account,danmaku,enhance,anime,library,sync,sources,remote,downloads
```

保留模块仍依赖排除项时会报错，不会自动扩大排除范围。

被排除模块不会出现在生成的 Cargo 直接依赖、组合协议、页面、模块开关和首次启动的模块选择中。外壳中的跨模块调用使用 Cargo 可选依赖和 `bern-module-<id>` feature；UI 使用 `build.modules.<id>` 在类型检查前移除引用。

初始页面按追番首页、媒体库、下载页的顺序选择已编译页面；三者均被排除时进入纯播放界面。

已有用户配置只恢复当前编译包含的模块。保留模块仍可在运行时启停；裁剪版中的磁力链接在未编译下载模块时返回“此版本未包含下载模块”。

“纯粹播放”关闭全部可选模块，包括账号；“看番”包含账号、弹幕、媒体库和新番。“可选功能”可单独关闭账号，同时关闭依赖它的账号同步。账号关闭期间隐藏登录入口并停用登录凭据，重新开启时恢复原有登录。旧版非纯粹播放配置首次读取时保留账号，之后显式关闭账号的选择会随配置保存。

可选功能中的“新番”控制首页推荐和新番表。底层 `anime`（追番）模块由“媒体库或新番已开启”决定，只有二者都关闭才停用；仅开启媒体库时仍使用完整番剧详情、剧集列表和播放功能。保存的是用户的功能选择，旧版仅开启媒体库的配置重启后也会自动启用详情模块。账号同步需要账号，以及媒体库或新番中的至少一项。

裁剪产物放在 `target/bern/<target>/without-<排序后的模块ID>/`，其中 `modules.json` 记录选择。默认构建仍在 `target/bern/<target>/`。

Bern 的完整开发约定见相邻仓库 `Docs/Implementation/Module-Builds.md`。
