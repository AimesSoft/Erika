# TMDB 主媒体库：设计与实现方案

> 2026-11-08 起草 · 2026-11-10 修订 · 实施中；本地代码与验收尚未全部完成。
>
> 依据：开工时 `device-kernels` 工作区代码及下列官方接口资料。实施分支为 `feat/tmdb-library`；工作区原有修改保留。2026-11-09 用户明确取消所有旧数据迁移：只初始化当前格式，不提供 Legacy 节点、身份归并、旧版导入或升级备份。没有操作用户现有数据库、生产服务或真实账号写入；五端实际验收未完成，尚未提交或推送。

## 1. 结论与范围

**让 TMDB 决定媒体库的作品身份与季集结构；文件识别、弹幕匹配、评分评论各自保留独立关联。**

这不是把某个 fallback 改成 primary：开工前应用没有独立 TMDB 客户端，TMDB 主要在 Misaka 内部参与后备识别；应用的数据模型和调用协议仍以弹弹play 编号为中心。只替换海报与标题，不能解决特别篇、跨季选集、电影和 Jellyfin 对接。

### 1.1 已确认的职责划分

| 能力 | 数据源与职责 |
| --- | --- |
| 媒体库中心、作品身份、季集编排 | **TMDB** |
| 自动刮削/识别顺序 | **弹弹play识别 → 失败或无法确定时，正则解析文件名并查询TMDB → 人工确认** |
| 播放时的内置在线弹幕 | **弹弹play**；独立保存弹幕库关联，不以它决定媒体库展示 |
| 评分与评论 | **Bangumi**；不再以弹弹play综合评分/短评或TMDB评分作为替代来源 |
| 观看记录 | **弹弹play / Bangumi**，按用户启用的账号同步；本地保留记录与文件级续播进度 |
| 标题、海报、简介、大幅背景海报、标题图片（Logo） | **TMDB**；缺失时按第8节降级，不改变作品身份 |

这里的“正则匹配”是从文件名/目录提取作品名、年份、季、集等线索，再匹配TMDB条目；不是用正则生成作品或剧集ID。弹弹play仍参与第一步识别、播放弹幕和观看记录，但不再是媒体库的结构中心或默认展示资料来源。

以下约束随上述分工固定：

- 本地媒体、远程媒体统一进入TMDB主媒体库；动画不是例外，但动画属性与新番日历仍保留。
- 已确认的文件绑定与Jellyfin等上游提供的明确TMDB身份直接复用/验证，不重复刮削；只有尚未确定身份的文件进入“弹弹play → 正则”顺序。
- 弹弹play识别不可用（包括当前鉴权不满足）时直接进入正则/TMDB阶段，不阻塞整个媒体库；正常播放不等待账号同步。
- TMDB未收录、离线或映射不确定时，保留可播放的本地条目和现有资料，不凭猜测归入错误作品。
- 弹弹play/Misaka编号保留为外部服务引用，不再充当媒体库主键。Misaka只保留独立弹幕能力，不插入新的默认刮削顺序。
- Bangumi关联可以落在整部作品、某一季、某一段剧集或单个特别篇，不强制一对一；没有对应条目时不把其他站评分冒充Bangumi评分。
- 当前格式内，文件、地址、进度与人工决定相互独立；更新元数据不重建文件。旧版数据库不迁移、不自动删除。

本次设计包含媒体库、播放器识别/选集、通用详情、账号关联、同步、共享、插件宿主及 CLI 契约；**不包含 Jellyfin 插件本体的开发**，但定义其需要的宿主接口。不改 liberika，不移动或重命名媒体文件，不把新番日历强行改成 TMDB 日历，不引入内置 AI 刮削服务。

## 2. 开工前基线（不是当前实施状态）

### 2.1 代码与影响面

| 位置 | 当前事实 | 对这次改造的影响 |
| --- | --- | --- |
| [erika-store/src/lib.rs](../erika-store/src/lib.rs) `Anime`、`Episode` | 两种 DTO 都以 `dandanplay_id` 对外；`Episode` 只有整数 `number` | 没有正式的电影、季、特别篇身份 |
| [erika-store/src/sqlite.rs](../erika-store/src/sqlite.rs) 建表代码 | `anime`/`episode` 有本地整数主键，但查写主要走 `dandanplay_id`；没有 TMDB 字段或季表 | 不能直接把 TMDB 数字塞入旧列；须扩展领域模型 |
| 同文件 `file`、`progress`、`address` | 文件指纹是前 16 MiB MD5 加大小；进度按文件，地址按来源与路径 | 这一层保留；不能因为元数据换源丢进度 |
| 同文件 `shelf`、`watched` | 墙上的已看数量由文件进度 ≥90% 推导；另有显式 `watched` 表 | 新投影要统一已看语义 |
| [modules/library/core/src/scan.rs](../modules/library/core/src/scan.rs) `match_videos` | 按来源类型二选一：弹弹play 或自建识别服务；并发数为 4 | 替换识别编排层，保留遍历、指纹增量扫描 |
| [modules/library/core/src/lib.rs](../modules/library/core/src/lib.rs) `start_matching` | 动画自动匹配依赖弹弹play token；已匹配/人工处理的文件跳过 | TMDB 刮削与登录解耦；人工锁定语义必须继承 |
| [erika-core/src/matching.rs](../erika-core/src/matching.rs) `recognize`、`show_episode` | 播放时先弹弹play、再自建服务；媒体身份、标题与取弹幕共用同一组 ID | 拆开媒体绑定与弹幕绑定；沿用异步 revision 防串片 |
| [erika-dandanplay/src/lib.rs](../erika-dandanplay/src/lib.rs) `Matched::number` | 以 `abs(episode_id) % 10000` 推断集数 | 新链路必须停止 ID 算术；外部服务适配不得生成 TMDB 编号 |
| [modules/library/core/src/names.rs](../modules/library/core/src/names.rs) | `S01E07` 只返回 `7`，季号丢失 | 需要结构化文件名线索，不是换一个正则就完成 |
| [modules/library/core/src/matching.rs](../modules/library/core/src/matching.rs) `Chosen::episode`、`Session::details` | 非整数集号会被过滤；找不到真实集时可合成 `anime_id * 10000 + number` | 新确认流程只能引用真实节点；未知项保持未解析 |
| [erika-dandanplay/src/bangumi.rs](../erika-dandanplay/src/bangumi.rs) | Bangumi ID 编码为 `10^12 + id`；只请求 `type=0`，并过滤小数及非本篇 | 来源适配须区分命名空间；特别篇不能继续被过滤 |
| [modules/anime/core/src/catalog.rs](../modules/anime/core/src/catalog.rs) | 已有弹弹play/Bangumi/公开缓存路由；只凭明确 `bangumiUrl` 建关联 | 可复用已验证关联，不能退回仅按名称合并 |
| [modules/anime/core/src/shows.rs](../modules/anime/core/src/shows.rs)、[erika-core/src/queue.rs](../erika-core/src/queue.rs) | 详情胶片、继续观看和播放队列依赖旧作品 ID 与整数集数 | 数据库改完后这些消费者必须一起切换 |
| [erika-dandanplay/src/custom.rs](../erika-dandanplay/src/custom.rs)、[erika-gateway/src/lib.rs](../erika-gateway/src/lib.rs) | Misaka ID 在客户端取负；网关调整临时 ID、保存指纹确认、转发兼容接口 | 负数不是 TMDB ID；旧路由保持兼容，新服务分命名空间 |
| [erika-store/src/portable.rs](../erika-store/src/portable.rs)、[modules/sync/core/src/local.rs](../modules/sync/core/src/local.rs) | 作品、剧集、绑定、已看与评分的跨设备身份依赖弹弹play ID | 同步格式是必改项，不能只改本地 SQLite |
| [modules/remote/core/src/share.rs](../modules/remote/core/src/share.rs) | NipaPlay 兼容共享字段 `animeId`、`episodeId` 使用旧 ID；部分列表按正数筛选 | 正数也可能是 Bangumi 编码，不能继续以符号判断供应商 |
| [erika-core/src/plugins/host.js](../erika-core/src/plugins/host.js)、[erika-core/src/plugins/mod.rs](../erika-core/src/plugins/mod.rs) | `library.getShows()` 返回旧 ID；写接口主要是 `setEpisodeWatched` | 尚无导入 Jellyfin 媒体源/元数据的通用宿主接口 |
| [erika-core/src/plugins/links.rs](../erika-core/src/plugins/links.rs) `play_resolved` | 插件播放结果读取 URL、标题、请求头和 `searchTitle`，不接收结构化媒体身份 | 仅补一个 Jellyfin URL 解析器仍会丢失上游刮削结果 |

历史文档中“弹弹play 只认番剧”“状态评分只在本次运行”等描述不能直接当作当前能力清单；代码和供应商接口已经演进。

### 2.2 当前链路

