# 新番与评论公开缓存

独立的 Python 3 标准库服务，监听 `127.0.0.1:18082`。Nginx 将
`/dandanplay/public/` 转发给它；原有弹弹网关的账号鉴权继续用于其他接口。
签名密钥仅从服务器的 `/etc/nipaplay-dandanplay-gateway.env` 读取。

每五分钟检查一次缓存日期，按北京时间每日首次检查更新新番列表、番剧详情与
评论。每部番的评论只请求弹弹 `/api/v2/bangumi/{id}/comments?page=0` 一次，
按更新时间倒序、去重，最多保留 **20 条**。这是截至刷新时的最新评论，
不会过滤掉前一天发表的评论。查看过的非当季番剧也会纳入刷新。

SQLite 保存快照与更新时间，图片保存为独立文件。刷新失败保留旧数据，五分钟后
重试；缓存读取不等待后台刷新。并发首次请求合并，同一天成功后不会再次拉取。
图片优先使用 Bangumi `image?type=large`，已有高清图在源站不可达时仍会保留。
首次无法取得高清图时缓存弹弹海报，并通过详情的 `imageQuality` 标记质量。

## 接口

| GET 路径（相对 `/dandanplay/public`） | 返回 |
| --- | --- |
| `/season` | 新番列表，包含缓存简介、海报地址 |
| `/details/{animeId}` | 番剧详情、剧集、正确的 `bangumiUrl` |
| `/comments/{animeId}` | 当日的弹弹评论快照，最多 20 条 |
| `/images/{hash}.jpg` | 服务器持有的图片 |
| `/live/comments/{animeId}` | 客户端发起的即时弹弹评论请求，供仅登录 Bangumi 的回退使用 |
| `/healthz` | 服务状态与新番缓存日期 |

即时接口每 IP 每分钟 10 次、全局每分钟 60 次；冷缓存请求也使用这一限制。
只接受固定的番剧编号与 GET 操作，不接受任意上游地址。JSON 快照携带
`cachedAt`、`cacheDate`，过期副本携带 `stale: true`。即时响应不进入公共缓存。

## 部署

1. 将 `cache.py`、`images.py`、`warm_posters.py` 放到 `/opt/erika/anime-cache/`。
2. 安装 `deploy/erika-anime-cache.service` 到 `/etc/systemd/system/`。
3. 将 `deploy/nginx-location.conf` 加入现有 Nginx server 块。
4. `sudo nginx -t` 后执行 `sudo systemctl daemon-reload`、
   `sudo systemctl enable --now erika-anime-cache` 和 `sudo systemctl reload nginx`。

数据目录由 systemd 的 `StateDirectory` 管理，路径为
`/var/lib/erika-anime-cache`，所有者 `nipaplay-api`。
如服务器不能直达 Bangumi，可在 systemd override 中配置
`ANIME_IMAGE_PROXY=http://<代理地址>`，重启服务后生效；代理只用于图片请求。
不要将含凭据的代理地址提交到仓库。

### 本机 Xray 图片代理

1. 从 [XTLS/Xray-core](https://github.com/XTLS/Xray-core/releases) 下载对应架构的
   官方包并校验 SHA256；本次部署使用 `v26.10.21` 的 Linux amd64 包。
   将二进制安装到 `/opt/erika/xray/xray`（root 所有，0755）。
2. 创建无登录权限的系统用户 `xray-cache`。在服务器的
   `/etc/erika-xray/config.json` 中按 `deploy/xray.example.json` 填入节点的
   地址、端口、UUID 和 SNI；目录权限 0750，文件 0640，所有者 `root:xray-cache`。
   示例中的 `alpn: ["http/1.1"]` 适用于本次节点，避免 TLS 协商进入节点的 HTTP 回落。
3. 安装 `deploy/erika-xray.service` 到 `/etc/systemd/system/`，并将
   `deploy/images.conf` 安装到
   `/etc/systemd/system/erika-anime-cache.service.d/images.conf`。
4. 执行 `sudo -u xray-cache /opt/erika/xray/xray run -test -config /etc/erika-xray/config.json`，
   成功后 `sudo systemctl daemon-reload`、`sudo systemctl enable --now erika-xray`，
   再重启 `erika-anime-cache`。

代理只监听 `127.0.0.1:18083`。缓存服务通过它访问 Bangumi 图片及 HTTPS DNS，
按 DNS 返回的地址连接图片源，并保留原域名的 TLS SNI 和证书校验。
Google DNS 不可用时尝试备用解析器；图片源的多个地址逐一尝试。
缺失高清图每五分钟重试，不重复拉取当日的弹弹元数据和评论。
上游尚未提供 Bangumi 对应条目的番剧使用已缓存的弹弹封面。

客户端会以最多四路后台下载把公开缓存封面保存到本机缓存，再交给原生图片组件。
这让 Apple 平台能显示现有 HTTP 网关的封面，并能在后续离线启动时复用图片；
下载仅接受已知网关下的 64 位哈希图片路径，不阻塞新番列表或评论的加载。

`warm_posters.py --output <目录>` 可从能访问图片源的机器下载首批高清图；
把该目录上传到服务器后，用服务用户运行
`warm_posters.py --import-dir <目录>` 导入。它读取公开元数据，不需要账号或密钥。
已有高清图随后由每日缓存继续使用；新海报仍需要服务器能够访问图片源。

查看运行状态：`systemctl status erika-anime-cache`；日志：
`journalctl -u erika-anime-cache`。回滚时移除新增的 Nginx location，检查并 reload，
再停止该服务；不会改变原有网关的二进制和配置。

## 验证

服务测试：在本目录运行 `python3 -m unittest -v`。
客户端公开接口实测：在仓库根目录运行
`cargo run -p erika-anime-core --example public_cache_smoke`。
该示例使用独立缓存命名空间，不读取用户账号，模拟 Bangumi 偏好以验证来源切换。
