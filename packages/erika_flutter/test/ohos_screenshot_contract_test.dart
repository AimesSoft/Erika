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
}