```text
本地动画来源 → 弹弹play match ─┐
电视剧/电影 → 网关 → Misaka ──┼→ Anime + Episode → file.episode_id
播放时识别 → 弹弹play/网关 ───┘        │
                                      ├→ 媒体库墙 / 详情 / 选集
                                      ├→ 观看状态 / 同步 / 插件 / 共享
                                      └→ 根据 episode ID 选择弹幕服务
```

根因是：**把某一家服务的作品与弹幕库编号，当成了所有媒体的统一身份，并把季集结构压成一个整数。**

## 3. 已核实的接口能力与边界

下述“官方支持”指查询当日的文档/上游源码，不等于已在 Erika 生产网关实测可用。没有向真实账号发送评论、评分或匹配确认。

### 3.1 TMDB：负责标准编目，不负责视频指纹识别

| 接口（v3） | 用途 |
| --- | --- |
| `GET /search/tv?query=…` | 电视剧候选；支持 `first_air_date_year`、`year`、`language`、`include_adult`、分页 |
| `GET /search/movie?query=…` | 电影候选；支持年份筛选 |
| `GET /tv/{series_id}` | 剧集资料及实际 `seasons[]`；用数组读取季，不能仅按 `number_of_seasons` 循环 |
| `GET /tv/{series_id}/season/{season_number}` | 该季实际分集；返回 `id`、`show_id`、`season_number`、`episode_number`、标题和播出日期等 |
| `GET /movie/{movie_id}` | 电影资料；电影不伪装成第 1 季第 1 集 |
| `GET /tv/{series_id}/images`、`GET /movie/{movie_id}/images` | 返回 `posters[]`、`backdrops[]`、`logos[]`；标题图片从 `logos` 取，不把公司/电视网标志当作品标题 |
| `GET /tv/{series_id}/season/{season_number}/images` | 季海报集合；该接口不提供季专属背景图/Logo，缺项使用所属TV的资源 |
| `GET /find/{external_id}?external_source=…` | 按 IMDb、TVDB 等受支持标识查找；未列出 Bangumi 或弹弹play |
| `GET /configuration` | 图片服务地址与可用尺寸；图片地址由配置、尺寸和路径拼成 |
| `GET /tv/{series_id}/episode_groups` | 其他编排方式的分组目录；与默认季集结构不是同一对象 |

关键约束：

1. `movie:123` 与 `tv:123` 是不同作品。TMDB 数字 ID 必须带实体类型。
2. 季记录 ID、季号、分集 ID、集号是不同字段，禁止互相代用。
3. `season_number = 0` 是合法特别篇季，不是“未匹配”。空值使用 `Option`/`null`，不再以 `0` 哨兵代替。
4. OVA 不保证都是 `S00`：以实际返回的电影/剧集/季集记录为准。供应商没收录时保留独立条目，不自行归并。
5. 搜索只生成候选，不能证明文件就是某一集；TMDB 没有在上述接口中提供视频 MD5 识别。
6. 新媒体库采用 TMDB 默认季集编排。若 Jellyfin 使用 DVD/绝对集数等其他编排，必须利用分集身份或明确映射转换，不能直接套用它的 `SxxExx`。

鉴权使用服务端 API Read Access Token（Bearer）或 v3 API key；无需用户登录 TMDB。上线需按 TMDB 要求在关于/致谢展示标识及声明，并核查项目用途对应的许可。图片和元数据同样受使用条款约束。

### 3.2 弹弹play：已有值得直接复用的跨库能力

官方 Swagger 已明确提供：

| 接口/字段 | 确认结果 | Erika 当前情况 |
| --- | --- | --- |
| `POST /api/v2/match` | 请求含文件名、前 16 MiB MD5、大小，可选时长；结果含真实 `episodeId`、`animeId`、`shift` | 已用，但未保存 `shift`，会用 ID 算集号 |
| `GET /api/v2/bangumi/{bangumiId}` | 参数是字符串，可用数字作品 ID，也给出了 `tmdb-movie-21832` 示例 | 当前封装参数是 `i64` |
| 详情 `bangumiId` | 新的作品标识，字符串 | 当前 `Details` 未读取；不能认为它是 bgm.tv subject ID |
| 详情 `seasons[]`、分集 `seasonId` | 季 ID 为字符串；部分来源有季列表 | 当前模型未读取；**不能未经查证将 `seasonId` 转成 TMDB 季号** |
| 分集 `episodeNumber` | 字符串，可能带字母；`episodeId` 是弹幕库 ID | 当前多处只接受整数 |
| 详情 `onlineDatabases[]` | 外部数据库名称及 URL | 当前未读取；这是正向关联 TMDB 的优先证据之一 |
| `GET /api/v2/search/episodes?tmdbId=…&tmdbIdType=…` | `tmdbIdType=0` 是 TV、`1` 是电影；可能返回多个关联作品 | 当前尚未封装，可用于 TMDB → 弹幕候选 |
| 上述接口 `episode` | 接受正整数或 `C1/S1/O1`；没有独立的 TMDB `season_number` 参数 | 不能拿 `S00E01` 或季内整数直接保证选对弹幕 |
| 上述接口 `hasMore` | 表示结果未完整返回 | 不得据此认定候选唯一 |
| `GET /api/v2/bangumi/bgmtv/{bgmtvSubjectId}` | 从 Bangumi subject 找弹弹play 详情；官方说明映射人工维护、可能缺失或错误 | 可补映射，但不能当作不可质疑的事实 |
| `GET /api/v2/comment/{episodeId}?withRelated=true` | 读取弹幕，包括关联来源 | 继续使用真实供应商 ID |

尤其要区分：官方部分元数据接口无需用户登录，不代表 Erika 当前 NipaPlay 网关也允许匿名透传。现有 `search()`/`match_video()` 在客户端要求 token；新增公开元数据调用必须先验证网关权限与应用额度，不能简单删登录检查。

官方发送应用弹幕现提供 `POST /api/v2/comment/{episodeId}/app`，而当前客户端使用没有 `/app` 的 POST 路径。**本方案不假设旧网关一定失效，也不顺手替换发送接口**；实施时确认网关适配和应用授权，元数据改造不得扩大“能发送”的能力声明。

### 3.3 Bangumi：条目关联与分集关联分开

- `GET /v0/subjects/{subject_id}` 获取条目。
- `GET /v0/episodes?subject_id=…&limit=…&offset=…` 分页获取章节；`type` 是可选筛选，不传才可收集不同类型。
- 章节 `type` 区分本篇、特别篇、OP、ED、预告等；`sort` 是数值，不限整数；`ep` 对非本篇不具备通常集号的意义。
- `POST /v0/search/subjects` 可用于候选搜索，官方标为实验性；不应成为无回退的强依赖。
- 现有短评实现调用 `https://next.bgm.tv/p1/subjects/{id}/comments`，不是上述公开 v0 schema 中的同一接口。保留独立适配、缓存与错误处理，不将它包装成有稳定保证的 v0 接口。
- 现有账号登录/写入适配继续使用自己的凭据；网站 Cookie 不传给 TMDB、Misaka 或公共元数据网关。

一部 TMDB TV 可能对应多个 Bangumi 条目，单季也可能包含分割放送的多个 subject。默认在所选季/分集范围显示其对应评分和评论；无法选定唯一范围时让用户选择条目，不把多个 subject 的评分平均成一个“Bangumi 评分”。

### 3.4 Jellyfin：直接继承上游资料，但先分清 ID 层级

官方 `BaseItemDto` 有 `Id`、`ServerId`、`Type`、`ProviderIds`、`SeriesId`、`SeasonId`、`ParentIndexNumber`、`IndexNumber`、`IndexNumberEnd`、`MediaSources` 等字段。`GET /Items` 支持字段选择与分页等查询；具体服务器版本也可能使用兼容的用户路径，插件应做能力探测。

- `Type=Series` 的 `ProviderIds.Tmdb` 才是该剧的 TMDB TV ID。
- `Type=Episode` 的 `ProviderIds.Tmdb` 若有，是分集层级的标识，**不能当成 TV ID**。通过 `SeriesId` 取父剧，通过季/集字段及分集元数据验证位置。
- 季对象也要区分“季 ID”和“季号”；缺字段时不能把 `null` 变成特别篇季 `0`。
- `Type=Movie` 走电影身份，不生成假季与假集。
- 多集文件保留 `IndexNumberEnd`；多个 `MediaSources` 保留各播放版本，不能合并为一个不可选择的 URL。
- 服务器若未给出 TMDB ID，已有标题、海报、剧集关系仍然有用；先作为来源条目导入，再补解析，不先扔掉上游资料去猜文件名。
- 官方命名说明也强调：特别篇编号因元数据供应商而异，不应以 `Season 00` 文件夹本身证明与 TMDB 一致。

### 3.5 Misaka 与现有网关

