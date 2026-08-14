import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

void main() {
  test(
      'Android content pipes spool off the platform thread with bounded cleanup',
      () {
    final plugin = File(
      'android/src/main/kotlin/dev/aimesoft/erika_flutter/'
      'ErikaFlutterPlugin.kt',
    ).readAsStringSync();
    final source = File(
      'android/src/main/kotlin/dev/aimesoft/erika_flutter/'
      'AndroidContentSource.kt',
    ).readAsStringSync();
    final host = File(
      'android/src/main/kotlin/dev/aimesoft/erika_flutter/'
      'AndroidPlayerHost.kt',
    ).readAsStringSync();

    expect(plugin, contains('Executors.newFixedThreadPool'));
    expect(plugin, contains('mainHandler.post'));
    expect(plugin, contains('cancelContentPreparations("superseded_by_'));
    expect(plugin, contains('contentPreparationExecutor.shutdownNow()'));
    expect(plugin, contains('detachAssetFileDescriptor(openedAsset'));
    expect(plugin, contains('stage = "zero_copy"'));
    expect(
      plugin,
      isNot(contains('Keep the pipe drain in that ownership boundary for now')),
    );

    expect(source, contains('ANDROID_CONTENT_SPOOL_MAX_BYTES'));
    expect(source, contains('ANDROID_CONTENT_SPOOL_MIN_FREE_BYTES'));
    expect(source, contains('closeables.toList()'));
    expect(source, contains('temporaryFiles.toList()'));
    expect(source, contains('insufficient_disk_budget'));
    expect(source, contains('max_bytes_exceeded'));

    expect(plugin, contains('registerSubtitleMemoryFont'));
    expect(plugin, contains('selectSubtitleMemoryFonts'));
    expect(plugin, contains('clearSubtitleMemoryFonts'));
    expect(plugin, contains('getSubtitleMemoryFontStatus'));
    expect(plugin, contains('host.registerSubtitleMemoryFontAsync(data)'));
    expect(plugin, isNot(contains('host.registerSubtitleMemoryFont(data)')));
    expect(host, contains('nativeRegisterSubtitleMemoryFont'));
  });

  test('Android foreground return preserves the documented paused state', () {
    final plugin = File(
      'android/src/main/kotlin/dev/aimesoft/erika_flutter/'
      'ErikaFlutterPlugin.kt',
    ).readAsStringSync();
    final suspendStart = plugin.indexOf('private fun suspendForActivityStop()');
    final resumeStart = plugin.indexOf('private fun resumeFromActivityStop()');
    final resumePendingStart =
        plugin.indexOf('private fun resumePendingPlayback()');

    expect(suspendStart, greaterThanOrEqualTo(0));
    expect(resumeStart, greaterThan(suspendStart));
    expect(resumePendingStart, greaterThan(resumeStart));
    final suspendBody = plugin.substring(suspendStart, resumeStart);
    final resumeBody = plugin.substring(resumeStart, resumePendingStart);
    expect(suspendBody, contains('host.cancelPlaybackIntent()'));
    expect(suspendBody, isNot(contains('host.suspendPlayback()')));
    expect(suspendBody, contains('postBackgroundCommand(host, "lifecycle", "pause")'));
    expect(suspendBody, contains('view::suspendSurfaceAsync'));
    expect(suspendBody, isNot(contains('host.invoke("pause"')));
    expect(
      suspendBody.contains(RegExp(r'view::suspendSurface(?!Async)')),
      isFalse,
    );
    expect(resumeBody, contains('resumePendingPlayback()'));
    expect(resumeBody, isNot(contains('requestPlayback()')));
    expect(resumeBody, isNot(contains('startPendingPlayback(')));
  });

  test('Android presenter lifecycle and Surface results stay asynchronous', () {
    final plugin = File(
      'android/src/main/kotlin/dev/aimesoft/erika_flutter/'
      'ErikaFlutterPlugin.kt',
    ).readAsStringSync();
    final view = File(
      'android/src/main/kotlin/dev/aimesoft/erika_flutter/'
      'ErikaTextureView.kt',
    ).readAsStringSync();
    final presenter = File(
      'android/src/main/kotlin/dev/aimesoft/erika_flutter/'
      'AndroidPresenterThread.kt',
    ).readAsStringSync();

    expect(plugin, contains('presenterCreates.registerIfCurrent'));
    expect(plugin, contains('presenterCreates.detach(retiringPresenterThread)'));
    expect(plugin, contains('view.bindAsync(host)'));
    expect(plugin, contains('view.unbindAsync(host)'));
    expect(plugin, contains('host.prepareForOpen('));
    final invokePlayerStart = plugin.indexOf('private fun invokePlayer(');
    final registerFontStart =
        plugin.indexOf('private fun registerSubtitleMemoryFont(');
    expect(invokePlayerStart, greaterThanOrEqualTo(0));
    expect(registerFontStart, greaterThan(invokePlayerStart));
    final invokePlayerBody =
        plugin.substring(invokePlayerStart, registerFontStart);
    expect(invokePlayerBody, isNot(contains('mediaSession.update(')));
    expect(plugin, isNot(contains('complete(result, view.bind(host))')));
    expect(plugin, isNot(contains('complete(result, view.unbind(host))')));
    expect(
      plugin,
      contains('reportRenderResponse(host, outcome.contentGeneration'),
    );
    expect(
      plugin,
      contains('val contentGeneration = host.latestExecutedContentGeneration'),
    );
    expect(
      plugin,
      contains('AndroidPendingEvent.Success(event, contentGeneration)'),
    );
    expect(
      view,
      contains('completeBindCompletions(host, response, bindingGeneration)'),
    );
    expect(
      view,
      contains('completeUnbindCompletions(host, NativeResponse.success())'),
    );
    expect(presenter, contains('androidPresenterTaskResult(block)'));
  });

  test('Android prebuilt tag follows the Flutter package version', () {
    final pubspec = File('pubspec.yaml').readAsStringSync();
    final gradle = File('android/erika-native.gradle').readAsStringSync();
    final version = RegExp(r'^version:\s*(\S+)', multiLine: true)
        .firstMatch(pubspec)!
        .group(1)!;

    expect(gradle, contains('?: "v$version"'));
  });
}
