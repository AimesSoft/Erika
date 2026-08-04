import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

void main() {
  test('OpenHarmony screenshot returns the composited RGBA frame', () {
    final plugin = File(
      'ohos/src/main/ets/components/plugin/ErikaFlutterPlugin.ets',
    ).readAsStringSync();
    final nativeBridge = File(
      'ohos/src/main/cpp/erika_flutter_plugin.cpp',
    ).readAsStringSync();

    expect(plugin, contains('this.captureFrame(args, result);'));
    expect(plugin, contains('erikaNative.nativeCaptureFrame('));
    expect(plugin, contains('as Uint8Array | null'));

    expect(nativeBridge, contains('napi_value NativeCaptureFrame('));
    expect(
      nativeBridge,
      contains('erika_presenter_capture_frame_rgba('),
    );
    expect(nativeBridge, contains('napi_create_arraybuffer('));
    expect(nativeBridge, contains('napi_create_typedarray('));
    expect(
      nativeBridge,
      contains('{"nativeCaptureFrame", nullptr, NativeCaptureFrame'),
    );
  });

  test('OpenHarmony exposes the complete subtitle memory font API', () {
    final plugin = File(
      'ohos/src/main/ets/components/plugin/ErikaFlutterPlugin.ets',
    ).readAsStringSync();
    final nativeBridge = File(
      'ohos/src/main/cpp/erika_flutter_plugin.cpp',
    ).readAsStringSync();
    final jsonBridge = File(
      '../../crates/erika_capi/src/presenter_json.rs',
    ).readAsStringSync();

    expect(plugin, contains("call.method === 'registerSubtitleMemoryFont'"));
    expect(plugin, contains('nativeRegisterSubtitleMemoryFont('));
    for (final method in <String>[
      'selectSubtitleMemoryFonts',
      'clearSubtitleMemoryFonts',
      'getSubtitleMemoryFontStatus',
    ]) {
      expect(plugin, contains("'$method'"));
      expect(jsonBridge, contains('"$method"'));
    }
    expect(
      nativeBridge,
      contains('napi_value NativeRegisterSubtitleMemoryFont('),
    );
    expect(
      nativeBridge,
      contains('erika_presenter_register_subtitle_memory_font('),
    );
    expect(
      nativeBridge,
      contains(
        '{"nativeRegisterSubtitleMemoryFont", nullptr, '
        'NativeRegisterSubtitleMemoryFont',
      ),
    );
  });

  test('OpenHarmony can consume and package the prebuilt runtime', () {
    final cmake = File(
      'ohos/src/main/cpp/CMakeLists.txt',
    ).readAsStringSync();

    expect(cmake, contains(r'$ENV{ERIKA_PREBUILT}'));
    expect(cmake, contains(r'$ENV{ERIKA_PREBUILT_TAG}'));
    expect(
      cmake,
      contains('erika-capi-openharmony-arm64.zip'),
    );
    expect(cmake, contains('ERIKA_USE_PREBUILT'));
    expect(cmake, contains('copy_if_different'));
    expect(
      cmake,
      contains(r'$<TARGET_FILE_DIR:${PLUGIN_NAME}>/liberika_capi.so'),
    );
  });
}