上游 Misaka 的 TMDB 适配确有 `tmdbApiKey`、`tmdbApiBaseUrl`、`tmdbImageBaseUrl` 配置；兼容详情 schema 有外部链接、季与字符串集号。但本轮查阅的 `/search/episodes` 路由要求 `anime`，不能因为官方弹弹play 支持 `tmdbId` 就推断 Misaka 同样支持。

现有网关还会把 Misaka 详情的 `bangumiId` 改成 `A{lasting_id}`。因此：

- 不能把它返回的作品数字/负数当 TMDB 标识。
- 不能只依赖 `bangumiId` 恢复 TMDB 关联；优先检查外部链接等明确字段，否则作为名称候选。
- 上游源码版本不等于生产容器版本。本轮没有读取线上配置、测试真实 TMDB 凭据或核对部署版本；这些属于上线前接口冒烟的明确项目。

## 4. 目标分层

```text
扫描器 / 打开文件 / 远程源插件
             │ 文件、来源条目、上游身份、文件名线索
             ▼
       erika-catalog（新增）
       ├─ TMDB 规范编目、图文资料与图片资源（通过 Erika 网关）
       ├─ 弹弹play识别 → 正则线索 + TMDB查询 → 候选确认
       ├─ Bangumi范围关联、评分与评论
       ├─ 弹弹play弹幕关联、人工锁定
       └─ 按职责生成显示资料；保留外部来源适配
             │
             ▼
       erika-store（扩展）
       ├─ 规范媒体节点与外部映射
       ├─ 文件→媒体绑定、人工决定
       ├─ 文件→弹幕绑定
       └─ 观看状态、来源身份、操作记录
             │
             ├─ 媒体库 / 通用详情 / 全局搜索
             ├─ 播放队列 / 继续观看
             └─ CLI / 插件 / 共享 / 同步

新番日历 → 原有弹弹play / Bangumi / 公开缓存
                       └→ 使用同一关联服务接入媒体库，不另造一套映射
```

职责：

- `erika-store` 保存事实和用户决定，不做网络请求。
- `erika-catalog` 是可被外壳与模块共用的 Rust crate，不能依赖具体 UI；含领域类型、供应商适配、候选策略与投影。不是每个模块复制一套 resolver。
- `erika-dandanplay` 继续承载弹弹play识别/弹幕/观看记录和Bangumi评分/评论/观看记录适配，首先补齐无损DTO；旧来源适配保留但不决定新的职责路由。无需同时进行全仓库crate改名。
- `erika-gateway` 新增独立元数据路由，直接请求 TMDB；不能让 TMDB 浏览依赖 Misaka 的视频站弹幕搜索。
- `modules/library` 负责本地扫描与导入编排；播放中识别复用同一 resolver。
- 当前共用详情页面可继续由 `modules/anime` 承载，但它接收中立 `MediaId`，不再把所有媒体解释成 `Details`/动画。新番入口另外带上原始 provider context，避免从日历打开后误同步到整部 TV。

新网络代码须以传输接口隔离原生线程 HTTP 与浏览器异步 HTTP。WebUI 在宿主执行；`web_app` 在浏览器执行、受 CORS 与存储能力约束。当前 `custom`/Bangumi 的 wasm 分支返回不可用，不能把它们原样复用后声称全端完成。纯 Web 原有不能扫描本地目录的限制单独列出，不伪装成刮削失败。

## 5. 身份与存储设计

### 5.1 “TMDB 主键”的准确含义

TMDB 是**规范编目键**，不是直接替换 SQLite 每张表的主键。

```rust
// 设计类型，不是当前已有接口。
struct MediaId(String); // 稳定、不暴露供应商含义的 UUID；协议中按字符串传递

struct ProviderRef {
    provider: Provider,  // Tmdb / Dandanplay / Bangumi / Misaka / Jellyfin
    scope: String,       // 自建服务/服务器实例的稳定标识；全球服务为空
    kind: EntityKind,    // Tv / Movie / Season / Episode / Subject / Item
    id: String,          // 不做正负号或数值区间编码
}
```

规范引用示例：`tmdb:tv:1399`、`tmdb:movie:1399`、`tmdb:episode:63056`。季号/集号是位置字段，不是身份；TMDB 调整编排时，已有分集 ID 不变就更新位置，而不是创建新集并丢掉已看状态。

本地使用稳定 `MediaId`；未识别文件只保留文件身份，不建立占位媒体节点。跨设备不能发送 SQLite rowid；当前格式快照按精确 typed TMDB 引用解析到接收端已有 UUID，并在本次导入中重写关联，不保留旧身份别名。

### 5.2 媒体层级

```text
TV 作品 → Season（含 season_number = 0）→ Episode
Movie 作品（自身可播放，不挂在 Season 下）
未解析文件 → 保留地址、文件名和识别证据（不创建媒体节点）
```

动画是内容分类或标签，不是与 TV/Movie 互斥的身份类别。一部动画电影归 Movie；来源里的“番剧/电视剧/电影”只作为搜索与筛选线索，不决定调用哪家识别服务。

推荐在新表使用统一 `media_node` 保存作品、季和分集，使绑定、已看与映射都有真实外键；Rust 以枚举封装，避免任意父子组合。TV→Season→Episode、Movie 无季父节点、分集与季同属一部 TV，由写事务及必要的 SQL 约束/触发器共同保证。

### 5.3 新增表与不变量

以下是逻辑表设计；字段命名可在实施时对齐现有风格，但约束不能省略。

| 表 | 核心字段与约束 |
| --- | --- |
| `media_node` | `id`、`kind`、`parent_id`、`season_number?`、`episode_number?`、文字标题/简介及来源、`revision`；相同 TV 下季号唯一，相同季下集号唯一；`NULL` 与 `0` 区分 |
| `media_artwork` | 节点、实际资源owner、`poster/backdrop/logo/still`、路径、语言、尺寸/比例、选中/人工覆盖、更新时间；图片与文字分列，图片字节留在有上限的缓存而非每次同步复制 |
| `provider_link` | 本地节点、`ProviderRef`、关系、覆盖范围、证据、状态、修改时间；TMDB 已确认 `exact` 引用唯一；外部编号按字符串保存 |
| `file_binding` | 每个文件一条决定：`auto/manual/blocked`、版本号、证据来源；`blocked` 是用户解除且禁止自动绑定 |
| `file_binding_item` | 一文件可对应多个可播放节点，含顺序、可选起止时间；引用真实 Movie/Episode |
| `danmaku_binding` | 文件、供应商/实例、真实弹幕库 ID、绑定依据、可选媒体关联、供应商偏移、人工偏移/选择；不能复用 TMDB ID |
| `media_watched` / `media_mark` | 节点的已看/显式未看、时间、人工标志；作品/季/条目状态与用户评分，保留提供方及作用范围；新评分对应Bangumi |
| `account_watch_sync` | 本地变更、提供方/账号、目标外部ID、范围、映射revision、远端基线、操作标识、待发/确认/失败状态；弹弹play与Bangumi分别记账，不保存明文凭据 |
| `account_sync` | 每账号的收藏/评分/短评发送队列与共享条目锁；保存完整本地决定、映射快照、执行token及租约，不进入导出快照 |
| `catalog_job` | 文件/节点、任务阶段、输入 revision、状态、下次执行时间、错误类别；支持断点续跑和重新解析 |
| `catalog_change` | 一次确认/批量操作的前后绑定及事务 ID，用于审计和撤销 |

`file`、`address`、`progress`、`recent`、`source` 保留原有身份和数据。元数据刷新不重建文件、不改 URL、不清零进度。

具体约束：

- 一个 Movie 是一个可播放节点，多版本电影对应不同文件；同一集多个压制版也分别保留文件进度。
- 多集文件用多个 `file_binding_item`，没有可靠章节/时间点时不编造切分时间：选任一集都明确指向同一个合辑文件，队列只播放该文件一次。仅末尾完成播放可据策略标记覆盖的全部集；中途不能按时长均分猜测已看集数。
- 文件级人工锁定与外部映射锁定是两件事；确认某一文件不是给所有同名文件下全局结论。
- `provider_link.relation` 区分 `exact`、`part_of`、`contains`、`related`。跨范围映射使用真实分集 `MediaId` 集合，不能用一个全局 `episode_offset` 处理全部特别篇。
- Bangumi subject 或弹弹play作品可能覆盖一季的部分集；这种关联不能伪装成整季 `exact`。同一外部条目可以有多条有范围的关联；互相冲突的精确关联进入待确认状态。
- 候选和拒绝记录不能被普通查询当作已确认关系。编号相同、标题相似、缓存先返回均不是“exact”的证据。
- 元数据的公开评分与用户自己的评分分列，保留 provider、对象和范围，不混成一个数值。

### 5.4 远程文件身份

优先复用真实指纹；没有指纹时，使用 `FileKey::Item` 与 `file.server_item`：

