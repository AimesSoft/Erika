# TMDB 主媒体库验收记录

更新：2026-11-11。**五端完整验收尚未完成；用户随后明确要求推送并发布 Bern 与 Erika，本次按该发行授权推进，不将其记为总验收通过。** 本文只记录已执行的范围，不把编译、单元测试或本地 fixture 当作生产验证。

## 环境与隔离

- 分支：`feat/tmdb-library`。原有工作区修改保留，尚未提交或推送。
- macOS 测试包：`com.aimessoft.erika.tmdb.acceptance`，独立 HOME；两个账号均未登录。
- 复用 `target/tmdb-work/runtime-current.t4qahsro` 隔离源码及现有构建缓存；公共资料指向本机 4319 fixture，WebDAV 指向本机 4320 内存仓库。网站账号接口未改指向，也没有登录或写真实账号。
- 验收使用临时 Bern 手势组合，尚不是正式依赖 pin 的可复现构建。早期 fixture 图片为空；后续启用官方 TMDB CDN 的公开海报、背景与 Logo 样本，具体渲染范围见下节。这不证明生产网关或真实作品身份匹配。
- 旧格式验收库未迁移、补表或清空；2026-11-10 复核 SHA256 仍为 `da63a52bdf62b3fc863995d79f497584be3ba97c1de4c55d1d301afbafab6cfc`。

## 自动化与构建

2026-11-10 在上述隔离源码运行联合回归：

| crate | 通过 | 跳过 |
| --- | ---: | ---: |
| erika-anime-core | 70 | 1 |
| erika-catalog | 28 | 0 |
| erika-core | 171 | 3 |
| erika-library-core | 32 | 0 |
| erika-remote-core | 9 | 0 |
| erika-store | 176 | 0 |
| erika-sync-core | 29 | 1 |
| 合计 | 515 | 5 |

跳过项为已标记 ignored 的真实播出数据、三个插件市场/安装网络用例和真实 WebDAV 服务器测试；没有把它们计入通过数。本机原生应用另执行了下述 WebDAV 往返，不等同于所有服务商互操作测试。

- `erika-core`、`erika-anime-core` 的 `wasm32-unknown-unknown` 编译通过；不是浏览器运行验证。
- macOS 应用、CLI 和 MCP 重建成功；已启动新应用并通过实际控件交互。
- 包含电影收藏与已看冲突保护、账号共享锁与 uncertain 恢复、评分清零公开命令、短评草稿保护、插件详情访问代次保护。
- 同日补入内核错误分类和来源说明回归后，单独重跑 `erika-core --lib`：**174 通过、0 失败、3 ignored**，并重建 macOS 应用/CLI/MCP。其余 crate 未随这一轮重跑；不将上表整轮结果改记为最新联合验收。
- 随后修正插件来源恢复、稳定地址和导入事务，再跑三个 crate：`erika-store` **184 通过**、`erika-sync-core` **31 通过 / 1 ignored**、`erika-core` **174 通过 / 3 ignored**，均无失败；macOS 应用/CLI/MCP 再次重建成功。新增契约同时运行于 SQLite 和浏览器内存存储。随后 `erika-store` 与 `erika-sync-core` 的 `wasm32-unknown-unknown` 编译也通过；未把它算作浏览器运行或跨刷新持久化验证。
- 随后补齐来源凭据与扩展数据密封：`erika-store` **184 通过**、`erika-library-core` **32 通过**、`erika-sync-core` **38 通过 / 1 ignored**，store/sync 的 wasm 编译通过。恢复通知调整后另跑 core/sync：**174 / 38 通过、3 / 1 ignored**，并重建 macOS 应用。不是重新执行上表全部项目。
- 挂载弹弹play目录改为无凭据稳定地址后，联合回归 `erika-core` **176 通过 / 3 ignored**、`erika-dandanplay` **30 通过**、`erika-library-core` **32 通过**；合计 **238 通过、0 失败、3 ignored**。新增测试覆盖配置来源的精确端点授权、续播重新解析当前密钥、历史/队列/插件上下文不带密钥，以及没有内核时不模拟远程播放。

- WebDAV 整份配置密封后，`erika-sync-core` **40 通过 / 1 ignored**，core/library/sync 三个 crate 的 wasm 编译通过，macOS 应用/CLI/MCP 重建成功。新增两项回归覆盖错口令、无口令、空密码、损坏密文、明文配置拒绝及 Vault 拒写时不更换目标地址。
- 来源文件直接播放与内容类型修正后，第一轮核心回归揭示续播标题仍取远程端点末段 `video-1`；修正历史保存为来源/规范标题后，core/library 联合复验 **176 / 33 通过、3 / 0 ignored**，无失败，macOS 应用/CLI/MCP 重建成功。

## 原生挂载来源直接播放与密钥轮换

2026-11-10，用本机 4322 fixture、独立凭据验收 HOME 和假来源密钥执行真实界面操作；两账号未登录，未设置同步口令：

- 初次原生检查发现未匹配文件没有播放入口，且电影来源目录被写死标成番剧。现所有可读文件共用“播放”按钮，通过 library 事件进入主播放器既有识别、进度和内核加载流程；内容类型读取来源自身配置。
- 实际点击未匹配行的播放按钮，30 秒 H.264/AAC 片在 **2.333 秒**显示红色画面，标题为来源文件名。通过真实原生菜单暂停，seek 到 **12 秒**显示绿色画面。
- 返回媒体库保存进度后，fixture 将假密钥换为第二代，应用编辑来源保存新密钥，再执行“继续上次”。内核恢复至 **12.147 秒**，截图为绿色；两代流请求均鉴权通过，未修改历史地址，也未制造 TMDB 绑定。
- 目录、公开状态、UI inspect、命令响应、历史/文件/地址/来源/进度表及该轮应用进程日志均不含假凭据。fixture 未收到认证缩略图请求。此轮文件尚未绑定弹幕，未触发弹幕请求，不将其计为弹幕端点验收。
- 原生应用导出无口令 `.ebp`，经实际备份格式解码后检查：账号、来源密钥、扩展和 WebDAV 配置分类均未导出；settings 无来源 token，最近播放保留无凭据地址和 **12.147 / 30 秒**进度。不是仅扫描压缩文件字节。
- 新版“同步口令”和“备份文件”页面截图已查看，均说明 WebDAV 配置也受口令保护；没有点击保存口令或重试系统钥匙串授权。

