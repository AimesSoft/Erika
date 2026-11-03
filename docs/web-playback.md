# Erika 浏览器播放

`web_app` 已接入 liberika Web 后端。Erika 的 Rust 引擎与 Bern 界面在浏览器运行，视频交给 WebCodecs 解码、Web Audio 输出声音，liberika Worker 使用同一套 Rust 弹幕排版和 libass 绘制字幕，再由 WebGL2 合成到 Bern 视频视图。

## 构建模式

本文描述纯 Web 的 `web_app` 目标：`platform = "browser"`、`ui = "web"`、
`web_mode = "app"`。项目还提供 `web_ui` 目标，构建和启动见 [WebUI（独立引擎）](README.md#webui独立引擎)。其中 `ui` 选择网页界面渲染器，`web_mode` 决定引擎在哪里运行。

| 模式 | 引擎位置 | 部署内容 |
|---|---|---|
| `web_mode = "app"`（`web_app`） | 浏览器中的 `app.wasm`，liberika 也在浏览器内运行 | 静态 HTML、JS、CSS、WASM 与资源 |
| `web_mode = "ui"`（`web_ui`） | 独立原生引擎宿主，网页通过 WebSocket 连接 | 网页资源、原生 `erika-host` 及 CLI/MCP 程序 |

开发测试和纯静态部署使用同一条 liberika 播放路径。静态文件服务器只提供文件，
不承担 Erika 业务逻辑、解码、字幕处理或弹幕绘制。

## WebUI 本地播放

`web_ui` 同样打包 `web/` 播放扩展。Erika 业务引擎运行在原生 `erika-host`，页面中的 WASM 只承担 Bern UI 协议桥；liberika Worker 在浏览器中解码本地文件、绘制字幕和弹幕。文件选择、拖放及文件启动入口先登记 `blob:` 地址、原文件名和页面标识，再调用原生引擎的打开命令，不上传视频内容。

原生 `kernel/web/remote.rs` 通过现有认证 WebSocket 收发带序号的播放指令与状态。重连时保留已确认的指令序号、去重操作结果；换片丢弃旧状态；多个页面打开同一界面时，本地文件仍由选择它的页面播放。关闭该页面后需重新选择文件。字幕和弹幕读取、截图下载也由浏览器执行。

```sh
bern build --target web_ui --release
target/bern/web_ui/bin/erika-host --port 4174
# 把宿主打印的完整 open 地址（含 token）传给测试
ERIKA_WEBUI_URL='http://127.0.0.1:4174/#token=...' \
PLAYWRIGHT_MODULE=/path/to/playwright/index.mjs node web/webui.test.mjs
```

该回归使用本地 MKV/H.264/AAC/ASS 和 WebM/VP9/Opus 合成视频；从原生 CLI 读取真实播放进度，并通过 CLI 暂停和跳转浏览器播放器，覆盖外挂字幕、弹幕、截图、多页面切换与坏文件恢复。报告和截图写入 `target/webui-playback-review/`。

2026-10-23 在 Chrome 155.0.8059.86 通过上述 WebUI 播放回归；同一脚本以 `ERIKA_WEBAPP_URL` 指向静态 Web App 时也通过共用播放器回归。原生核心测试覆盖本地文件名、实际播放状态、文件所属页面、重连结果去重、换片旧状态过滤及断开超时。另有 Bern 多页面媒体路由测试。完整旧版 `browser.test.mjs` 的弹幕输入框选择器目前与现有未登录界面不符，该 UI 用例未算入本次通过项。

## 构建和启动

需要用 Cargo 装好的 `bern`（见[构建工具](dependencies.md#构建工具-bern)）。先按 liberika 的 `docs/web.zh.md` 安装 Emscripten、wasm-bindgen 和其他构建依赖；Rust 需要 `wasm32-unknown-unknown`。

```sh
bern build --target web_app --release
bern run --target web_app --release --port 18766
```

`Bern.toml` 的 `web_extension` 调用 `web/build.py`，自动构建 liberika，并把 JS、两个 WASM、Worker 和许可证放到 `target/bern/web_app/extension/`。回退字体只在 Rust WASM 内嵌一次，libass 使用同一份字体数据；构建时会清理旧版独立的 `subfont.ttf`，不再额外下载字体。不需要手工修改生成目录。liberika 源码位置由 Cargo 解析（Git 依赖或本地 patch），也可通过 `LIBERIKA_DIR` 显式指定。仅在 Web C 依赖已经编译且没有变动时，可设置 `LIBERIKA_SKIP_NATIVE=1` 加快迭代；Rust、C 桥接及 JS 仍会更新。

部署使用 HTTPS 或 localhost，并正确提供 `.wasm` 和 JavaScript 文件。部署更新时对入口和运行时文件使用重新验证缓存策略，或将整个发布包放在新的版本目录。无需 SharedArrayBuffer 或跨源隔离。`bern package --target web_app` 会包含浏览器扩展资源。

## 纯 Web 的 CLI / MCP 控制

`web_app` 的 `channels = ["ipc"]` 生成浏览器命令桥和协议工具。引擎仍在页面的 WASM 中运行，本机 Python 服务只提供静态文件、转发请求，不运行 Erika 引擎。

```sh
# 也可用 bern run --target web_app --release --port 18766
python3 target/bern/web_app/serve-bridge.py --port 18766
# 打开服务打印的 URL，令该标签页连接命令桥，然后执行：
target/bern/web_app/bin/erika state
target/bern/web_app/bin/erika open-onboarding
target/bern/web_app/bin/erika sessions
```

MCP 程序是 `target/bern/web_app/bin/erika-mcp`（stdio），工具名沿用同一目录，例如 `erika_open_onboarding`、`bern_state`、`bern_sessions`。它与桌面版的 IPC 命名空间分开，不会操作桌面播放器。多个网页同时连接时，CLI 用 `--engine ID` 指定；MCP 默认要求只有一个已连接网页。

网页必须保持打开；每次刷新都会产生新的引擎实例。桥接凭据只放在启动 URL 的 fragment 和该标签页的 sessionStorage 中。本机服务检查 Host、同源 Origin 和凭据，IPC 目录与 socket 仅当前用户可访问。关闭网页会移除其 socket；异常断开在心跳过期后清理。请求超时会报告结果未知，不自动重试。

支持命令、查询、状态、命令目录、窗口与 UI 操作；持续状态订阅暂不支持。静态发布包不含本机工具，也不要求启动桥接；普通静态托管没有本机命令入口。需要控制时使用上述本地服务和配套工具。

## 纯静态发布

```sh
bern package --target web_app
```

macOS 的发布包为 `target/bern/dist/erika-0.1.0-web.zip`，解压得到 `erika/`。
把这个目录整体上传至静态网站，访问 `/erika/`；也可将目录中的内容部署到网站根目录。
发布机器不需要 Rust、Bern CLI、原生 Erika 或解码服务。保留 `extension/` 与
`resources/` 的目录结构；WASM MIME 类型为 `application/wasm`。

本机查看解压后的纯静态包：

```sh
python3 -m zipfile -e target/bern/dist/erika-0.1.0-web.zip target/web-static-preview
python3 -m http.server 18767 --bind 127.0.0.1 --directory target/web-static-preview
# 浏览器访问 http://127.0.0.1:18767/erika/，选择本地 MKV 即可播放。
```

上面的标准静态服务器用于本地文件播放。若还要由同一服务器提供大型远程视频，
使用支持 HTTP Range 的静态服务器，例如 Nginx，或 liberika 的 `scripts/serve_web.py`。
线上使用 HTTPS；直接打开 `file://` 下的 HTML 不能启动 WASM 模块与 Worker。

## 已接入的操作

- 从「打开」选择本地视频，或打开 HTTP/HTTPS 视频链接。浏览器的 File 直接交给 Worker 分块读取，不将整段视频复制进应用 WASM。
- 播放、暂停、时间轴定位、音量、倍速、音轨选择、真实容器章节及章节定位。
- 内嵌 ASS/SSA，外部 ASS/SSA、SRT、WebVTT，字幕选择、关闭、字号和独立双倍采样。
- 本地 XML/JSON 弹幕，开关、字号、不透明度、区域、速度、延迟、去重和类型屏蔽。多个弹幕文件追加；弹幕解析在 Worker 完成后才更新数量与成功提示。
- 全屏播放器与迷你播放器共用同一个 Canvas/Worker；打开菜单不会重建播放器。连续参数输入按类型合并待处理更新，透明度不重建字形图集。
- 截图通过浏览器下载，历史缩略图使用临时 blob URL。关闭媒体会清空旧字幕、弹幕、章节和视频帧，并停止音频。

## 当前边界

视频和音频编码必须受当前浏览器 WebCodecs 支持；不支持会显示错误。优先硬解只是向浏览器提出的偏好，不能保证实际选择硬件解码。网络文件需要 CORS；大文件需要 HTTP Range 及暴露 `Content-Range`，具体读取限制见 liberika 文档。

当前是 SDR，未移植桌面的 HDR/EDR、Dolby Vision 和画质增强。浏览器界面不显示画质增强选项。倍速仍会改变音调；内嵌位图字幕、内嵌非 ASS 字幕转换、DRM、未知长度直播不在本次支持范围。内嵌 ASS 首次 seek 时仅预读前 5 秒，可能漏掉更早开始的长事件；外部完整 ASS 无此限制。

浏览器的账户、在线弹幕匹配/发布、自建识别服务、BT 下载及持久媒体库仍受原有 Web 适配限制，本次没有宣称这些桌面功能已接通。浏览器存储目前是会话实现，重新打开页面需再次选择本地文件，不能直接使用桌面的 `file://` 路径或邻接文件扫描。

## 回归验证

```sh
python3 web/test_fixtures.py
# 单独终端：带 HTTP Range 的本地静态服务器
ERIKA_LIBERIKA_ROOT="$(python3 web/dependencies.py)"
python3 "$ERIKA_LIBERIKA_ROOT/scripts/serve_web.py" --port 18766 --directory "$PWD/target/bern/web_app"
# PLAYWRIGHT_MODULE 指向本地安装的 Playwright 的 index.mjs
PLAYWRIGHT_MODULE=/path/to/playwright/index.mjs node web/browser.test.mjs
```

自动测试使用合成视频，不读取用户影片。覆盖实际文件选择、H.264/AAC/ASS、VP9/Opus、2000 条弹幕、暂停跳转、真实滑块拖动、实时参数、字幕和音轨切换、菜单及迷你播放器切换、截图、关闭后重开、HTTP Range、章节、窗口尺寸切换以及坏文件后的恢复；同时检查 Canvas 尺寸与截图中的实际彩色像素。报告与截图写入 `target/web-review/`。自动回归使用 Chrome。另在 Safari 27.0.1 使用用户提供的 HEVC Main 10 / Opus / 双 ASS / 五份字体附件 MKV 验证打开、字幕、暂停、章节定位与连续快进；原文件未打包。Firefox 未验证。窄屏竖向播放器仍沿用已有的等比缩放场景，本次没有重做手机竖屏控件布局。

Safari 的音频必须在真实用户手势中启动。扩展初始化时创建 `AudioSession`，先于文件选择器注册输入监听，再交给延迟创建的 Player；音频尚未启动时，播放请求不会堵塞 seek、pause 和 close。HEVC 跳转会丢弃首次随机访问点后依赖前一个 GOP 的 RASL 帧，后续连续解码保留正常帧。

纯静态发布包另有独立回归：

```sh
python3 web/static_test.py
# 指定其他发布包：python3 web/static_test.py --archive /path/to/erika-web.zip
```

脚本把 ZIP 解压到临时站点的 `/erika/` 子目录，启动仅提供文件和 Range 的服务器，
从包内读取命令声明并运行同一组播放回归。浏览器阻断其他 origin 的网络请求与所有
WebSocket；测试确认没有连接原生引擎、缺失的独立字体请求或对开发目录的资源依赖。
测试结束后自动关闭服务器，保留 `target/web-static-review/report.json` 与截图。
2026-10-22 已在 Chrome 155.0.8059.85 通过，`externalRequests`、`webSockets` 和
`fontRequests` 均为空。Playwright 默认从相邻 liberika 的 `target/web-tests/node_modules`
加载，也可通过 `PLAYWRIGHT_MODULE` 指定。

## 播放器悬浮控件（2026-10-22）

Web 悬浮控件使用不透明白色文字／图标和沿内容轮廓的阴影。Bern 原先把裸 IconButton 的 shadow 生成为 box-shadow，导致快进／快退出现空按钮框的投影；现在透明图标按钮使用 drop-shadow，带填充的按钮仍投影其底板。

`web/player.css` 通过现有浏览器扩展加载，并由 `web/build.py` 打包。样式限定在播放器控制 ID：顶栏、字幕、倍速、跳过、弹幕开关等去掉默认描边，播放键与启用的弹幕开关保留主题色底板，键盘焦点保留明确的焦点环。菜单／气泡内容排除在这些规则之外；没有对承载气泡的父容器加 filter。时间轴与输入提示使用明亮的不透明颜色，进度条沿用 Rust UI 的阴影。

验证：34 项 Bern Web 运行时测试通过，Web Release 构建通过；浏览器中以合成的亮色视频检查 1280×800、1280×720 桌面和 844×390 横屏的控件颜色、描边、内容阴影与设置气泡底板。截图保存在本地 `target/web-overlay-review/`。

播放设置关闭修正（2026-10-22）：Bern Web 的 `PopoverButton` 在 `beforetoggle` 同步回写开关状态并释放内容，避免浏览器延后或合并 `toggle` 事件时，播放器状态刷新重新打开旧气泡并重复追加设置。气泡外的按下、抬起和点击由共用的关闭处理消费，这次操作只收起气泡，下一次点击画面才切换播放；气泡内部控件、触发按钮和 Escape 保持正常。快捷弹幕设置的 `SplitButton` 共用外部点击处理。

菜单关闭补齐（2026-10-22）：字幕入口和「更多」使用 `Menu`，倍速使用 `Picker` 下拉框，原先没有接入上述拦截。关闭处理现移到 Bern 基础运行时，由 `PopoverButton`、`SplitButton`、`Menu` 和下拉 `Picker` 共用。自动气泡打开时，Escape 优先由菜单或浏览器处理，不再同时触发播放器的退出快捷键。`web/menu-regression.mjs` 由实际视频回归和 WebKit 界面回归共同调用，覆盖三种播放器菜单在播放／暂停时的外部点击、下一次画面点击、Escape、倍速选项生效，以及字幕菜单转入播放设置。

弹幕输入失焦修正（2026-10-22）：播放器在 Rust UI 中用 `FocusRegion { Row { … } }` 包住弹幕输入框与发送／收起按钮，Bern Web 按区域处理首次外部按下、抬起和兼容点击。该操作只失焦，不提交或清空草稿；区域内控件和下一次外部点击正常执行。`web/index.js` 已移除按弹幕控件 ID 拦截的专用逻辑。`web/composer-regression.mjs` 由实际视频回归和 WebKit 界面回归共用，覆盖播放／暂停、草稿保留、再次点击、空格输入与失焦后的快捷键，以及发送按钮可点击（测试拦截发送动作，不发布弹幕）。

`web/browser.test.mjs` 覆盖播放与暂停时的实际外部点击、下一次点击、内部音频分页、同一轮事件中的反复开关，以及桌面和横屏下的唯一内容与 Escape 关闭；截图为 `target/web-review/settings-1280.png` 和 `settings-844.png`。

`web/settings.webkit.test.mjs` 使用应用的示例播放时钟验证 WebKit 的同一套界面，覆盖鼠标及触摸，避免依赖无头 WebKit 的 GPU 解码。它也覆盖 WebKit 触摸的 `click.pointerId` 与按下编号不同的兼容情况。安装 Playwright 的 WebKit 后，沿用上面的 `PLAYWRIGHT_MODULE` 与服务器运行该脚本，报告写入 `target/web-settings-webkit-review/`。Chrome 无头环境无法创建 WebGL2 时，可以设置 `ERIKA_SOFTWARE_WEBGL=1` 使用软件 WebGL 跑完整播放回归。

进度条命中区域修正（2026-10-22）：章节刻度的 `Positioned` 透明容器曾覆盖整条轨道中线，导致点击中线被拦截，偏下点击才生效。Bern Web 现在让轨道容器与纯绘制刻度透过指针事件，章节名称按钮保留命中。实际播放回归使用带三段章节的合成视频，检查桌面和横屏的轨道中线、刻度中心、点击定位、拖动及章节名称跳转；Bern 的 `rail-hit-testing.test.mjs` 另覆盖 Chromium / WebKit 的鼠标和触摸。