```text
jellyfin:<server-id>:<item-id>:<media-source-id>
```

服务器实例、条目及播放版本均参与身份；缺少 `media_source_id` 时使用来源适配器约定的稳定单版本标识。流 URL 可能过期、含 token 或经反代变化，不能作为文件身份。现有共享源中用于替代指纹的字符串要通过兼容适配保留，不能当作真实 MD5 上传到公共识别服务。

## 6. 自动解析与人工确认

### 6.1 流程

```text
文件/远程条目入库
  ├─ 已人工锁定或 blocked → 保持决定；允许独立刷新已绑定作品资料
  ├─ 已有可验证 TMDB 引用 → 拉取相应实体并校验层级
  ├─ 已有文件绑定/已确认跨库映射 → 复用，不重复扫指纹
  └─ 身份尚未确定 → 按顺序刮削
       ① 弹弹play文件识别
          ├─ 返回真实弹幕库ID及作品资料 → 保留独立的弹幕绑定证据
          └─ 外部数据库链接/明确映射 → 验证 TMDB 作品与具体分集
               ├─ 身份和位置明确 → 绑定，取 TMDB 图文资料
               └─ 失败/不可用/不唯一/只有作品无分集 → 进入②
       ② 正则解析文件名 + 目录 + 年份 + 类型 → TMDB 查询与分集定位
          ├─ 作品与分集均满足自动确认规则 → 原子绑定
          └─ 不唯一/冲突/缺少集定位 → 保留原条目，进入人工确认
```

“以TMDB为中心”描述规范身份和展示来源，**不改变“弹弹play → 正则/TMDB”的新文件识别顺序**。②可利用①已验证的作品身份缩小范围，但不能覆盖明确冲突；即使①只确定了弹幕，②也可继续独立确定媒体身份。当前鉴权不满足或远程文件无可用指纹时，记录①不可用并进入②，不把其他识别服务插为默认中间步骤。已有Jellyfin等来源的明确身份属于入口复用，不算重新刮削。

### 6.2 自动接受的边界

| 证据 | 可以自动做什么 | 不可以做什么 |
| --- | --- | --- |
| 用户明确绑定 TMDB 实体 | 验证存在及层级后绑定，并锁定 | 静默覆盖其他文件的人工选择 |
| Jellyfin 提供正确层级的 TMDB ID | 验证后确立相应作品/分集身份 | 把分集 ID 当 TV ID；默认认同其他供应商的集号 |
| 弹弹play `onlineDatabases` 的 TMDB URL | 验证域名、实体类型、路径、实际对象后关联相应层级 | 由整剧链接推导季/特别篇映射 |
| 已确认的逐集映射 | 复用相应文件/范围 | 外推到未验证的下一季或 SP |
| 仅标题一致 | 排入候选，补查原名、别名、年份、类型、季集数据 | 取第一条搜索结果写库 |
| `S02E03` / `SP1` 文件名 | 生成位置/类别线索 | 单凭正则判定作品；把 `SP1` 固定映射到 `S00E01` |

自动文本匹配的初版使用可解释规则，而不是凭空定义一个“0.9 置信度”：作品候选须有一致的名称证据及明确年份/已验证目录绑定等附加约束，无同等候选；分集必须在已确认编排中存在且没有冲突。缺少充分证据即待确认。记录命中的规则、候选和失败原因，便于 CLI 与用户检查。

即便作品身份已确定，某个文件的季集仍可未知：媒体库可以展示它属于该作品的待确认文件，而不是把整部作品丢回无名文件堆。

### 6.3 文件名解析

替换仅返回 `Option<i64>` 的接口，建议返回：

```rust
struct NameHints {
    titles: Vec<String>,
    year: Option<u16>,
    season: Option<u32>,
    episodes: Vec<u32>,      // S01E01-E02 等明确范围
    absolute: Option<u32>,   // 长篇的绝对集数，不等于季内集数
    special_label: Option<String>, // 原样保留 SP1 / OVA / OAD 等
    release_group: Option<String>,
    version: Option<String>,
    evidence: Vec<HintEvidence>,
}
```

识别 `S00E01` 时保留 `season=Some(0)`；解析 `SP1` 只给出特别篇线索，不产生季内位置。来源目录可补作品名、年份与季号；同一作品候选和季详情按缓存键共享，避免整季每个文件重复搜索。正则是线索提取手段，不是数据库 ID 生成器。

### 6.4 异步、安全与性能

- 网络请求在后台执行，数据库写入沿用引擎收件箱模式；每次提交检查来源扫描代次、文件身份和绑定 revision。
- 用户在请求期间手动改配、解除、删除来源或打开另一文件，迟到结果不得覆盖。
- 元数据、弹幕、账号记录分队列；已确定媒体身份后的海报/标题刷新无需等待弹弹play弹幕请求或旧Misaka检索完成。
- 并发按供应商和进程整体限额，而不是每个模块各开 4 个造成倍增；初始可沿用 4 个识别任务上限，数值通过实际额度与耗时调整。
- 区分 `not_found`、`ambiguous`、`unauthorized`、`rate_limited`、`unavailable`、`invalid_payload`。超时/429 不是“TMDB 没收录”，不能触发持久误配或覆盖缓存。
- 读请求可受限重试；确认、账号写入与发布评论不能盲重试。缓存键包含 provider、实例、对象类型、语言和版本。
- 先显示已保存资料再后台刷新；缓存过期失败仍保留资料。错误不缓存成成功的空结果。
- 自动识别只发送必要的文件名、指纹、大小等；完整本地路径、Jellyfin 凭据和账号 Cookie 不上传到公共服务。沿用/补齐在线识别的用户控制，不扩大私有媒体信息上传范围。

## 7. 弹幕、评分评论、观看记录各司其职

### 7.1 弹弹play弹幕

```text
打开某个文件
  ├─ 本地同名 XML/JSON → 原有路径
  ├─ 已有该文件的有效弹弹play绑定 → 调用弹弹play读取弹幕
  └─ 无绑定 → 弹弹play文件识别，或按已确认 TMDB 引用查弹弹play关联作品
             → 校验具体分集 → 用户选择或可靠自动确认 → 保存文件级绑定
```

本次内置在线弹幕主链使用弹弹play。现有本地外挂弹幕、插件及旧Misaka绑定保留原有能力与兼容路径，不因这次编目改造删除；它们不影响TMDB身份，也不加入新的默认媒体刮削顺序。

- `search/episodes?tmdbId=…` 返回候选集合，不提供“TMDB S02E03 唯一对应哪条弹幕”的保证。先定位相关弹弹play作品，再依据真实详情、已确认范围和分集证据选择。
- `episodeId` 按不透明标识保存，不能再 `%10000`、乘法合成或正负号路由。
- 供应商 `shift` 和用户手动偏移分别保存与合成；按压制版本/文件生效，不污染作品元数据。
- TMDB 正确而弹幕未知时，显示正确媒体资料，正常播放；不为了找到弹幕把作品改成另一个。
- 媒体重新绑定只使“由旧媒体推导”的自动弹幕候选失效；明确基于同一文件指纹或用户选择的弹幕绑定独立保留。不能把所有弹幕关联无差别清掉。
- 发送弹幕是否可用取决于供应商能力、授权和真实 episode ID，不取决于是否绑定了 TMDB。

### 7.2 Bangumi评分与评论

- 作品评分、短评/评论均取Bangumi；用户评分和发布评论也使用已授权的Bangumi账号。TMDB的 `vote_average`、弹弹play的综合评分和评论不再作为默认展示或失败回退来源。
- 优先复用已有 `bangumiUrl` 和显式链接；无链接时可检索候选并人工绑定。TMDB `/find` 不支持直接用 Bangumi ID 代替外部源。
- 评分与评论绑定到准确的subject及覆盖范围。同一TMDB TV包含多季/篇章时，显示当前范围对应的Bangumi资料，不合并平均分；多个subject时允许选择。
- 没有可靠Bangumi关联、该作品未收录或当前不可访问时，保留已有Bangumi缓存，或不显示评分/评论；不能显示其他站评分却标成Bangumi，也不为电影/真人剧虚构关联。
- 评论缓存键包含Bangumi subject、账号相关性和当前范围。沿用现有晚到响应检查与失败保留缓存机制；网页短评接口仍按第3.3节独立适配，不承诺公开v0所未提供的能力。
- 评分始终保留提供方和作用范围；某一季的评分不得变成整部多季电视剧的评分。
- 本地 `MediaMark` 按字段原子编辑，整条替换使用 revision CAS。`rating=None` 表示未决定，`Some(0)` 表示清除；`review=None` 与 `Some("")` 同样区分未决定与明确清空。每次本地编辑推进 revision，防止同文本/同评分 ABA。
- 评分公开命令接受 0–10；0 清除，越界明确拒绝而非夹取。短评输入只在读取完成后初始化一次；用户清空不能被自动填回，提交失败保留草稿。
- 收藏、评分、短评任务保存完整本地字段快照；同账号同 Bangumi subject 的任务从预读前至回读共用条目锁，与电影已看/分集已看写入互斥。发送前原子重验映射、记录、token和租约；超时保留 unknown，先回读原目标，不能直接重发。撤销关联仍允许既有 unknown 只读核对，不允许凭旧关联重新发送。
- 原生队列持久化在 SQLite；独立 `web_app` 目前使用同页面共享的内存库，刷新后不持久。此处记录当前实现边界，不作为全端完成的依据。

