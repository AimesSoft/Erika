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
| Android | `erika-capi-android.zip`，包含 `arm64-v8a`、`armeabi-v7a`、`x86_64`、`x86` |

每个归档还包含 `include/erika.h`、`LICENSE`、`THIRD_PARTY_NOTICES.md`、依赖许可证和记录 tag/commit 的 `MANIFEST.txt`。原生依赖使用 `lgpl` profile 静态链接；Android 同时携带匹配 ABI 的 `libc++_shared.so`。

## 创建 Release

Release 由 [release.yml](../.github/workflows/release.yml) 自动执行。推送 `v*` tag 才会创建 GitHub Release：

```sh
git tag v0.1.3
git push origin v0.1.3
```

手动运行 `workflow_dispatch` 只生成 Actions Artifact，不发布可供 `ERIKA_PREBUILT_TAG` 下载的 GitHub Release。

macOS arm64 在 `macos-15` 原生构建，x64 在 `macos-15-intel` 原生构建，然后合并 universal 包。Windows x64 在 `windows-latest` 构建，ARM64 在 `windows-11-arm` 原生构建。

## Flutter 使用预构建包

```sh
export ERIKA_PREBUILT=1
export ERIKA_PREBUILT_TAG=v0.1.3
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

Android 示例：

```sh
ERIKA_PREBUILT=1 ERIKA_PREBUILT_TAG=v0.1.3 \
ERIKA_ANDROID_ABIS=arm64-v8a,x86_64 flutter build apk
```

更完整的源码构建和 target 对齐规则见 [building.zh.md](building.zh.md)。