本机证据：`target/tmdb-work/check_native_source_key_playback.py`、`source-key-playback-results.json`、`source-key-directory-with-play.png`、`source-key-red-playback.png`、`source-key-green-seek.png`、`source-key-rotated-resume.png`、`source-key-{paused,resumed}-{state,ui}.json`、`source-key-plain-backup-{state,decoded}.json`、`source-key-sync-passphrase.png`、`source-key-sync-backup-settled.png`。目录、起播、续播与两张设置截图均已查看；不等同生产 HTTPS 或其他平台验证。

## 原生认证来源播放

本机 4321 媒体 fixture 同时检查假 query、Authorization 和 Cookie；只记录各项是否满足，不记录原始请求信息。测试插件通过宿主来源接口完整发布正常/损坏两个条目，再从稳定 `erika-source://` 地址解析当前 URL 和请求头；未开发 Jellyfin 插件，未登录账号。

- 正常媒体实际出画面。第二轮用每十秒换色的 30 秒 H.264/AAC 视频：2.155 秒处为红色，实际菜单暂停后 seek 到 15 秒，截图为绿色；恢复播放后内核时间推进至 17.053 秒。三张截图均已查看，不仅检查先行更新的时间状态。
- 损坏媒体同样携带完整认证信息，显示“无法播放 / 媒体播放失败”，没有把私有 URL 或内核原始诊断放进错误提示。
- Erika 适配层将六类 `PlayerError` 穷尽映射为固定中文类别，覆盖同步返回、异步事件和该层的错误日志。针对性适配器测试 3 项通过，其中诊断测试向四个字符串类别注入假 URL、路径 token、query、Authorization、Cookie 和服务端正文。
- 本次公开状态、UI inspect、命令响应及进程日志均未含测试哨兵。**这不等于证明 liberika/FFmpeg 所有内部日志路径都已脱敏**；未修改 liberika。
- 发现并修正来源影片被标成“本地文件”：直接打开、最近播放、规范媒体播放、fallback 队列和解除绑定恢复共用逻辑来源分类。真实 JS 回归覆盖最近播放、队列、人工解除及外部解除；原生正常与失败截图均显示“网络链接”。

本机证据：

- `target/tmdb-work/check_native_private_playback.py`
- `target/tmdb-work/private-playback-results.json`
- `target/tmdb-work/private-playback-red-start.png`
- `target/tmdb-work/private-playback-green-seek.png`
- `target/tmdb-work/private-playback-bad-link.png`
- `target/tmdb-work/private-playback-{paused,bad}-{state,ui}.json`

上述只覆盖本机原生 HTTP fixture。生产 HTTPS/CORS、其他平台，以及需要请求头的片头分析仍未完成：当前 `liberika::opening::opening_profile(uri, seconds, per_second)` 无请求头参数，不能将主播放成功等同于二次分析成功。

## 原生界面与迟到回包

### 短评

- 打开已有短评，实际清空输入框；之后 UI inspect 仍为 `value=""`，截图已查看，没有填回旧正文。
- 输入“未登录时保留草稿”，点击提交。原生错误弹窗提示“请先登录 Bangumi 账号”，输入框保留草稿。
- 这是未登录失败路径；没有把它算成真实 Bangumi 发布成功。

本机证据：

- `target/tmdb-work/account-review-empty-verified.json`
- `target/tmdb-work/account-review-empty.png`
- `target/tmdb-work/account-review-failed-submit.json`
- `target/tmdb-work/account-review-failed-submit.png`

### 同一详情的离开与返回

根因：只比较作品与季不能区分两次访问。现为每次详情访问生成独立代次，异步插件请求捕获该代次，回包重验；关联修改和账号重置也使旧观察失效，普通刷新不更换代次。

通过实际插件设置按钮触发公共详情请求，fixture 延迟 5 秒；在返回之前操作：

1. 特别篇 → 第一季 → 特别篇。
2. 关闭详情 → 重新打开同一作品 → 特别篇。

两次旧回包均未改变已看值或 revision。随后无延迟的新页面标记及撤销均成功，排除“所有请求都被拒绝”的假通过。最后截图已查看，S00E02 保持未看；没有调用真实账号写接口，也没有手动插件 tick。

本机证据：

- `target/tmdb-work/check_native_navigation_aba.py`
- `target/tmdb-work/native-navigation-aba-results.json`
- `target/tmdb-work/native-navigation-final-ui.json`
- `target/tmdb-work/native-navigation-final.png`

## 当前格式备份与 WebDAV

### `.ebp`

2026-11-09，实际导出并导入当前格式备份：

- 同库重复导入 `changes=0`，检查的完整行不变。
- 空白接收实例导入 `changes=31`：30 个媒体节点、2 条收藏记录、1 条已看记录。
- 节点 UUID 集合保持；收藏、已看及空账号队列行核对一致。
- 当时备份的评分/短评为 None，非空短评是导入之后的隔离测试种子。因此这份备份不能证明评分清零、短评清空、非空 outbox 或所有来源资料的往返。

### 来源、清空值与发送队列边界

2026-11-10，先在原隔离双实例执行 `.ebp` 导出/导入：首次 13 项，重复 0 项。评分 `Some(0)`、短评 `Some("")`、17.25/30 秒文件进度保留，发送端非空 `sending` 收藏队列未被复制。该轮同时发现插件来源只恢复了普通 `source` 行，没有恢复 `plugin_source` 归属，不能算完整来源恢复通过。

修正后，用两个**新建**隔离 HOME 再测；旧库和失败样本保留：