### 7.3 弹弹play / Bangumi观看记录

- 本地用 `MediaId` 保存已看/未看、时间与作用范围；再通过各自已确认的逐集映射，读取/写入用户启用的弹弹play、Bangumi观看记录。两边都启用就分别同步，一边失败不撤销另一边的成功结果，也不阻塞播放。
- **账号观看记录与文件续播进度分开**：播放秒数、压制版进度、弹幕偏移仍按本地文件保存并走Erika原有设备同步；不能假设外部账号支持同等精度，不能把远端“已看”当成同一视频的精确续播位置。
- 弹弹play按真实episode ID，Bangumi按真实episode ID及subject/范围定位；不凭季内集号、ID算术或标题相似上报。只知subject而未知具体分集时，不把某一集完成改成整条目看完。
- 映射缺失/有歧义或未登录时只保存本地记录。待映射确认并按账号同步设置允许补传后再执行；重新匹配不能把之前对另一作品的已看事件盲目重发。
- 每个待同步状态保留账号/提供方、目标外部ID、范围、映射revision、本地变更revision、待发/成功/失败状态。发送前重验映射；失效映射暂停该任务，不把旧记录发送给新绑定目标。
- 使用稳定的本地操作标识去重，保存各账号读取基线及确认结果，避免导入→上报→再次导入的循环。写入超时先回读核对再决定重试；当前存在本地待发改动时，较旧远端状态不得覆盖人工已看/未看。无法判定先后时保留冲突，不谎称已收敛。
- 公开元数据缓存不自动触发收藏、已看、评分、短评写入；账号同步与公开元数据解析是两条任务流。接口能力仍以各站实际授权与现有适配验证为准，不据设计宣称已实现双向同步。
- Movie 直接关联 Bangumi subject，不制造假分集。Bangumi 收藏状态和电影已看共用远端字段；本地两项明确决定相反时，保留各自记录并拒绝新的账号写入，显示冲突，不按队列先后覆盖。未确认写入仍可回读。撤销电影已看时保留明确的“在看／抛弃／想看”，没有本地收藏决定时才使用“想看”；预读后收藏 revision 改变则重新预读。

### 7.4 新番入口

新番名单、播出日历仍来自内置服务，未登录也可浏览。未解析行用独立的来源标识及标题展示，不能为此写入占位媒体节点；打开后异步解析，成功才转到真实 TMDB UUID。简介与美术只使用 TMDB；评分评论按第7.2节取 Bangumi。解析失败保留来源行及可重试错误。

## 8. 显示与交互

### 8.1 按职责投影，不用一个通用fallback混合所有字段

| 展示内容 | 主来源 | 缺失/失败时 |
| --- | --- | --- |
| 作品标题、简介 | TMDB详情 | 用户明确覆盖优先；依次保留TMDB缓存、来源标题或文件名；简介可留空 |
| 海报 | TMDB `poster_path` / `posters[]` | 保留已验证的同作品TMDB缓存，再使用普通占位；不能清掉有效旧图 |
| 大幅背景海报 | TMDB `backdrop_path` / `backdrops[]` | 保留同作品的有效缓存；无图使用原生纯色/渐变背景，不拉伸竖海报充当横图 |
| 标题图片 | TMDB `logos[]` | 保留同作品的有效Logo缓存；无图以文字标题展示，不从海报裁字、不生成假的Logo |
| 评分、评论 | Bangumi | 仅保留对应Bangumi缓存；未关联/无数据则不显示，不能回退TMDB或弹弹play评分评论 |
| 已看标记 | 本地媒体状态 + 已验证的弹弹play/Bangumi账号记录 | 保留本地决定；同步状态单独处理，不因上游不可用显示成未看 |

这不是继续并列多个刮削中心：图文从TMDB获取，同作品缓存用于离线展示；恢复有效TMDB资料后刷新。评分评论不参与图文资料的字段回退。来自图片接口的 `vote_average` / `vote_count` 只用于挑选图片，不是作品的Bangumi评分。

- TMDB某字段为空、404、超时或图片损坏，不应擦掉现有有效资料；重新绑定到另一作品时则不能复用旧作品的图文缓存。
- 标题先取目标语言版本，再取TMDB原名；原名、别名保留用于搜索。简介缺翻译时可另查原语言资料，仍缺失才沿用已有资料。
- 作品墙一部TV一张主卡，内部按季展示；Movie独立一张卡。未解析文件仍可浏览与播放，可靠匹配后进入对应作品。
- 评分注明Bangumi及必要的季/篇章范围，不在每张卡片额外堆“来自TMDB”等说明。

#### 8.1.1 图片资源与协议模型

不再把图片全塞到一个 `image: String`。保留候选资源与选择结果，协议示意如下（非当前已有类型）：

```rust
struct ArtworkAsset {
    media: MediaId,
    owner: ProviderRef,       // 真实TMDB TV/Movie/Season，不跨作品复用
    kind: ArtworkKind,        // Poster / Backdrop / Logo / Still
    file_path: String,        // TMDB原始路径，不是带凭据的任意URL
    language: Option<String>,// iso_639_1；null保留为空，不当成英文
    width: u32,
    height: u32,
    aspect_ratio: f64,
}

struct MediaArtwork {
    poster: Option<ArtworkAsset>,
    backdrop: Option<ArtworkAsset>,
    logo: Option<ArtworkAsset>,
}
```

图像接口已确认的资源字段有 `file_path`、`iso_639_1`、尺寸、宽高比和图片投票。格式不能一律假设JPEG：下载时校验真实Content-Type及可解码性。TMDB的公司/电视网Logo格式转换说明，不代表作品 `logos[]` 必然有同样的格式保证；不擅自改扩展名猜另一个资源。

`MediaDetail`投影应分别带文字标题、简介、`MediaArtwork`、可选Bangumi评分与评论上下文、已看状态；不能把标题字符串换成Logo URL。即使视觉上显示Logo，也保留文字标题用于搜索、无障碍与无图回退。季可以继承所属TV的背景/Logo，但保留资源实际owner；分集缩略图使用季详情的 `still_path`，不误当作品背景。

#### 8.1.2 语言与选图

- 文字请求可用 `zh-CN` 等locale；图片筛选用 `iso_639_1` 语言标记。官方说明目前图片不区分地区变体，不能保证 `zh-CN` 与 `zh-TW` 返回不同图。
- `/images` 的 `language` 会过滤结果；显式使用 `include_image_language` 纳入目标语言、原语言、英文及 `null`（去重，例如 `zh,ja,en,null`），避免无语言背景被过滤。查询无候选时可补不带语言筛选的请求；空集合与请求失败分别缓存。
- 海报/Logo优先目标语言，其次原语言，再到英文/无语言和其他可用候选；背景优先无语言图。这里是Erika的选图策略，不把上游默认排序当作固定契约。
- 同档候选按显示比例、清晰度及图片投票稳定选择；同分用路径作稳定次序，避免每次刷新跳图。Logo完整等比容纳并保留透明通道，不能用海报的裁切方式；背景可按容器裁切但不挤压变形。
- 用户明确指定的图片保留为覆盖，自动刷新不得改掉；清晰度不足时选当前可用最大尺寸，不能靠放大声称获得原本不存在的细节。

#### 8.1.3 传输、缓存与原生展示

- 图片配置与图片集合分开缓存。地址由配置提供的HTTPS图片基址、允许的尺寸档及 `file_path` 组成；不能拼接客户端任意域名。
- 缓存键包含提供方、实体类型/ID、图片种类、语言集合、路径、尺寸和解码/渲染版本；绑定revision防止迟到图片覆盖新作品。重新绑定失效旧投影，不删除仍被其他节点引用的资源。
- 墙上只加载可见海报；进入详情后异步加载背景与Logo，不能为整库预取所有原图。图片失败不阻塞标题、简介和播放；先验证新图，再替换旧图。
- 按最终显示的物理像素选择尺寸，图标与矢量图片遵守“2倍目标物理像素栅格化/解码，再缩半显示”；位图取足够尺寸，不无条件请求原图。缓存与解码设字节数/像素数上限，避免大背景压垮手机或TV内存。
- 原生端用原生图片/矢量解码及合成展示这些静态资源，不引入WebView。若上游给出当前渲染器不能安全解码的格式，保留错误状态并使用同作品可支持资源或文字标题；不为显示Logo加载可执行脚本。WebUI沿用自身图片组件，但资源选择与身份规则相同。
- 浅色/深色底分别检查透明Logo可读性；必要时使用原生遮罩/底色或文字标题，不把文件内的文字/图形裁掉。高DPI、大屏背景、窄屏布局均在实际尺寸验收。

