# Changelog

## Unreleased

## 0.1.8

- Fixed persistent audio/video drift after rendering stalls and overlapping
  queued audio during foreground recovery.
- Fixed Windows DWM crashes, frame reuse, video fill sizing, and DLL shutdown;
  native opaque playback retains HDR, with an explicit SDR Flutter texture path.
- Smoothed playback-rate transitions and improved iOS foreground/interruption
  recovery, audio-track startup fallback, and danmaku continuity.
- Updated native artifacts and the presenter ABI together; native integrations
  must use the matching headers and runtime.

- Added per-open HTTP read-ahead tuning through `httpReadAheadBytes` on
  Android, Apple platforms, Windows, and OpenHarmony.
- Added a packed-alpha video mode that stores color and alpha side by side,
  reconstructs premultiplied transparency in the GPU renderer, and propagates
  the mode through the C ABI and Flutter platform integrations.
- Added a macOS `ErikaTextureVideoView` backed by IOSurface and Metal for
  Flutter-composited opacity, clipping, transforms, and color filters without
  per-frame CPU pixel readback.
- Added native macOS opacity and overlay compositing for transparent video
  platform views when backdrop-aware blending is required.
- Added Windows DirectComposition presentation for transparent video, with a
  premultiplied-alpha swap chain attached directly to the Flutter HWND and
  native overlay blending/opacity instead of a covering popup window.
- Raised the native playback-rate ceiling to 16× for short-form video effects.

## 0.1.7

- Published `erika_flutter` as a standalone pub.dev package with package-local
  license, changelog, metadata, and runnable iOS and macOS examples.
- Made verified, version-pinned native bundles the default for Android, Apple
  platforms, Windows, and OpenHarmony.
- Split Flutter Android runtimes by ABI so app builds download only the selected
  architecture and omit native-embedder static libraries.
- Added explicit `ERIKA_FORCE_SOURCE_BUILD=1` source builds without silent
  fallback when a prebuilt download or checksum fails.
- Added isolated package and cross-platform consumer validation in GitHub
  Actions.

## 0.1.6

- Added the ArtCNN C4F16 DS denoising and sharpening upscaler.
- Added source-aware SDR and EDR output selection on Apple platforms.
- Moved Android and macOS presentation work off the application UI thread.
- Exposed renderer resource status on Android, Windows, and OpenHarmony.
- Restored Windows system media controls and tightened the OpenHarmony bridge.

See the [repository changelog](https://github.com/AimesSoft/Erika/blob/main/CHANGELOG.md)
for native engine and earlier release details.
