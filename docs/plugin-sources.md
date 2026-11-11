# 插件来源目录与播放接口

2026-11-09 · 当前开发分支的 v2 宿主契约；Jellyfin 插件本体不在本次实现范围。以下接口已接入原生/浏览器存储及 JS 宿主，实际 App 的跨端联合验收尚未完成。

## 权限与身份

- `library.apiVersion === 2`。能力列表包括 `sources.import`、`sources.pages`、`sources.status`。
- 以下目录操作需要 `library.sources`；宿主按实际调用插件检查归属，不能在参数里指定 `owner`。
- 播放解析还需要 `url.resolve`；读取全库或当前播放的 `mediaContext` 需要 `library.read`；人工改绑与已看写入需要 `library.write`。
- 方法均返回 Promise；成功表示宿主事务已经完成，不只是 JS outbox 已入队。失败为带 `code` 的 Error（如 `invalid`、`conflict`、`permission_denied`、`storage`）。
- 所有请求不超过 4 MiB，结构拒绝未知字段。服务器 ID、来源条目 ID、版本 ID、播放 handle 必须是稳定标识，不是网址、账号 token 或 Cookie。

## 注册与完整扫描

```js
const source = await library.registerSource({
  id: 'movies', serverId: 'server-a', name: '电影', content: 'movie'
});
const scan = await library.beginScan(source);
await library.upsertItems(scan.id, items, 0);
await library.finishScan(scan.id, {pages: 1, complete: true});
```

`content` 为 `anime`、`series` 或 `movie`。同插件的 `id` 重复注册返回同一 source；可更新名称与分类，不能换 `serverId`。

扫描返回 `{id, source, nextPage, state, expiresAt}`，租约为 3600 秒，每次新分页提交后续期。页号从 0 连续；同页同内容重试幂等，不同内容冲突。每页 1–1000 项、不超过 4 MiB；一次扫描最多 1000 页、100000 个不同条目。重复条目不能绕过页数上限。

`finishScan` 的 `pages` 必须等于已落库页数。只有 `complete: true` 才发布快照并隐藏缺失地址；取消用 `complete: false`。新扫描取代旧扫描的租约，但暂存、取消、失败均不改变上一次已发布目录。确实成功读取到空目录时用 `pages: 0, complete: true` 发布空目录。缺失地址不删除文件、续播或人工绑定。

## 来源条目

```json
{
  "sourceItemId": "movie-a",
  "serverId": "server-a",
  "mediaSourceId": "original",
  "kind": "movie",
  "providerRefs": [{"provider":"tmdb","scope":"","kind":"movie","id":"71"}],
  "seriesProviderRefs": [],
  "ordering": "unknown",
  "title": "电影标题",
  "playbackHandle": "movie-a-original"
}
```

分集还可提供 `seriesItemId`、`seasonItemId`、`seasonNumber`、`episodeNumber`、`episodeNumberEnd`；未知为 `null`，季 0 合法。`seriesProviderRefs` 的 TMDB 引用属于 TV，`providerRefs` 的分集引用属于 Episode，不能混用层级。只有明确使用 TMDB 编排时才填 `ordering: "tmdb"`；不知道就保留 `unknown`。

可选 `fingerprint: {md5, size}` 用于复用文件身份；格式合法不等于宿主亲自读取验证过，不会作为已验证本地指纹上传。没有指纹时，文件身份由插件、服务器、条目、播放版本共同确定。换播放 handle 不换文件；不同版本分开保存进度。

存储结果 `StoredItem` 为 `{source, owner, generation, file, url, item}`。`url` 是内部 `erika-source://…` 地址，不是媒体流，不可直接交给外部播放器。`file` 和 `source` 是本机 ID，不可充当跨设备身份。

## 有界读取与单项状态

```js
let cursor;
do {
  const page = await library.getSourceItems(source, {limit: 100, cursor});
  for (const stored of page.items) {
    // 消费当前页。
  }
  cursor = page.nextCursor;
} while (cursor !== null);

const reference = {source, sourceItemId: 'movie-a', mediaSourceId: 'original'};
const stored = await library.getSourceItem(reference);
const status = await library.getSourceStatus(reference);
```

`getSourceItems` 返回 `{generation, items, nextCursor}`，不是整库数组。默认 100 项，`limit` 范围 1–1000；响应不超过 4 MiB，可能提前结束一页。游标是不透明字符串，绑定调用插件、来源与已发布代次；不要自行修改它。重扫完整发布后旧游标返回 `conflict`，重新读取第一页，不能拼接两个代次。暂存或取消扫描不使游标失效。

`getSourceItem` 是按条目与播放版本定位的单项查询，不遍历整库。不存在返回 `null`，不属于此插件的来源返回错误。

`getSourceStatus` 在同一存储快照里返回：

- `item`：当前 StoredItem。
- `revision`、`binding`：当前文件绑定版本和决定，包含人工 `blocked`；绑定为空不表示播放不可用。
- `job`：此来源条目的最近任务，或 `null`。包含 `id`、`state`、`phase`、`current`、`error`、`nextAttempt`、`candidates`、`candidateCount`。不返回原始 job payload，也不返回共享指纹下另一来源的任务。