- 正常注册与导入共用插件 / 来源 / 服务器身份校验。相同插件、来源 ID 换服务器会失败；来源导入错误不再吞掉。媒体、来源和数据库设置在同一写边界回滚，凭据与外部 Store 写入放在数据库成功之后。
- SQLite 和浏览器契约复现了另一问题：来源编号从 2 变成 3 后，已导入最近播放不能解析。内部地址现为 `erika-source://item/<稳定身份编码>`，编码插件、来源外部 ID、服务器和条目版本；不含本地 rowid，不做旧地址迁移或别名。
- 原生应用实际导出新备份；接收实例先有一个本机来源，导入 **12 项**，重复导入 **0 项**。插件来源在发送端为 2、接收端为 3，所属插件 / 来源 / 服务器完全一致。
- 导入后、插件尚未安装时：接收端 `plugin_source=1`，`plugin_source_scan=0`、`plugin_source_item=0`；没有复制扫描缓存、租约或分页状态。
- 两端评分 0、空短评、文件进度和最近播放稳定地址一致，发送端本地 mark revision 3、接收端 revision 1；发送端 `account_sync=1`，接收端两个账号队列表均为空。
- 通过原生公开命令安装测试插件，初始化重新注册并完整扫描。仍使用来源 3，没有重复来源；文件行号与发送端不同，但人工媒体绑定和进度仍指向正确影片。
- 从导入的「备份验收电影」最近播放实际开流，暂停状态为 **17.664 / 30 秒**，截图为绿色段，标题为规范电影标题，截图已查看。验收脚本最初按数据库里的规范标题查最近行而失败；运行时该行保留来源标题，改按稳定地址核对后完成证据保存，未修改产品代码来迁就断言。

本机证据：`target/tmdb-work/boundary-backup-{comparison,decoded}.json`、`source-roundtrip-before-register.json`、`source-roundtrip-*-result.json`、`source-roundtrip-resumed-{state,ui}.json`、`source-roundtrip-resumed.png`、`source-roundtrip-decoded.json`。仅使用假账号队列标识及本机认证 fixture，两账号未登录；不是生产来源或受保护凭据往返验证。

### 非空已看发送队列的 `.ebp` 边界

2026-11-11，沿用来源导出/接收实例，未重新初始化数据库。发送端已有一条合成 Movie72 的手工已看记录，以及处于 `sending` 的 WatchSync 和非空 SubjectSync；两个账号始终未登录，队列使用隔离假账号标识。

- 原生导出的 `.ebp` 经实际格式解码：规范 `media.watched` 被导出，发送 operation、账号 lane 标识和 claim token 均不在解码内容中；导出没有改变发送端 `sending` 状态。
- 接收端导入前 `media_watched=0`、两个账号队列表为空。通过应用实际 `sync import-from` 导入 **3 项**，恢复规范 Movie UUID、手工已看标志和原更新时间；接收端 revision 为 1。
- 导入后 `account_sync=0`、`account_watch_sync=0`；重复导入 **0 项**，已看完整行及两个队列表保持不变。
- 这是无口令本机备份验证，不涵盖受保护凭据，也不把文件备份结果当作 WebDAV 非空队列验证。

证据：`target/tmdb-work/watch-backup-{seed-receipt,decoded,export-queues,native-roundtrip}.json`、`watch-backup-receiver-{import,repeat}-state.json`；接收端驱动为 `check_watch_backup_native.py`。

### 双实例 WebDAV

两个隔离原生实例串行运行，共用无凭据的本机内存 WebDAV fixture：

- 接收实例首次上传 Created；原始实例下载并恢复 1 项，得到隔离短评。
- 原始实例通过应用命令把特别篇收藏改为 Watching，将 S00E02 改为显式未看，上传 2 项。
- 接收实例下载 1 个变更文件、恢复 2 项。两库 `media_mark`、`media_watched` 完整行一致；`account_sync`、`account_watch_sync` 均为空。
- 两端重复同步的 uploaded/downloaded/restored 均为 0。
- 反向修改没有丢掉短评；旧格式验收库哈希不变。往返截图已查看。

本机证据：`target/tmdb-work/account-webdav-*-state.json`、`account-webdav-roundtrip-repeat.json`、`account-webdav-roundtrip-ui.json`、`account-webdav-roundtrip.png`。

### 不同来源编号的 WebDAV 同步

新建独立本机仓库 `/source-portable-20261110-0143`，使用上述来源编号为 2 和 3 的原生实例：

- 接收实例创建仓库 `Created`，随后重复同步三个计数均为 0。
- 发送实例首次同步 `downloaded=0, restored=3, uploaded=0`，记录来自仓库快照；随后重复同步三个计数均为 0。
- 两库来源编号仍分别为 2、3，文件进度和最近播放记录一致；最近播放的稳定地址都可解析到各自发布的来源条目，接收端两个账号队列表仍为空。

本机证据：`target/tmdb-work/source-webdav-*-state.json`、`source-webdav-comparison.json`。两边已经通过 `.ebp` 建立来源，不是从空库经 WebDAV 新建来源，也未测试受保护来源凭据。

### 非空已看队列与空库 WebDAV 恢复

2026-11-11，启动新的本机内存仓库 `/watch-empty-20261111-0045`，没有覆盖旧仓库或清空旧数据库。

- 发送端保留 `account_sync=2`、`account_watch_sync=1`，通过原生公开同步命令创建仓库。解码真实上传快照，含规范已看记录，但无 WatchSync operation、假账号 lane 标识及 claim token；同步前后完整发送队列行不变。
- 新建独立 `watch-webdav-empty-home`，由应用首次初始化当前格式空库，不导入 `.ebp`、不预装来源。首次 WebDAV 同步 `restored=44, downloaded=0, uploaded=0`：从快照恢复媒体、已看及一个插件来源归属。
- 接收端两个账号队列表为空；来源扫描缓存没有复制，插件尚未安装，因此没有把归属恢复等同于重扫或播放成功。
- 接收端再次同步三个计数均为 0；手工已看及更新时间保持，两账号始终未登录。无同步口令，不涵盖私密来源凭据恢复。
- 随后在同一空库恢复实例通过公开命令安装测试插件并完整扫描：来源编号 2、插件 / 来源 / 服务器身份均保持，发布两个条目，没有复制扫描缓存或创建重复来源。重新注册生成新的 `generation` 是正常运行状态，验收按稳定身份字段核对，不要求 generation 不变。
- 该仓库实际恢复的最近条目名为“来源测试片”，进度 **10.161 / 30 秒**，并非较早 `.ebp` 样本的 17.25 秒。按真实最近条目发起续播后，标题经人工绑定解析为“备份验收电影”，暂停于 **10.472 / 30 秒**；窗口截图为绿色网格，已查看。规范已看完整行不变，两个账号队列仍为空。脚本曾误用旧样本标题，返回 no_such_recent；更正验收输入后完成，没有修改产品或重新播种。

