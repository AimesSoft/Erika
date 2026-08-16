# 发布 Erika

> 翻译：[English](releasing.md) · [日本語](releasing.ja.md)

本文说明如何发布预构建 `erika_capi`，让依赖项目无需从源码编译 FFmpeg 和 Erika。

## 发布产物

| 平台 | 归档 |
|------|------|
| macOS arm64 | `erika-capi-macos-arm64.zip` |
| macOS x64 | `erika-capi-macos-x64.zip` |
| macOS universal | `erika-capi-macos-universal.zip` |
| Windows x64 | `erika-capi-windows-x64.zip` |
| Windows ARM64 | `erika-capi-windows-arm64.zip` |
| iOS | `erika-capi-ios.zip`，包含 device 和 simulator XCFramework slice |
| tvOS | `erika-capi-tvos.zip`，包含 device 和 arm64/x86_64 simulator XCFramework slice |
| Android | `erika-capi-android.zip`，包含 `arm64-v8a`、`armeabi-v7a`、`x86_64`、`x86` |
| Flutter Android | `erika-flutter-android-<abi>.zip`，每个归档只包含一个 ABI 的 shared runtime |
| OpenHarmony arm64 | `erika-capi-openharmony-arm64.zip`，包含 `liberika_capi.so` 和 `liberika_flutter.so` |

OpenHarmony 归档使用 OpenHarmony 5.1.0 Native SDK、compatible SDK 18 构建，
包含 C API runtime 和 Flutter N-API bridge。Flutter 插件默认下载 package 固定的
release、校验 SHA-256、链接预构建 runtime，并把它与本地链接的 N-API bridge
一起打入 HAR/HAP。下载或校验失败会明确报错；只有设置
`ERIKA_FORCE_SOURCE_BUILD=1` 才会从源码构建。

每个归档还包含 `include/erika.h`、`LICENSE`、`THIRD_PARTY_NOTICES.md`、依赖许可证和记录 tag/commit 的 `MANIFEST.txt`。原生依赖使用 `lgpl` profile 静态链接；Android 同时携带匹配 ABI 的 `libc++_shared.so`。

Flutter Android 构建按实际请求的 ABI 下载对应
`erika-flutter-android-<abi>.zip`，不会下载其他架构或仅供原生嵌入使用的静态 `.a`。
合并的 `erika-capi-android.zip` 继续提供给需要多 ABI 或静态链接的 C/C++ 使用者。

## 发布 Flutter package

`erika_flutter` 独立发布在 [pub.dev](https://pub.dev/packages/erika_flutter)。
`0.1.7` 是首个 standalone package release，支持 macOS、iOS、tvOS、Windows、Android
和 HarmonyOS/OpenHarmony。package archive 包含插件源码、package `LICENSE`、README、
example 和固定 native artifact manifest；平台构建会下载匹配的 GitHub Release 归档并校验 SHA-256。

在干净工作树中从 package 目录执行：

```sh
cd packages/erika_flutter
dart pub publish --dry-run
dart pub publish
```

合并前由 [flutter-package.yml](../.github/workflows/flutter-package.yml) 执行 isolated
package 和各平台 consumer 检查。Linux 和 Web 目前还不是 package 发布目标。

### pub.dev publisher 身份

`unverified uploader` 表示 package 由未关联已验证 pub.dev publisher 的账号上传。
这不表示 package 校验、License 或构建失败。要显示已验证 publisher，需要创建或加入
自己控制域名对应的 pub.dev publisher，完成域名验证后，再把 package ownership 转移给该 publisher。

## 创建 Release

Release 由 [release.yml](../.github/workflows/release.yml) 自动执行。推送 `v*` tag 才会创建 GitHub Release：

```sh
VERSION=0.1.8
git tag "v${VERSION}"
git push origin "v${VERSION}"
```

手动运行 `workflow_dispatch` 只生成 Actions Artifact，不发布可供 `ERIKA_PREBUILT_TAG` 下载的 GitHub Release。

macOS arm64 和 x64 都在 `macos-26` 交叉构建，然后合并 universal 包；iOS 和 tvOS XCFramework 同样使用 `macos-26`。Windows x64 在 `windows-latest` 构建，ARM64 在 `windows-11-arm` 原生构建。

## 发布前检查

在推送 `v*` tag 前，维护者应在干净工作树中完成并记录：

```sh
cargo fmt --all -- --check
cargo test -p erika -p erika_capi
cargo test --workspace
cargo clippy -p erika -p erika_capi --all-targets -- -D warnings
```

此外还应：

- 编译受影响的平台示例，避免示例继续引用已删除的 C ABI、uniform 或配置字段；
- 对比 `crates/erika_capi/include/erika.h`，同步 C ABI 参考文档和 Flutter FFI glue；
- 检查每个预编译归档的 `MANIFEST.txt`、LICENSE、第三方声明及目标架构；
- 更新 package README、CHANGELOG 和 [平台能力矩阵](platform_matrix.zh.md)；
- 在 NipaPlay 中验证固定的 `ERIKA_PREBUILT_TAG`，并同步 NipaPlay 的 pin 与 Release Notes。

## Flutter 使用预构建包

插件默认使用 package 内固定的 release tag 和 SHA-256。覆盖
`ERIKA_PREBUILT_TAG` 时必须同时提供匹配的 `ERIKA_PREBUILT_SHA256`，保证插件源码和
C ABI 版本一致。Android 多 ABI 构建需按 ABI 提供
`ERIKA_PREBUILT_SHA256_ARM64_V8A`、`ERIKA_PREBUILT_SHA256_ARMEABI_V7A`、
`ERIKA_PREBUILT_SHA256_X86_64` 和 `ERIKA_PREBUILT_SHA256_X86`。下载、解压或校验失败会明确报错；本地源码调试时设置：

```sh
export ERIKA_FORCE_SOURCE_BUILD=1
```

平台架构选择：

| 平台 | 配置 | 选择的包 |
|------|------|----------|
| macOS | `ERIKA_MACOS_ARCHS=arm64` | `macos-arm64` |
| macOS | `ERIKA_MACOS_ARCHS=x86_64` | `macos-x64` |
| macOS | `ERIKA_MACOS_ARCHS=universal` | `macos-universal` |
| Windows | `ERIKA_WINDOWS_ARCH=x64` | `windows-x64` |
| Windows | `ERIKA_WINDOWS_ARCH=arm64` | `windows-arm64` |
| Android | `ERIKA_ANDROID_ABIS=<列表>` | 从统一 Android 包抽取所选 ABI |
| iOS | 由 Xcode platform/arch 决定 | 从统一 iOS XCFramework 选择 slice |
| tvOS | 由 Xcode platform/arch 决定 | 从统一 tvOS XCFramework 选择 slice |

Android 示例：

```sh
ERIKA_ANDROID_ABIS=arm64-v8a,x86_64 flutter build apk
```

更完整的源码构建和 target 对齐规则见 [building.zh.md](building.zh.md)。