任务状态含 `queued`、`running`、`retry`、`pending`、`bound`、`failed`、`cancelled`、`stale`。成功是 `state: bound, phase: complete`。`current: false` 表示条目代次或文件 revision 已改变；旧候选不再输出。当前 `pending` 返回候选摘要（最多 100 项，完整数量为 `candidateCount`），候选不等于已确认绑定；后续确认仍经过正常预览/CAS 写接口。状态读取不会发起网络请求，也不会重启任务。

完整扫描发布后，启用的来源插件由宿主后台验证 typed TMDB 身份。持久任务按文件租约协调多引擎；迟到结果重验完整来源快照与文件 revision，不能覆盖人工决定。停用、撤销权限或关闭自动匹配会阻止提交。短暂服务错误记录重试时间；同快照遵守该时间，新快照取消旧 queued/retry 再建立新任务。来源内部地址不作为文件名送入普通识别。

## 解析播放地址

用户从目录、队列或历史打开内部地址时，宿主只调用所属插件的 `pluginResolveUrl`，输入增加 `sourceItem` 和 `playbackHandle`。每轮可继续使用原有 `request`、`select` 协议，最终返回：

```js
return {
  type: 'play',
  url: 'https://media.example/video',
  headers: {Authorization: 'Bearer …'},
  expiresAt: Math.floor(Date.now() / 1000) + 300
};
```

- `url` 只接受有效 HTTP(S)，不带 userinfo、fragment 或控制字符，最长 16 KiB。
- `expiresAt` 可省略；提供时须为将来的 Unix 秒数。
- `headers` 可省略；最多 64 项、64 KiB，键值必须合法，不允许 Host、Content-Length、Connection 等报文控制头。非法请求头拒绝整个播放结果，不静默丢弃后继续播放。
- 原生 liberika 的 URL、续播点、请求头通过每次开流独有的一次性私有数据包传递；不使用按 URL 共用的全局凭据缓存。浏览器 liberika 不支持自定义播放请求头，宿主会明确拒绝非空 headers；此内核上需使用来源提供的签名 URL，不能宣称已有 header 认证能力。
- 历史、队列及公开媒体事件使用稳定内部地址；实际取流 URL 只用于播放 I/O。异步解析保存最初发起入口，WebUI 发起的播放不会因其间的桌面命令被改路由。
- 换文件、关闭、取消、来源被移除/重扫、绑定变化或权限撤销后，旧回包不能开流。失败或 null 不回退到“直接播放内部地址”。来源错误不展示原始插件错误中的临时 URL/token。

普通链接解析器也可返回已有目录引用，宿主按调用插件验证归属并重新走来源解析：

```js
return {type: 'play', sourceItem: {source, sourceItemId: 'movie-a', mediaSourceId: 'original'}};
```

来源解析本身不能再次转交来源，避免递归。插件不能直接在结果里注入任意 `mediaContext` 来建立绑定；`searchTitle` 仍只是普通链接的搜索提示。

## 异步网络与解析期限

- `pluginResolveUrl` 可以返回 Promise；宿主区分“等待 Promise”与真正的 `null`，不会在等待中回退播放。每轮最多等待 30 秒，最多 16 轮；等待用户选择版本时暂停计时。取消或换轮会释放截止计时器。
- `request` 轮次执行 GET，20 秒超时，响应正文最多 4 MiB；原生不跟随重定向，保留状态与 `location` 给插件处理。浏览器受 Fetch 限制，手动重定向不可读时明确失败，不偷偷改为自动跟随。
- `net.fetch` 需要 `network` 权限，单次最多 60 秒、正文最多 16 MiB，每个插件实例同时最多 16 个请求；在解析器内调用时仍受该解析轮的 30 秒期限约束。按既有契约跟随重定向，非 2xx 状态也作为响应返回。请求方法、地址、请求头和正文先校验；GET/HEAD 不带正文。
- 浏览器使用真正的异步 Fetch 和分块读取；超时或取消中止 Fetch，正文不依赖 Content-Length 限流。浏览器不附带环境 Cookie，不能设置的头明确报错；跨域、HTTPS/mixed-content 仍受浏览器安全规则约束，来源服务器需要正确配置 CORS。
- `request` 结果归属解析轮次；`net.fetch` 结果归属插件运行实例和请求号。关闭、换轮或重启后的旧结果不能推进新解析，也不能填入新实例复用的请求号。原生请求取消后不交付结果，读流逐块检查取消；正在等待的阻塞 I/O 最晚在请求期限内退出。
- 网络错误不附带原始 URL、认证头或响应正文。`request` 文本按既有契约有损解码 UTF-8；`net.fetch` 拒绝非 UTF-8 正文。

## 当前验收边界

SQLite 与浏览器存储使用相同契约测试；真实 JS 宿主覆盖扫描、Promise 确认、稳定来源播放、解析超时与实例重启隔离；原生 adapter 的一次性数据包和路由已做回归。核心 161 项测试通过，3 项原有网络测试忽略，wasm 编译通过。

隔离副本中的生产 HTTP/计时代码已在真实浏览器完成 18 项组件验收，覆盖跨域认证请求、重定向、流式限额、超时、取消、UTF-8、CORS 拒绝和计时器释放。浏览器插件的安装/加载链及整个 App 的五端开流、UI、网络联合验收仍未完成；组件验收不等于宿主契约全部交付。
