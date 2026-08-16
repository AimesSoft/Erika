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
| tvOS | `erika-capi-tvos.zip`、device と arm64/x86_64 simulator の XCFramework slice |
| Android | `erika-capi-android.zip`、`arm64-v8a`、`armeabi-v7a`、`x86_64`、`x86` |
| Flutter Android | `erika-flutter-android-<abi>.zip`、1 archive あたり 1 ABI の shared runtime |
| OpenHarmony arm64 | `erika-capi-openharmony-arm64.zip`、`liberika_capi.so` と `liberika_flutter.so` |

OpenHarmony archive は OpenHarmony 5.1.0 Native SDK、compatible SDK 18 で
build され、C API runtime と Flutter N-API bridge を含みます。
Flutter plugin は package で固定された release を既定で download し、SHA-256 を
検証して prebuilt runtime を link し、N-API bridge とともに HAR/HAP に package
します。download または検証の失敗は明示的な error になり、source build は
`ERIKA_FORCE_SOURCE_BUILD=1` を設定した場合だけ有効になります。

各 archive には `include/erika.h`、`LICENSE`、`THIRD_PARTY_NOTICES.md`、dependency license、tag/commit を記録する `MANIFEST.txt` も含まれます。native dependency は `lgpl` profile で static link され、Android は ABI に対応する `libc++_shared.so` も含みます。

Flutter Android build は要求された ABI の
`erika-flutter-android-<abi>.zip` だけを download し、他 architecture や native
embedder 専用の static `.a` は download しません。combined
`erika-capi-android.zip` は multi-ABI / static link の C/C++ consumer 向けに維持します。

## Release の作成

Release は [release.yml](../.github/workflows/release.yml) で自動化されています。GitHub Release を作成するには `v*` tag を push します：

```sh
git tag v0.1.7
git push origin v0.1.7
```

`workflow_dispatch` の手動実行は Actions Artifact のみを生成し、`ERIKA_PREBUILT_TAG` から取得できる GitHub Release は公開しません。

macOS arm64 と x64 はどちらも `macos-26` で cross build し、その後 universal package を合成します。iOS と tvOS の XCFramework も `macos-26` を使用します。Windows x64 は `windows-latest`、ARM64 は `windows-11-arm` で native build します。

## Release 前の検証

`v*` tag を push する前に、clean worktree で次を実行して結果を記録してください。

```sh
cargo fmt --all -- --check
cargo test -p erika -p erika_capi
cargo test --workspace
cargo clippy -p erika -p erika_capi --all-targets -- -D warnings
```

さらに、影響を受ける example を compile し、公開 `erika.h` と C ABI reference / Flutter
FFI glue の整合性、各 archive の manifest と license、NipaPlay で固定した prebuilt tag を確認してから Release Notes を公開してください。

## Flutter で prebuilt を使用

plugin は package 内で固定された release tag と SHA-256 を既定で使用します。
`ERIKA_PREBUILT_TAG` を上書きする場合は、対応する `ERIKA_PREBUILT_SHA256` も必須です。
download、展開、検証の失敗は明示的な error になります。local source debug では次を設定します：

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
| tvOS | Xcode platform/arch に従う | 共通 tvOS XCFramework から slice を選択 |

Android の例：

```sh
ERIKA_ANDROID_ABIS=arm64-v8a,x86_64 flutter build apk
```

source build と target の一致ルールは [building.ja.md](building.ja.md) を参照してください。