证据：`watch-webdav-{sender-create,empty-first,empty-repeat}.json`、对应 `-state.json`、`watch-webdav-snapshot-decoded.json`、`watch-webdav-rescan-{before,after,state,ui,playback}.json`、`watch-webdav-rescan-resumed.png`，位于 `target/tmdb-work/`；驱动为 `check_watch_webdav_native.py`、`check_watch_webdav_rescan.py`。

### 私密记录与系统钥匙串边界

- 来源凭据不再混入普通 settings；按“来源种类 + 地址”密封，接收端通过稳定身份查找本机来源编号。扩展 JSON 整体密封，避免按属性名猜测其中的密码。无口令不导出、无/错口令不覆盖本机私密记录；不提供明文旧格式兼容。
- 七项回归覆盖来源编号 2→3、错误来源/插件归属、来源删除后的编号复用、非法扩展 JSON 回滚，以及错误口令同步后再解锁。同步基线记录私密分类是否真的已应用，解锁时不会把本机尚未恢复的记录误上传为删除。
- 新版原生“同步口令”和“备份文件”页面已复核，保护范围包含 WebDAV 配置；本轮只查看页面，没有再次设置口令。普通无口令导出经实际格式解码，确认私密分类未导出，详情见“原生挂载来源直接播放与密钥轮换”。
- 原生设置测试口令时，系统钥匙串最终返回“无法保存同步口令：授权被用户取消。”，`has_passphrase=false`、`busy=false`。没有重试授权、输入密码、更换默认钥匙串或用测试 Vault 冒充系统保存；受保护 `.ebp` / WebDAV 原生往返因此**尚未完成**。两账号均未登录。
- WebDAV 地址、用户名和密码原先分开恢复，解密失败会留下新地址与旧密码的组合。现将整份配置密封，并在本机凭据写入成功之后才发布对应地址；无口令保留本机整份配置，空密码也不会继承接收端旧密码。此项回归与原生证据分别记录，不混为一谈。

本机证据：`target/tmdb-work/protected-source-passphrase-ui.png`、`protected-source-backup-ui.png`。自动化使用 MemoryVault/MemoryTransport；不等同系统钥匙串或生产 WebDAV 验证。

## iOS、Linux、Windows 独立运行

2026-11-10，三端独立应用构建并运行，数据与正式安装隔离。Linux 使用 Steam Deck 的 `erika-build` 容器，Windows 只控制本任务绝对路径的桌面 session 1 进程；截图以 PID 枚举窗口后 `PrintWindow`，不激活窗口、不截桌面。iOS 是唯一运行的模拟器，独立 bundle 重装保留来源及历史。

- 三端未登录新番、TMDB 简介、S01 与 S00 切换均有原生界面证据。Windows 初次截图为欢迎页，后续引导已完成的变化来源未确认，不能把引导完成记成本会话实际操作。
- iOS 通过真实点击及输入挂载 4322 假密钥来源；Linux、Windows 通过原生 UI 命令填写并提交表单。三端均点击未匹配、可读文件的播放按钮并显示实际视频，没有先制造规范绑定。
- 三端均定位 **12 秒**并暂停，截图为绿色段；关闭播放器、继续上次、暂停后，iOS 为 **12.024 秒**，Linux 为 **12.39 秒**，Windows 为 **12.0 秒**。
- Linux、Windows 来源设为 Movie 后，目录状态也是 Movie，不再固定为 Anime。iOS 此轮来源仍为 Anime。
- Linux 屏幕 DPMS 关闭时截图曾停留在旧黑帧；唤醒显示并重新定位后取得实际绿色帧。没有据此修改内核，也未把旧帧算为通过。iOS 首轮脚本误用 toggle 恢复了播放，后续改为幂等 pause 后重新验证，未将其算成产品修复。
- 三端通过公开 catalog 接口加载 TMDB Movie 701，规范 UUID 与 TV 701 不同；电影简介正确，季列表为空，只显示影片本身的条目，没有 SxxExx 编号。两个账号仍未登录。

证据在 `target/tmdb-work/`：`ios-native-paused-twelve-state.json`、`ios-native-resumed-twelve-state.json`、`linux-native-seek.png`、`linux-native-resumed-twelve-state.json`、`windows-native-twelve.png`、`windows-native-resumed-twelve-state.json`、`three-platform-movie-results.json`、`{ios,linux,windows}-movie-detail-state.json`。绿色画面和分季截图均已查看；电影断言是公开状态验证，不替代所有控件交互。

### 海报、背景和标题图

fixture 的合成作品采用官方 `image.tmdb.org` HTTPS 公开图片，仅验证渲染，不将样本当成该合成作品的真实 provider 关联。没有放宽生产 CDN 地址校验。

- iOS、Linux、Windows 均实际显示海报、背景和 Logo。
- 实际图片暴露共享详情背景布局问题：背景只声明 435 点高度，组件按源图比例推导约 773 点宽度，在手机撑宽页面，在桌面不能铺满区域。改为图片填充已有背景容器，不再由源图比例决定页面宽度。
- 手机资料栏去掉固定 224 点宽度；Logo 使用比例框和 72 点高度，按可用宽度适配。修复版 iOS 已截图核对三类图片与完整主按钮；Linux、Windows 桌面背景也已截图确认铺满。
- 首轮仅给 Logo 比例但未保留高度时，iOS 标题图没有显示；这轮不算通过，补回高度约束并重建后才获得最终截图。Linux、Windows 当前验证的桌面布局不走手机标题图分支。