### 8.2 详情与手动匹配

- TV 详情用季选择器区分“第 1 季 / 第 2 季 / 特别篇”，季内列实际分集；电影无季选择器。
- 所有列表、选集、滚动定位以 `MediaId` 为 key，显示 `S02E03` 或真实特别篇名称，不能继续以 `number` 唯一定位。
- 手动流程为“选作品 → 选季/电影 → 选真实分集/多集范围 → 预览 → 确认”。
- 原来的按编号/按顺序批量匹配保留，但限定在已选定的真实分集集合；跨季或跨特别篇不自动加固定偏移。
- 高亮冲突、超范围、缺号、多个文件版本；确认前显示实际变更，不默认覆写已锁定项。
- “重新匹配媒体”和“选择弹幕”是两个动作；修改一个不隐式篡改另一个。
- 布局沿用已有原生控件和设计语言；平台差异只在布局，不在匹配规则。空状态保持一句话加直接操作，不新增长篇解释面板。

### 8.3 连播与已看

- 队列在选定季/编排内按真实分集顺序构造；默认不把 `S00` 插进正片中间。
- 正片跨季播放使用明确顺序；未提供可验证插入位置的特别篇不靠编号猜观看顺序。
- 同一文件覆盖多集时队列去重；同一集多个文件按用户选择/可用性选择版本，不连着播多个压制版。
- 显式人工已看/未看优先于自动进度推导；未手动决定时仍可沿用 90% 完播规则。电影按自身节点统计，合辑按已知覆盖范围统计。

## 9. Erika 元数据网关（实现中，未部署）

保持现有 `/{token}/api/v2/*` 兼容接口不变。新增不混用弹弹play DTO 的 `/metadata/v1/*` 路由；以下为设计契约，具体实现以 `erika-gateway/src/metadata.rs` 为准；**生产地址尚未部署验证**：

| 路由 | 请求与响应 |
| --- | --- |
| `GET /metadata/v1/capabilities` | schema 版本、支持供应商、能力状态；不回传凭据 |
| `GET /metadata/v1/search?kind=tv\|movie&query=…&year=…&language=…&page=…` | 类型明确的候选及分页；不自动选第一条 |
| `GET /metadata/v1/tmdb/tv/{id}?language=…` | 剧集、季引用及显示资料 |
| `GET /metadata/v1/tmdb/tv/{id}/seasons/{number}?language=…` | 实际季与全部已返回分集身份，保留季 0 |
| `GET /metadata/v1/tmdb/movie/{id}?language=…` | 电影对象 |
| `GET /metadata/v1/tmdb/images/configuration` | 受控HTTPS图片基址、按资源种类可用的尺寸档与配置版本；由服务端缓存TMDB configuration，不暴露凭据 |
| `GET /metadata/v1/tmdb/{kind}/{id}/images?language=…&include_image_language=…` | `kind`仅接受 `tv` / `movie`；返回类型化海报、背景、Logo候选及语言/尺寸；保留空集合与读取失败的区别 |
| `GET /metadata/v1/tmdb/tv/{id}/seasons/{number}/images?language=…&include_image_language=…` | 季海报候选；背景/Logo继承TV资源由投影层处理，不伪造季资源 |
| `GET /metadata/v1/tmdb/find?source=…&id=…` | 仅允许 TMDB 已支持的外部源；结果保留对象类型 |

图片查询限制语言集合长度与合法值，`null`表示纳入无语言资源；缓存键保留规范化后的整个语言集合。配置接口返回资源能力，不把可用尺寸硬编码在每个UI里。图片字节默认走受控配置中的图片服务；若需代理，必须遵守下述域名、重定向和资源大小限制。

统一响应携带 `schema_version`、typed provider refs、资料更新时间和缓存状态。错误用稳定 code 与 `retry_after`（若可用），不只给自然语言字符串。资料含的 URL 只作为资料；服务端不接受客户端任意上游 URL。

部署要求：

1. TMDB 凭据只在服务端配置，由管理员提供；不从 Misaka 管理 API 抽取或下发 key。可以由部署配置共享同一合法凭据，具体额度/许可需核对。
2. 新接口使用 HTTPS；仓库历史记录的是 HTTP 内置识别地址，本轮未核实线上是否已升级。不能把历史 HTTP 状态写成新接口已满足安全要求。
3. 匿名公开元数据可以不依赖用户弹弹play账号，但仍需网关访问策略、IP/设备与全局限流、缓存和单请求资源上限。内置公开应用标识不是真正秘密，也不能防止别人复制调用。
4. 固定上游与允许的路由/参数，禁止开放代理；图像代理限制允许域名及重定向，防止 SSRF。用户自建 Jellyfin 地址由客户端/插件自己的来源连接处理，不交给公共网关请求。
5. TMDB 读取与 Misaka 弹幕检索资源隔离；监测缓存命中、429、错误率、响应时间，不在日志写 token、完整路径或私有搜索历史。
6. 服务端配置图片地址/尺寸，客户端按实际显示像素选择足够清晰的尺寸；不无条件下载原图。关于页落实 TMDB 标识及指定声明。
7. 对 `web_app` 明确 HTTPS、CORS、允许方法、缓存及凭据策略；不能只验原生 HTTP 客户端。

## 10. Jellyfin 插件宿主契约（实现中）

`library.getShows()` 不是媒体源导入接口。来源目录、typed 身份验证、持久元数据任务与稳定来源播放已接入；完整接口与当前验收边界见[插件来源目录与播放接口](plugin-sources.md)。不开发 Jellyfin 插件本体。

### 10.1 能力与权限

`library.apiVersion` 为 2，来源能力包括 `sources.import`、`sources.pages`、`sources.status`：

- `library.registerSource(descriptor)`：返回宿主 source ID；来源归调用插件所有。
- `library.beginScan(sourceId)`：返回扫描事务/代次。
- `library.upsertItems(scanId, items, page)`：从 0 连续分页导入，同页同内容重试幂等。
- `library.finishScan(scanId, { pages, complete })`：完整成功才发布并隐藏缺失条目；取消、超时、失败保留上一完整目录。
- `library.getSourceItems(sourceId, {cursor, limit})`：有界分页，返回 `{generation, items, nextCursor}`；完整重扫后旧游标明确冲突。
- `library.getSourceItem(reference)` / `library.getSourceStatus(reference)`：按来源条目和播放版本查询；状态含文件 revision、绑定决定、所属任务、待确认候选与单项错误，不暴露原始任务输入。
- `library.getMedia(query)` / `library.getFiles(mediaId)`：按新媒体身份查询。
- `library.setWatched({ mediaId, watched, revision })`：精确节点 CAS 操作，不以整数集号寻址。

沿用 `library.read`/`library.write`，来源导入另加 `library.sources` 能力权限，限定插件只能操作自己的来源；文件绑定、删除、账号写入都按宿主策略检查。`upsertItems` 的成功是宿主落库确认，不是 JS 已把调用排入 outbox。

### 10.2 条目数据

示意数据，字段语义属于新契约：

```json
{
  "sourceItemId": "server-item-id",
  "serverId": "server-instance-id",
  "mediaSourceId": "playback-version-id",
  "kind": "episode",
  "seriesItemId": "parent-series-item-id",
  "seasonItemId": "parent-season-item-id",
  "providerRefs": [
    {"provider": "tmdb", "kind": "episode", "id": "63056"}
  ],
  "seriesProviderRefs": [
    {"provider": "tmdb", "kind": "tv", "id": "1399"}
  ],
  "seasonNumber": 1,
  "episodeNumber": 1,
  "episodeNumberEnd": null,
  "ordering": "unknown",
  "title": "分集标题",
  "playbackHandle": "plugin-owned-opaque-handle"
}
```

ID 组合仅说明字段层级；接收方仍验证它们是否确实指向同一分集。`ordering` 不知则为 unknown，不能由插件默认为 TMDB。TMDB 分集 ID 明确且验证成功时以其位置为准；没有分集 ID又不能证明编排一致时，只确认作品、保留分集待确认。

播放通过 handle 请求当前 URL、请求头与有效期，凭据保留在系统凭据库/插件受保护的连接配置，不进入媒体元数据、普通日志或公开共享。插件条目的可信度不超过用户授权的来源，不能据此提交全局指纹共识。

### 10.3 旧接口兼容

