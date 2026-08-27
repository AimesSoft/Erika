# 接入自编译 Erika 内核（HTTP 预读缓冲可调版）

分支 `feat/http-readahead-option`（fork: 1824239290/Erika）在 v0.1.7 之上新增
`erika_presenter_open_with_options`，支持逐请求覆盖 HTTP 预读窗口。本文档说明
如何取产物、替换 XXPlayer 的内核、以及在 App 侧设置链路。

## 1. 新 C API 摘要

```c
typedef struct ErikaOpenOptions {
  const ErikaHttpHeader *headers;   /* 可为 NULL */
  uintptr_t header_count;
  uint64_t http_read_ahead_bytes;   /* 0 = 内核默认 2 MiB */
  uint64_t reserved[3];             /* 必须全零 */
} ErikaOpenOptions;

ErikaStatus erika_presenter_open_with_options(
    ErikaPresenterHandle *handle, const char *uri,
    const ErikaOpenOptions *options);
```

- `http_read_ahead_bytes` 仅影响 HTTP(S) 源；本地文件忽略。
- 显式值优先于 `ERIKA_HTTP_READAHEAD_BYTES` 环境变量。
- 老符号 `erika_presenter_open_with_headers` 保留，行为不变。

## 2. 构建（GitHub Actions，无需本地工具链）

仓库 Actions 已启用。进入 fork → Actions → **Release** workflow → Run workflow
→ 分支选 `feat/http-readahead-option` → Run。

- 这是 dry-run 模式：构建所有平台产物并上传为 workflow artifacts，不发布 Release
  （publish job 只在 push tag 时启用）。
- 只需要 macOS + iOS 的话，等 run 结束后从 run 页面下载这两个 artifact：
  `erika-capi-macos-arm64`、`erika-capi-ios`（含 xcframework）。
- 全量跑约 40–60 分钟（Windows/Android/OHos 并行）。

## 3. 替换 XXPlayer 内核

```bash
# 下载 artifact 解压后（假设放在 /tmp/erika-build/）
cd /Users/jumusu/Documents/test/XXPlayer

# macOS slice + iOS xcframework 按 fetch-erika.sh 的目录布局放入:
#   Vendor/extracted/erika-capi-macos-arm64/...
#   Vendor/extracted/erika-capi-ios/...
# 然后手动合成（或直接替换现成产物）:
rm -rf Packages/ErikaKit/Vendor/Erika.xcframework
xcodebuild -create-xcframework \
  -library /tmp/erika-build/macos-arm64/lib/liberika_capi.a \
           -headers /tmp/erika-build/macos-arm64/include \
  -library /tmp/erika-build/ios/lib/erika_capi.xcframework/ios-arm64/liberika_capi.a \
           -headers /tmp/erika-build/ios/include \
  -library /tmp/erika-build/ios/lib/erika_capi.xcframework/ios-arm64_x86_64-simulator/liberika_capi-sim.a \
           -headers /tmp/erika-build/ios/include \
  -output Packages/ErikaKit/Vendor/Erika.xcframework

# 同步 C 头（fetch-erika.sh 每次都会比对）
cp /tmp/erika-build/ios/include/erika.h \
   Packages/ErikaKit/Sources/CErika/include/erika.h
```

注意：`fetch-erika.sh` 之后不能再直接跑 —— 它会从上游 Release 拉官方包覆盖本地产物。
要固定就用分支产物重打 tag，或暂时别跑它。

## 4. App 侧接入点（回 XXPlayer 时）

1. `PlaybackSource`（`Packages/PlaybackKit/.../PlaybackTypes.swift:73`）加
   `readAheadBytes: UInt64?`。
2. `ErikaPresenter.open` 组装 `ErikaOpenOptions`：headers 照旧，
   `http_read_ahead_bytes` 填设置值（0 表示默认）。
3. `PlaybackController.openPreparedRequest` 传值。
4. 设置页“网络预读缓冲”：默认 / 8 MiB / 16 MiB / 32 MiB（0 / 8388608 / 16777216 / 33554432）。

Swift 侧调用示例：

```swift
var options = ErikaOpenOptions(
    headers: headersPtr, headerCount: UInt(headers.count),
    httpReadAheadBytes: readAheadBytes)   // 0 = 默认
try withUnsafePointer(to: &options) { ptr in
    erika_presenter_open_with_options(presenter, uri, UnsafeRawPointer(ptr))
}
```

（Flutter 侧 `ErikaPlayer.open(uri, httpReadAheadBytes: ...)` 已同步支持，
Dart/Android JNI/OHos JSON 桥参数名为 `httpReadAheadBytes`。）
