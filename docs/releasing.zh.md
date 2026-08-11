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
| OpenHarmony arm64 | `erika-capi-openharmony-arm64.zip`，包含 `liberika_capi.so` 和 `liberika_flutter.so` |

OpenHarmony 归档使用 OpenHarmony 5.1.0 Native SDK、compatible SDK 18 构建，
包含 C API runtime 和 Flutter N-API bridge。设置 `ERIKA_PREBUILT=1` 后，插件
CMake 会下载匹配 tag 的归档、链接预构建 runtime，并把它与本地链接的 N-API bridge
一起打入 HAR/HAP；下载失败时自动回退源码构建。

每个归档还包含 `include/erika.h`、`LICENSE`、`THIRD_PARTY_NOTICES.md`、依赖许可证和记录 tag/commit 的 `MANIFEST.txt`。原生依赖使用 `lgpl` profile 静态链接；Android 同时携带匹配 ABI 的 `libc++_shared.so`。

## 创建 Release

Release 由 [release.yml](../.github/workflows/release.yml) 自动执行。推送 `v*` tag 才会创建 GitHub Release：

```sh
git tag v0.1.6
git push origin v0.1.6
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

```sh
export ERIKA_PREBUILT=1
export ERIKA_PREBUILT_TAG=v0.1.6
```

建议始终显式固定 `ERIKA_PREBUILT_TAG`，保证插件源码和 C ABI 版本一致。下载或解压失败会回退源码构建；本地调试时设置：

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
ERIKA_PREBUILT=1 ERIKA_PREBUILT_TAG=v0.1.6 \
ERIKA_ANDROID_ABIS=arm64-v8a,x86_64 flutter build apk
```

更完整的源码构建和 target 对齐规则见 [building.zh.md](building.zh.md)。