已查看：`ios-artwork-fixed.png`、`linux-artwork-fixed.png`、`windows-artwork-fixed.png`。macOS 最新美术场景、Android 与更窄手机视口仍待验；这不是五端美术全部通过。

### 整窗渐变背景与亮暗可读性

随后按视觉反馈继续调整共享详情页：背景移到滚动正文之外，取消 435 点固定高度，以宿主视口填充；只有背景层延伸到系统安全区。顶端 30% 与底部 58% 采用主题底色渐变，中部统一遮罩由 84% 降至 32%，简介使用正文主色。没有改为 WebView，也没有隐藏海报、Logo 或主按钮。

- 独立 iOS 应用重建成功，最终版本的浅色、深色和实际滚动截图均已查看：背景延伸到状态栏与 home indicator，正文滚动时背景保持位置。不是只检查生成代码。
- Web 桌面 1280×800 浅色、深色均已查看；背景覆盖详情视口，不覆盖左侧导航和桌面工具栏。375×812 手机视口实际切到特别篇，页面显示 S00E01–S00E03；三类主美术均已加载，文档宽度 375，没有页面整体横向溢出。
- 手机 Web 浅色、深色截图已查看；该轮控制台无警告/错误，服务端无错误日志、网络无失败请求。检查结束恢复桌面视口及浅色。
- Linux、Windows 随后已重建此轮版本；Linux 深色及唤醒显示后的实际浅色、Windows 独立 session 1 窗口浅色截图均已查看。Linux 的旧 `linux-backdrop-gradient-light.png` 保留了休眠前的深色画面，不作为浅色证据，改用 `linux-backdrop-gradient-light-awake.png`；Windows 使用 `windows-backdrop-gradient-current.png`。两端此轮完整滚动、季切换及亮暗往返尚未完成。
- Android 的 SoundTouch bindgen 问题已通过完整 NDK target/sysroot/libclang 配置解除，随后实际启动发现欢迎弹窗跨 Compose 布局树换算坐标而崩溃。Bern 独立弹层作用域修复已本地提交，包含修复的新 CLI 与 Android APK 已编译成功，实际启动复验尚未执行，不能标记为已修复。macOS 最新渐变、更窄原生手机、横屏、折叠列与 TV 尚未覆盖。

最终截图已归档到 `target/tmdb-work/`：`ios-backdrop-gradient-{light,dark,scrolled}.png`、`web-backdrop-gradient-desktop-{light,dark}.jpg`、`web-backdrop-gradient-mobile-{light,dark}.jpg`。0.20 中间遮罩版不作为最终证据。

### 桌面整窗详情与分集左对齐

同日继续按反馈修改：桌面作品详情脱离侧栏 SplitView，媒体库列表仍保留侧栏；返回时恢复列表。不调用系统或浏览器全屏。共享分集条由 `snap: center` 改为 `snap: start`，去掉中心卡片放大，卡片保持 220 点等宽。

- Web 1280×800：详情侧栏数量为 0，背景为 `(0,56,1280,744)`，海报从 y=68 开始，返回按钮实际点击成功，媒体库侧栏与选中页恢复；浏览器未进入全屏，文档宽度 1280。
- 原背景 0×0 的根因是 Bern Web 没把无尺寸 cover 图片视为背景层。已修正布局意图分类；新回归修复前失败，修复后 runtime 的 19 项全部通过。固定尺寸图片和文字前景仍决定其容器尺寸，不靠应用硬编码视口尺寸。
- Web 375×667：正文实际滚动 191 点前后背景矩形保持，特别篇切换后出现 S00E01；返回媒体库成功，没有文档横向溢出。亮暗截图均已查看。
- 最新分集布局：桌面初始 S01E01 与向右滚动停靠后的 S01E04 均在 x=32，与分集视口左边重合；每张卡片实测 220 点宽。切到 375 点宽后保留 S01E04，停在分集视口 x=0；切特别篇后 S00E01 也从左端开始。返回媒体库成功，控制台无错误。
- 当时尾部仍受内容末端钳制，没有把这一项记为“末集也能独占左端”；2026-11-11 的根因修正与复验见下节“起点停靠的首末边界”。
- 这轮通过公开 `anime.open_from_library` 入口打开合成资料；Web 内存库没有文件来源，未把它记为实际媒体库海报点击或刷新后持久化验收。网络面板保留重启期间的连接拒绝记录，不声称全历史网络零失败。

证据：`web-fullwindow-detail-{dark,light}.jpg`、`web-fullwindow-mobile-{light,dark-scrolled}.jpg`；最新分集证据为 `web-episodes-leading-desktop.jpg`、`web-episodes-leading-mobile.jpg`，均在 `target/tmdb-work/` 且已查看。

### 桌面整窗覆盖核对（2026-11-11）

- “电脑端”包含 macOS、Windows 与 Linux，不是 macOS 专属。主仓与隔离副本的 `main_shell` 都在共用桌面布局中先挂载无侧栏详情，再在非详情状态挂载 SplitView；没有按 macOS 限制这项功能。
- 实际交付覆盖此前不足：Windows 远端验收源码没有 `desktop_toolbar` 与独立详情根分支，Linux 仅同步过滚动运行时和分集 UI。旧二进制的截图不能证明最新整窗要求完成。
- 已将同一份桌面共用根布局及配套框架同步到两端独立副本，Windows、Linux 原生构建均成功。Linux 首轮 `--locked` 因本机解析差异拒绝构建；保存原锁文件后通过本机缓存解析，再按既有入口完成构建，没有修改正式依赖 pin。
- 两端实际选择媒体库，经公开 `anime open-from-library` 入口打开 TV701：UI inspect 均无 sidebar，`desktop_toolbar.back` 存在。Windows 1920×1200、Linux 1280×750 的实际窗口截图均已查看，背景覆盖整个详情内容区，左侧不保留导航；没有触发系统全屏。
- 实际点击两端 `desktop_toolbar.back`，sidebar 恢复且选中项仍为 Library；返回截图均已查看。两端此时媒体库海报集合为空，因此该轮证明详情路由与返回布局，不冒充媒体库海报点击或完整业务验收。
- Windows 首张详情图有本机 fixture 连接失败提示；重建 SSH 4319 转发并再次打开后提示消失，最终使用 `windows-full-window-detail-connected.png`。Linux 也恢复了本任务转发。只控制独立 bundle / 绝对路径进程，两个账号未登录，未覆盖正式安装和数据。
- 证据：`{windows,linux}-full-window-{detail,return}-ui.json`、`windows-full-window-detail-connected.png`、`windows-full-window-return.png`、`linux-full-window-{detail,return}.png`、`windows-full-window-build.log`、`linux-full-window-build-resolved.log`，均在 `target/tmdb-work/`。