- `getShows()` 与 `setEpisodeWatched(animeId, number, …)` 保持旧命名空间；只对能无歧义映射的旧对象提供兼容行为，不把 TMDB ID 填入旧字段。
- 异步已看请求捕获详情访问代次、关联和本地记录 revision、插件运行实例及权限；回包全部重验。即便离开后回到相同作品和季，也不接受前一次访问的请求；同页普通刷新不改变访问代次。
- 新的 TMDB-only 项由 v2 查询接口覆盖；旧 API 不可能表达的对象不伪造旧 ID。能力探测让插件决定使用哪个 API。
- `pluginResolveUrl` 的 `type: play` 可返回自己目录中的 `sourceItem` 引用。宿主重新取得来源快照并验证，不接受插件任意注入媒体绑定；`mediaContext` 是有 `library.read` 权限时由宿主提供的只读上下文。旧 `searchTitle` 保留为候选提示而不是最终身份。
- NipaPlay 原插件继续按既有 API 运行；新 Jellyfin 插件使用 v2，不依赖“ID 为正就是弹弹play”。

## 11. CLI 与人工/AI可操作性

复用 Bern 协议生成机制，不新做一个与 GUI 行为不同的命令行工具。以下是**拟新增协议命令名**，准确 shell 拼写与参数形状由生成后的 CLI schema 确认：

| 命令 | 语义 |
| --- | --- |
| `catalog.inspect` | 查看文件的媒体绑定、弹幕绑定、证据、revision 与锁定状态 |
| `catalog.search` | 返回 typed refs、候选和判定理由，不写库 |
| `catalog.resolve` | 指定文件/来源进行解析，返回任务 ID；可 dry-run |
| `catalog.bind` | 引用真实作品/分集，带 expected revision；可按文件批量原子确认 |
| `catalog.unbind` | 清除媒体绑定并置 blocked，除非用户明确选择允许重新识别 |
| `catalog.map_provider` | 显式添加带范围的提供方关联，检查冲突 |
| `catalog.undo` | 通过事务 ID恢复绑定；遇到后续修改则报告冲突，不能硬覆盖 |
| `catalog.job` | 查询进度、失败原因、下一步需确认项 |
| `danmaku.bind` | 独立选择文件的弹幕供应商/episode ID |

要求：参数结构化、结果可机读、批量先预览、写入有幂等请求 ID/输入 revision、返回逐项错误。第三方 AI 可以读取候选后调用相同确认接口，但不因此获得静默改全库或向账号发布内容的权限。本方案不加入 LLM 服务依赖。

## 12. 当前格式初始化、同步与备份

### 12.1 只初始化，不迁移

- 空数据库一次性执行当前完整 schema；没有有序 migration、ALTER TABLE 升级、数据回填或升级前备份。
- 用 `application_id`、`user_version` 与完整用户表/索引/触发器结构识别当前格式。与当前 schema 的内存参考库比较后才启用 WAL；同版本缺表、错列或多触发器同样明确拒绝。原文件不修改，不建迁移副本，也不回退空内存库掩盖失败。
- 不建立 Legacy 节点、旧身份对照表、持久别名或“确认媒体身份”归并界面。
- 用户的现有数据库不会被自动清空。开发验收使用独立目录内的新数据库。
- 规范作品和分集来自 TMDB。弹弹play/Bangumi 等外部记录是来源资料，不能自动转换成规范媒体身份。

### 12.2 当前格式快照与备份

- `.ebp` 备份、跨设备同步保留，但只支持当前格式；不提供旧格式转换器、旧同步仓库导入或自动升级。
- 快照携带真实媒体节点、typed refs、文件绑定、独立弹幕绑定、已看状态和评分。文件用指纹或服务器条目身份，不携带本地 rowid。
- 两设备发现同一 TMDB 对象时，按类型及引用定位接收端节点，在本次导入里重写父子、绑定和状态引用；不保存旧 UUID 别名。
- 整批导入原子执行。人工解除、人工未看和绑定 revision 参与冲突检查；不同压制版的秒数不合并。
- 图片只同步引用和人工选择；账号凭据走独立受保护路径。`account_watch_sync`、`account_sync` 的发送状态、执行token和租约均为设备本地状态，不从快照复制成可执行请求。`MediaMark.revision` 在接收端生成，重复导入不推进；不以外部 revision 覆盖本地 CAS 时钟。
- 备份及同步格式版本必须拒绝不支持的输入，而不是部分导入。已完成两个隔离原生实例的 `.ebp` 导入、重复导入，以及本地 WebDAV 双向收藏/短评/显式未看同步和重复同步幂等；评分清零、来源目录、非空发送队列及受保护凭据的完整运行时往返仍待验收。范围和证据见[验收记录](tmdb-library-acceptance.md)。

### 12.3 远程共享兼容

NipaPlay v1 共享中的 `animeId/episodeId` 继续只表达真实弹弹play身份；不输出 Misaka 负数、Bangumi 编码或 TMDB ID来冒充。只有 exact、可验证的旧映射才能填旧字段。

Erika 间增加独立版本/能力协商的媒体目录 payload，携带 movie/TV/season/episode、typed refs 和来源文件；不得让新消费者必须依赖旧的正数过滤。现有按 `animeId` 寻址的旧路由不能突然返回 UUID。对没有旧身份的影片，v1不能表达完整编目，但新共享和本地使用必须完整可见；具体旧客户端展示能力在兼容测试中明确记录。

## 13. 实施拆分与文件落点

以下阶段有依赖，不能当成互不相关的多人并行修改；也不能在阶段 1/2 完成后就宣称“TMDB 已转正”。

| 阶段 | 实现内容 | 主要落点 | 完成条件 |
| --- | --- | --- | --- |
| A. 契约与无损读取 | 中立身份、provider refs、原始集号/季/链接、独立图片资源、错误类型；录制脱敏响应样本 | 新 `erika-catalog/`；`erika-dandanplay/src/lib.rs`、`bangumi.rs`、`custom.rs` | 不再在新适配层丢弃 SP、季、外链和偏移；旧调用仍可用 |
| B. 网关元数据 | TMDB凭据配置、详情/图片集合/图片配置白名单、缓存、限流、能力查询 | `erika-gateway/src/lib.rs`、`main.rs`、`store.rs`，新增 `metadata/` | 契约测试通过；海报/背景/Logo都可获取；部署前单独验证凭据与网络，不影响旧Misaka路由 |
| C. 当前格式存储 | 完整 schema 初始化、事务 API、手动锁定、可恢复作业 | `erika-store/src/lib.rs`、`sqlite.rs`、`session.rs`、`portable.rs`、`sqlite/portable.rs` | 空库初始化、重复打开及并发访问安全；格式不符不改文件；wasm模型同步更新 |
| D. 共用 resolver | 已有身份复用，弹弹play识别→正则/TMDB查询，候选确认、按职责投影 | `erika-catalog/`；`modules/library/core/src/scan.rs`、`names.rs`、`matching.rs`、`lib.rs`；`erika-core/src/matching.rs` | 新文件顺序固定；本地/播放/远程共用规则；无弹弹play登录仍可进入正则/TMDB阶段 |
| E. 消费者切换 | TMDB墙/标题/简介/背景/Logo，季选择、电影、SP、搜索、连播、独立弹幕 | library/anime 的 protocol/core/ui；`erika-protocol`、`erika-ui`；`erika-core/src/queue.rs`、`search.rs`、`history.rs`、`player.rs`、`lib.rs` | 每个入口用真实节点；没有旧整数隐式跨季；静态图片原生展示，缺Logo用文字 |
| F. 账号与导出 | Bangumi评分/评论，弹弹play/Bangumi范围化观看记录，当前格式备份/同步、新共享 | anime `catalog.rs`、`marks.rs`、`sync.rs`、`reviews.rs`；sync模块；remote模块；`erika-core/src/sync_restore.rs` | 评分评论不再回退其他站；两站观看记录独立同步且不混续播进度；公开缓存不写账号；不支持的格式明确拒绝 |
| G. 插件/CLI 接入 | v2来源接口、权限、结构化播放上下文、查询/预览/绑定/撤销 | `erika-core/src/plugins/{host.js,mod.rs,hooks.rs,links.rs}`、相关protocol | Jellyfin插件只需调用稳定宿主协议，不直接写库；旧插件契约不变 |
| H. 联合验收与上线 | 脱敏真实样本、断网、并发、空库初始化、跨端和供应商冒烟 | 各crate测试、文档、发布流程 | 下表通过，网关先兼容部署、客户端后切换；有当前格式备份与错误保护 |

### 13.1 本工作区的冲突注意点

当前已有未提交的播放器、插件安装/市场、共享来源和 Cargo 依赖改动。尤其 `erika-core/src/plugins/mod.rs`、`player.rs`、`lib.rs`、`erika-protocol/src/lib.rs`、`erika-ui/src/player.rs`、`modules/library/core/src/shared.rs` 及各 `Cargo.toml`，不能按旧基线整文件覆盖。

