# Changelog

## Unreleased

- Added `httpBackBufferBytes` to `ErikaOpenOptions`, forwarding the HTTP(S)
  rewind budget through the presenter JSON bridge.

## 0.1.9

- Published the ArkTS/OHPM package with the matching v0.1.9 native runtime.
- Added `videoAlphaMode` propagation through the ArkTS wrapper and N-API
  bridge while preserving opaque playback as the default.
- Synchronized the OpenHarmony wrapper header with the C API and validated
  HAR installation and consumer HAP builds.

## 0.1.8

- Updated the native runtime with audio-clock recovery after render stalls,
  smooth playback-rate transitions, audio-track startup fallback, and stable
  danmaku placement across planner windows.

- Added HTTP headers and per-open read-ahead tuning to `ErikaPlayer.open`.

## 0.1.7

- Initial ArkTS/OHPM package scaffold.
- Added native N-API presenter bridge for OpenHarmony arm64.
- Added surface attachment, playback control, render ticks, events, screenshots,
  and subtitle memory-font registration.