### Android 弹层实际复验与条件叠层

最新 UI 暴露第二个 Compose 生成问题：叠层里的动画条件分支被生成 Column，却仍使用 BoxScope 的 `matchParentSize`。生成器现为叠层分支建立自己的 Box，并保持普通横排分支为 Row；针对性生成回归 1 项通过。新 CLI、APK、应用 CLI 和 MCP 已全部构建成功，未修改生成文件绕过问题。

- `emulator-5554` 为当时唯一模拟器；安装使用独立 bundle 的 `install -r`，没有清数据。
- 实际冷启动进入欢迎页，点击 Start、滚动模块页、继续到外观与完成页，点击 Get started。进程 PID 3586 保持；该进程日志没有原跨布局树异常，未跳过欢迎弹层。
- 未登录状态确认后，通过原生入口打开 TV701，S01 及详情控件实际挂载，分集从左侧开始。此时图片没有加载，不能算美术通过：模拟器系统报告蜂窝连接，但 `ip route` 只有 dummy0、外网 ping 为 Network is unreachable，域名无法解析；宿主能够正常解析同一 CDN 域名。
- 禁用快照后冷启动仍无默认路由；模拟器 Wi-Fi 已启用但没有连接。连接它自带的开放虚拟接入点 `AndroidWifi` 后，获得 10.0.2.16，能够解析并访问官方 CDN，详情中的海报、背景、Logo、缩略图实际显示。没有改图片 CDN、关闭 TLS 校验或用本地图片冒充联网美术。
- 重启后 PID 3387：浅色、深色截图均已查看；实际横向滑动后 S01E03 停在分集视口左边，点击特别篇后显示 S00E01。提供方关联 Modal 实际打开、滚动目标列表并关闭成功；第三项先前只露上沿是 120 点高列表的正常滚动边界，滑动后可见 S01 和后续分集。空白区域对应尚无记录的关联列表，未误判为 Compose 裁切故障。没有保存关联或写账号。
- 从详情打开设置，再通过真实控件返回详情和媒体库；来源页的挂载弹层成功打开，并通过表单挂载本机 4322 假密钥来源。实际点击未匹配文件的“播放”，30 秒影片出画面；暂停并定位 **12 秒**，截图为绿色，标题为来源文件名。公开状态中的地址为 `erika-playback://`，最近记录保留不带凭据的稳定 HTTP 地址。
- 凭据轮换首轮操作顺序错误：应用先验证新密钥、fixture 随后才轮换，因此来源验证被拒绝；之后重新保存成功。失败播放退出时另外暴露真实历史缺陷：尚未加载的播放器用时长 0 覆盖旧最近记录，导致续播比例变成 0。新增回归在旧代码复现 `(12,30)` 被写成 `(12,0)`。
- 根因修复：历史写入以当前 transport 已报告 `loaded` 为前提，而不是仅以已发起打开为前提；切换/停止 transport 重置该标志。真实播放后的断线仍保存最后有效位置，总时长未知的真实媒体仍能记录。`erika-core --lib` 复验 **178 通过、0 失败、3 ignored**。
- 新 APK 保留数据重装后，以实际播放重新建立 **12/30 秒**基线；故意轮换 fixture 密钥但暂不更新应用，确认内核 Failed 后退出，最近记录的标题、进度、时长和缩略图完整一致。随后正常更新来源密钥，“继续上次”停在 **12.000 秒**；恢复实际解码后 **14.386 秒**截图为绿色。最初在首帧前立即暂停的黑帧不算画面通过。两代正常流鉴权成功，中间 3 次拒绝属于故意制造失败的路径。
- 新进程 PID 4708 的日志与公开 state、UI inspect 无测试密钥，无 fatal 或已知跨布局树异常。未覆盖所有内核日志路径。此轮未请求弹幕端点。
- Android 来源改为 Movie；Movie701 与 TV701 的 UUID 不同，电影季列表为空、条目无 SxxExx。通过公开 CAS 绑定接口把隔离文件关联到合成 Movie701 后，实际点击媒体库海报进入详情，电影标签、继续观看、海报、背景及 Logo 正常；并非真实作品识别结果。

已查看：`android-popup-layers-launch.png`（欢迎页）、`android-detail-wifi-light.png`、`android-detail-dark-leading.png`、`android-provider-popup.png`、`android-unmatched-seek-12.png`、`android-fixed-resumed-decoded.png`、`android-movie-poster-open.png`。`android-details-current-settled.png` 为联网前缺图详情；`android-details-current-initial.png` 是切页过渡空帧，均不作美术成功证据。历史不变断言保存在 `android-fixed-{baseline,after-failure}.json`，续播内核状态在 `android-fixed-resumed-{media,decoded-media}.json`。

## Bern 布局修复收尾

