# Erika のリリース

> 翻訳：[English](releasing.md) · [中文](releasing.zh.md)

この文書は、依存 project が FFmpeg と Erika を source から build せずに利用できる prebuilt `erika_capi` の公開方法を説明します。

## Release artifact

| Platform | Archive |
|----------|---------|
| macOS arm64 | `erika-capi-macos-arm64.zip` |
| macOS x64 | `erika-capi-macos-x64.zip` |
| macOS universal | `erika-capi-macos-universal.zip` |
| Windows x64 | `erika-capi-windows-x64.zip` |
| Windows ARM64 | `erika-capi-windows-arm64.zip` |
| iOS | `erika-capi-ios.zip`、device と simulator の XCFramework slice |
| Android | `erika-capi-android.zip`、`arm64-v8a`、`armeabi-v7a`、`x86_64`、`x86` |
| OpenHarmony arm64 | `erika-capi-openharmony-arm64.zip`、`liberika_capi.so` と `liberika_flutter.so` |

OpenHarmony archive は OpenHarmony 5.1.0 Native SDK、compatible SDK 18 で
build され、C API runtime と Flutter N-API bridge を含みます。Hvigor の
`ERIKA_PREBUILT` 自動 download はまだ実装されていません。

各 archive には `include/erika.h`、`LICENSE`、`THIRD_PARTY_NOTICES.md`、dependency license、tag/commit を記録する `MANIFEST.txt` も含まれます。native dependency は `lgpl` profile で static link され、Android は ABI に対応する `libc++_shared.so` も含みます。

## Release の作成

Release は [release.yml](../.github/workflows/release.yml) で自動化されています。GitHub Release を作成するには `v*` tag を push します：

```sh
git tag v0.1.4
git push origin v0.1.4
```

`workflow_dispatch` の手動実行は Actions Artifact のみを生成し、`ERIKA_PREBUILT_TAG` から取得できる GitHub Release は公開しません。

macOS arm64 は `macos-15`、x64 は `macos-15-intel` で native build し、その後 universal package を合成します。Windows x64 は `windows-latest`、ARM64 は `windows-11-arm` で native build します。

## Flutter で prebuilt を使用

```sh
export ERIKA_PREBUILT=1
export ERIKA_PREBUILT_TAG=v0.1.4
```

plugin source と C ABI の version を一致させるため、`ERIKA_PREBUILT_TAG` を明示的に固定することを推奨します。download または展開に失敗した場合は source build に fallback します。local debug では次を設定します：

```sh
export ERIKA_FORCE_SOURCE_BUILD=1
```

Platform architecture の選択：

| Platform | 設定 | 選択される package |
|----------|------|--------------------|
| macOS | `ERIKA_MACOS_ARCHS=arm64` | `macos-arm64` |
| macOS | `ERIKA_MACOS_ARCHS=x86_64` | `macos-x64` |
| macOS | `ERIKA_MACOS_ARCHS=universal` | `macos-universal` |
| Windows | `ERIKA_WINDOWS_ARCH=x64` | `windows-x64` |
| Windows | `ERIKA_WINDOWS_ARCH=arm64` | `windows-arm64` |
| Android | `ERIKA_ANDROID_ABIS=<list>` | 共通 Android package から ABI を選択 |
| iOS | Xcode platform/arch に従う | 共通 iOS XCFramework から slice を選択 |

Android の例：

```sh
ERIKA_PREBUILT=1 ERIKA_PREBUILT_TAG=v0.1.4 \
ERIKA_ANDROID_ABIS=arm64-v8a,x86_64 flutter build apk
```

source build と target の一致ルールは [building.ja.md](building.ja.md) を参照してください。