实施前记录实际 diff，以小提交按领域推进；公共类型/协议先确定，消费者随后切换。同一文件只安排一个写入方。Jellyfin插件作者可以先按本文契约评审，但宿主接口稳定并发布能力版本前，不能把示例当已实现 API。

## 14. 验收清单

测试必须覆盖全链路，而不仅是海报墙看起来正确。

| 用例 | 必须得到的结果 |
| --- | --- |
| 新文件无已确认身份 | 先弹弹play识别；失败/不可用/不充分才走正则解析+TMDB查询，不插入默认Misaka阶段 |
| 已确认绑定或Jellyfin明确TMDB身份 | 验证/复用，不重复对同一文件跑全套刮削 |
| 弹弹play只匹配到弹幕、TMDB作品不确定 | 保存可靠弹幕证据，媒体继续解析；不以弹幕库ID伪装媒体身份 |
| `S01E01` 与 `S02E01` | 同剧不同季不同节点，选集、已看、评论和连播不串 |
| `S00E01` | 季 0 合法；与“无季号”不同，不被过滤 |
| `SP1` / OVA / OAD | 不默认等于 `S00E01`；可进入电影、特别篇或未解析条目 |
| Bangumi `type=1`、`sort=12.5` | 原始类型/排序保留，不变成 12 或 13 |
| 同数值 TMDB TV 与 Movie ID | 不合并、不共享观看状态 |
| 弹弹play的字符串 `bangumiId`、`seasonId`、外链 | 无损读取；未定义的编码不强转季号 |
| TMDB ID反查返回多个弹弹play作品或 `hasMore=true` | 不自动选择第一个，不假定候选唯一 |
| Jellyfin Episode 的 `ProviderIds.Tmdb` | 按分集处理，通过父Series取TV；缺失信息不伪造 |
| Jellyfin 非TMDB编排 / 未知编排 | 不把来源季集数字直接当规范位置；保留来源资料等待验证 |
| 同名重制、续篇、分割放送 | 年份/范围冲突进入待确认，不以名称合并 |
| 多集文件与一集多版本 | 绑定范围正确；队列不重复播放；进度按文件 |
| 未登录弹弹play | TMDB元数据、媒体库与播放可用；弹幕权限单独体现 |
| TMDB无海报/429/断网 | 保留旧资料与正常播放；错误不变成“不存在” |
| TMDB海报、背景、标题Logo齐全 | 三种资源分别显示，Logo透明且完整等比，文字标题仍可搜索/被无障碍读取 |
| 缺背景/缺Logo、图片损坏或格式不可解码 | 同作品缓存优先；否则背景用原生底色、Logo用文字，不裁海报冒充；异步旧图不覆盖新绑定 |
| 中文/原语言/无语言图片，高DPI及浅深色主题 | 语言集合筛选正确；无语言背景不遗漏，尺寸清晰有上限，Logo不裁切、不为静态图引入WebView |
| 无Bangumi关联或Bangumi接口失败 | 保留Bangumi缓存或无评分/评论，不改用弹弹play或TMDB评分评论 |
| 一部TV关联多个Bangumi subject | 所选季/篇章显示对应评分评论，不平均、不串范围 |
| 两站观看记录、单站失败、离线后恢复 | 弹弹play/Bangumi分别记录状态；映射准确才上报，不重发旧绑定事件，不把账号已看转换成精确续播秒数 |
| 本地人工未看与远端旧已看冲突、写请求超时 | 人工决定不被旧快照覆盖；先核对远端再重试，导入上报无循环；无法判断时保留冲突 |
| 批量匹配期间手动改配/解除 | 迟到结果不能覆盖；blocked 重启和同步后仍有效 |
| 新空库、第二次启动、多个模块同时开库 | 完整初始化只执行一次，数据保留，连接并发安全 |
| 非当前格式数据库 | 明确拒绝；原文件内容不变，不创建转换副本 |
| 当前 `.ebp` 导出与导入 | 完整还原当前快照；不支持的版本明确拒绝 |
| 两设备同对象、不同UUID与旧解除记录 | 合并保持引用；人工解除不被自动绑定复活 |
| 当前格式备份往返 | 评分/已看/文件/进度及关联一致，不复制账号发送中状态 |
| 插件扫描失败/取消/只收到部分分页 | 保留上次完整目录；不能清空来源 |
| NipaPlay兼容共享、旧插件 | 旧字段只出现真实旧身份；新能力通过版本化接口提供 |
| 多平台 | 原生桌面、手机、平板/折叠、TV、WebUI与可运行的web_app路径共用语义；逐端记录实际验收范围 |

自动化以 mock HTTP、供应商脱敏 fixture 与临时 SQLite 为主；真实请求只做只读冒烟。需要真实账号写入的测试另行确认，不借验收发布评论或修改收藏。

## 15. 上线前尚需确认的运行事实

这些不阻止设计与本地实现，但不能在交付时省略：

- 生产 Misaka 版本、已配置的 TMDB 鉴权/网络可达性、额度与数据许可；本轮没有读取线上密钥。
- NipaPlay官方网关是否允许并透传新增的 `search/episodes` 参数、字符串详情 ID与匿名资料查询。
- 真实脱敏样本中 TMDB外链覆盖率、弹弹play `seasonId` 的具体值、跨季/特别篇对应完整度；覆盖不足就保持待确认，不能以启发式覆盖率充当精确率。
- 新元数据HTTPS地址及浏览器CORS策略；海报、背景和Logo的真实语言覆盖、格式、尺寸及CDN访问能力；上线部署与密钥配置单独执行。
- Bangumi评论读取/发布与两站观看记录适配的实际授权、状态语义和失败核对能力；本轮未对真实账号做写入测试。
- 当前格式同步与备份剩余场景的实际往返验证，范围见[验收记录](tmdb-library-acceptance.md)；不向旧同步仓库发布当前格式数据。

## 16. 资料与核查入口

核查日期：2026-11-08。外部文档和上游 main 会变化，实施时保存实际使用的脱敏 fixture 与规格版本；不要把本文的查询时间当成生产测试记录。

### 官方接口

- [TMDB TV详情](https://developer.themoviedb.org/reference/tv-series-details)、[季详情](https://developer.themoviedb.org/reference/tv-season-details)、[电影详情](https://developer.themoviedb.org/reference/movie-details)
- [TMDB TV搜索](https://developer.themoviedb.org/reference/search-tv)、[电影搜索](https://developer.themoviedb.org/reference/search-movie)
- [TMDB外部ID查询](https://developer.themoviedb.org/reference/find-by-id)、[其他剧集编排](https://developer.themoviedb.org/reference/tv-series-episode-groups)
- [TMDB应用鉴权](https://developer.themoviedb.org/docs/authentication-application)、[图片配置/格式说明](https://developer.themoviedb.org/docs/image-basics)、[许可与署名](https://developer.themoviedb.org/docs/faq)
- [TMDB TV图片](https://developer.themoviedb.org/reference/tv-series-images)、[电影图片](https://developer.themoviedb.org/reference/movie-images)、[季海报](https://developer.themoviedb.org/reference/tv-season-images)、[图片语言筛选](https://developer.themoviedb.org/docs/image-languages)（本轮页面直连抓取未成功，图片部分依据官方页面的搜索索引核对；尚未携凭据实测）
- [弹弹play开放弹幕网络](https://doc.dandanplay.com/open/)、[官方Swagger JSON](https://api.dandanplay.net/swagger/v2/swagger.json)、[接口变动记录](https://doc.dandanplay.com/open/changelog.html)
- [Bangumi OpenAPI规格](https://github.com/bangumi/api/blob/master/open-api/v0.yaml)（实施时以仓库实际默认分支和具体版本为准）
- [Jellyfin剧集与特别篇规则](https://jellyfin.org/docs/general/server/media/shows/)
- [Jellyfin BaseItemDto](https://github.com/jellyfin/jellyfin/blob/master/MediaBrowser.Model/Dto/BaseItemDto.cs)、[ItemsController](https://github.com/jellyfin/jellyfin/blob/master/Jellyfin.Api/Controllers/ItemsController.cs)

### 上游与项目代码

- [Misaka TMDB适配](https://github.com/l429609201/misaka_danmu_server/blob/main/src/metadata_sources/tmdb.py)、[兼容详情schema](https://github.com/l429609201/misaka_danmu_server/blob/main/src/schemas/dandan/protocol.py)、[分集搜索路由](https://github.com/l429609201/misaka_danmu_server/blob/main/src/api/dandan/search.py)
- [项目存储与内置识别服务记录](README.md#存储与弹弹play2026-10-21)、[远程访问记录](README.md#远程访问2026-11-06)
- [现有同步设计](../modules/sync/README.md)、[纯Web能力边界](web-playback.md)

**交付判断：只有规范身份、季集结构、媒体/弹幕分离、当前格式备份与同步、全部消费入口共同完成后，才能称为“TMDB 主媒体库”。只改默认查询或展示来源不算完成。**