2026-11-10，独立弹层作用域、条件动画叠层、Web 无尺寸背景图片布局已通过 [AimesSoft/Bern#61](https://github.com/AimesSoft/Bern/pull/61) 合并到 `master`，合并提交 `024d9934d7ac8fc4ddbc4bd4e0f04fa7b9a81f92`。没有发版或修改版本号。

- 在不含手势覆盖层的干净提交树上完成格式检查、Compose/Web 两个 crate 的测试、19 项 Web 运行时回归、全工作区 clippy（warnings 为错误）和 check，全部通过。
- 第一轮扩展构建测试暴露符号链接路径比较误报；将预期路径按配置加载器同样解析后，完整复验通过，没有跳过失败测试。
- 以本机完整检查为合并依据；合并时应用 CI 面板尚未显示检查条目，不声称远端 CI 已通过。
- 手势覆盖层的 280 个文件逐文件核对哈希后保留，未混入提交；仅复用原 worktree 和共享构建目录。共享 Bern checkout 未同步或改动。
- 应用仍使用验收组合，正式依赖 pin 与五端最新版本验证尚未完成；此次框架合并不等于整个 TMDB 改造验收完成。

## macOS 组件工具栏与整窗详情复验

2026-11-11，最新原生构建发现共享 `desktop_toolbar` 被 SwiftUI 生成器当成普通 View；根 Toolbar 本应由容器附加，却在组件边界被丢弃。修复在 Bern 中把根 Toolbar 及其组件别名生成为 ViewModifier，并附着到所属真实容器；没有在应用里内联重复工具栏，也没有给 EmptyView 挂假工具栏。

- 新回归在旧生成器上失败，修复后通过，覆盖 macOS/iOS/tvOS 生成、值参数、会话、条件返回、palette/searchable 和子容器归属。
- 干净提交树的 fmt、SwiftUI crate 全部测试、workspace clippy（warnings 为错误）、workspace check 均通过。独立手势覆盖层 279 个文件逐一核对恢复，未混入提交。
- 修复通过 [AimesSoft/Bern#62](https://github.com/AimesSoft/Bern/pull/62) 合并到 `master`，合并提交 `bfe267619c1278fbe54c25ce5cd16ebcf00c4a9e`。依据本机完整检查合并；远端面板没有检查条目，不宣称远端 CI 已通过。没有发版，未改共享 checkout。
- 新 macOS 验收应用实际点击最近媒体打开无侧栏详情，返回按钮存在；实际返回后首页与侧栏恢复，搜索和打开入口保留。
- 原生搜索输入“旅途”，实际点击 TV 结果打开整窗详情；有图深色、浅色截图均已查看，背景铺满、上下过渡及正文可读。实际切特别篇后显示 S00E01–S00E03。
- 通过原生 `desktop_toolbar.open_file` 文件控件载入 30 秒本地测试片，截图为红色网格画面；暂停状态再次读取为 `paused=true, video=true, elapsed=1.278, duration=30`，之后关闭播放器。
- 第一季第一卡位于左侧；特别篇短列表的首卡仍多一层左侧内边距，尚未核准精确对齐；末集靠左边界也未解决，不能把全部定位情况标为通过。

已查看：`macos-toolbar-detail-main.png`、`macos-toolbar-search-settled.png`、`macos-toolbar-tv.png`、`macos-toolbar-tv-light.png`、`macos-toolbar-specials.png`、`macos-toolbar-open-playback.png`，位于 `target/tmdb-work/`。`macos-toolbar-detail.png` 误选了菜单栏，搜索初始图是过渡态，不作为最终证据。iOS/tvOS 本轮只验证生成结果，未运行设备；正式依赖 pin 仍未更新。

### 起点停靠的首末边界

2026-11-11，定位到两层原因：桌面正文已有 32 点内边距，分集行再次添加 32 点；各原生/Web 后端仅为居中停靠预留首尾空间，起点停靠的末项受到最大滚动范围限制。应用删除桌面分集行重复内边距；Bern 五后端按末项实际尺寸保留尾部空间，普通位置绑定不增加留白。

- 新增 Web 横向/纵向末端回归在旧实现均失败，修后通过；覆盖末项不等于最大项、首卡无额外前缘空间、缩放、末项超宽、移除末项及清空列表。原有居中与普通位置回归仍通过。
- Swift 6 编译并执行实际运行时类型的几何回归，覆盖首末位置、末项尺寸、滚动和逐项删除；新版 macOS 实际应用构建成功。
- macOS 1280×800 原生窗口：首集 S01E01、第 24 集、特别篇 S00E03 实际停靠截图均已查看，卡片与海报、正文共用左边线，没有将代码生成当作运行证据。`macos-start-snap-special-first.png` 实际是切季保留末端位置后的 S00E03，文件名不能作为首集证据；首集操作后另存 `macos-start-snap-special-leading.png`。
- Web 首轮真实检查进一步发现竖向滚动容器中的柔性叠层会按短内容收缩，去掉测量留白时正文从 1280 缩至 920 像素。已在框架修正该容器的柔性内容宽度，不在应用硬编码视口。
- Web 新构建 1280×900：特别篇首卡 x=32，视口宽 1216，移除测量留白前后宽度不变；尾部留白 996。点击末端按钮后 S00E03 x=32，scrollLeft=max=500。375×812 窄屏末集到 x=0，视口宽 375、尾部留白 155；截图已查看，控制台无错误。测试后恢复默认视口并停掉验收 Web 服务，避免干净树检查期间热重建。
- Bern 独立提交 `4c32a97` 只含停靠修正及回归，不含独立手势覆盖层。干净树 fmt、75 项 Web 回归、SwiftUI/Web/Compose/WinUI3 四个生成后端全部测试、workspace clippy（warnings 为错误）及 check 均通过。首轮误选独立 workspace 的 `bern-gtk`，纠正选择器后完成剩余检查；两轮均逐文件核对并恢复 280 个文件。
- [AimesSoft/Bern#64](https://github.com/AimesSoft/Bern/pull/64) 已合并到 `master`，合并提交 `2d9a21bc351238e873071d3bef6e90c264917cda`；CI 面板为 3 通过、0 失败、0 等待。Linux 已实际编译更新后的 GTK 运行时；数据转发断开导致搜索失败，恢复原 SSH 测试转发后正常。诊断实测视口 936、末卡 220、尾部留白 716；移除诊断后重新构建并实际检查 S01E24、S00E01、S00E03，均与正文左缘对齐。最初截图为滚动动画中间帧，不作为失败或最终停靠证据。已删除本次远端/本地分支及两份已恢复的验证 stash；覆盖层 280 个文件哈希不变，旧 stash 与共享 checkout 保留。
- Linux 当前仅同步 GTK 运行时与分集 UI，根 shell 仍为此前版本，不能作为最新整窗无侧栏布局验收。iOS、Windows 尚未重建本轮末端停靠版本；正式依赖 pin 尚未更新。
- Android 本轮 APK 保留数据安装，两个账号未登录。模拟器先被旧崩溃报告询问阻塞，已选择“不发送”；软件图形冷启动后出现 System UI ANR，记录保留，连接原 AndroidWifi 后恢复。实际详情显示三类美术，但特别篇末项曾左侧裁切，原截图不算通过。
- 继续诊断发现两个问题：换季卸载后旧项目的测量记录仍留在停靠候选中；此外边缘回弹可消耗全部 `ScrollScope.scrollBy` 位移而不改变实际内容偏移。逐帧日志中目标为 1548，最终实际位置仍为 1596；仅清理旧项目未消除裁切，没有将该中间版记为修好。
- 框架修复让卸载项目移除测量记录并重算最大尺寸；停靠在既有 fling mutation 内更新本列表实际偏移，手势边缘回弹仍保留。JVM 回归使用模板中实际状态与 fling 实现，布局/状态/动画使用明确的替身，覆盖双轴替换、删除、缩小、空列表与边缘消耗；边缘用例修前复现 `532 != 516`，修后通过，不冒充真实 Compose 生命周期测试。
- 无诊断日志的 Android 修复版构建、保留数据安装后，实际从 S01 切到 S00，两次左滑至 S00E03，再两次右滑回 S00E01；两张稳定截图均已查看，图片与文字完整停在视口左端，无此前裁切。修复提交为 Bern `9c24194`，干净树 fmt、Compose 全部测试、workspace clippy（warnings 为错误）及 check 均通过；覆盖层 280 个文件哈希核对恢复。
- 已通过 [AimesSoft/Bern#65](https://github.com/AimesSoft/Bern/pull/65) 合并到 `master`，合并提交 `921ce3ba6bcbe375a18e43227d2cdba0f7b8b0d3`。以本机完整检查及 Android 实测为依据；合并前 CI 面板为 3 项等待，不声称当时远端已通过。已删除本次分支和已恢复验证 stash，旧 stash 与共享 checkout 保留，未发版。证据：`check-compose-scroll-9c24194.json`、`android-scroll-fling-every-step.log`、`android-scroll-local-snap-{special-last,special-first}.png`、`android-scroll-local-snap-clean-build.log`。

证据位于 `target/tmdb-work/`：`macos-start-snap-{first,last,special-first,special-leading}.png`、`start-snap-edits/web-before-result.txt`、`bern-start-snap-4c32a97-checks.json`、`bern-start-snap-4c32a97-checks-remaining.json`。检查脚本逐文件核对并恢复独立覆盖层；首次记录确认 280 个文件恢复。

## 尚未验完

- 受保护账号/来源凭据完整往返仍被系统钥匙串取消边界阻塞。无口令 `.ebp` / WebDAV 的 SubjectSync/WatchSync 排除、空库经 WebDAV 恢复来源归属、评分 0/空短评、文件进度，以及恢复后插件重扫与最近播放已按上节各自范围验证；这些结果不代替受保护凭据验证。
- 清除评分的真实原生成功路径；目前通过公开命令的 mock 账号回归，未写真实账号。
- 正式依赖组合、生产 HTTPS/CORS、美术资源及带请求头来源的片头分析；本机原生主播放成功/失败路径已验，范围见上节。
- Windows、Linux、Android、iOS 的新架构完整实际运行验收；macOS 也只完成本文列出的场景。
- 独立 web_app 的网站账号适配、持久存储和其他消费入口尚需收口；wasm 编译不能替代这些能力验证。

不向旧同步仓库发布当前格式数据，不触碰生产服务或真实账号。2026-11-11 用户明确要求推送并发版后，按新的发行授权推进；未完成项目仍保留，不据此宣称全部验收通过。

## 0.1.1 发行组合（2026-11-11）

- Bern 手势覆盖层基于最新 `master` 无冲突应用，核对应用所用 `Swipe` 连续更新、结束提交、活动状态及双击接口后，通过 [AimesSoft/Bern#66](https://github.com/AimesSoft/Bern/pull/66) 合并为 `47820993b1f519b5e4e754a1597867da39c2ee54`。`v0.1.15` 标签已推送，[发行流水线](https://github.com/AimesSoft/Bern/actions/runs/42975364791)已全部成功，GitHub Release 的六个平台 CLI 制品已上传。
- 本机格式、相关编译器及八个生成后端测试、workspace clippy/check 通过；Compose 实际 Kotlin 适配器编译执行通过，Web 运行时 89 项通过。独立 GTK 运行时远端 Clippy 首轮发现回调类型复杂度告警，提取类型别名后 GTK CI 通过；合并时 JavaScript 检查也通过，workspace 远端检查仍在运行。
- 原手势快照保留于 stash，未改共享 Bern checkout。手势代码已进入正式源码，不再依赖临时覆盖层才能构建。
- Erika 0.1.1 的应用清单、开发测试依赖、发行构建工具统一固定到上述 Bern 正式提交；liberika 仍固定 `v0.3.6` 的 `e38a511561cb30ca57bd370b944d2e99b12737c7`。
- 在不读取项目本机 `.cargo/config.toml` 的源码副本生成正式锁文件，保留远程 Git 来源；本机预览配置、隔离应用和测试 HOME 不进入提交。正式依赖组合的检查如下，不将之前验收截图重新标成发行组合实测。
- 正式 Git 依赖、`--locked` 的全工作区 `cargo check --workspace --all-targets` 通过。首次缺少本机 zlib/dav1d 原生依赖路径，补齐检查环境后发现三个联网示例仍使用旧整数身份或旧封面路径；已改为日历不透明键、规范媒体 UUID 和 TMDB 美术契约，没有恢复旧编码。这三个示例只编译，未执行联网请求。
- 同一正式组合八个 crate 联合回归：anime 70、catalog 28、core 178、dandanplay 30、library 33、remote 9、store 184、sync 40，共 **572 通过、0 失败、5 ignored**。ignored 为已有真实网络用例；仍有 anime 的 dead_code 编译告警。日志为 `target/tmdb-work/erika-0.1.1-public-check-examples-aligned.log`。这不替代正式组合的五端实际运行验收。
