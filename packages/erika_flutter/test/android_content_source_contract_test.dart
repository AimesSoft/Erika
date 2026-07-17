import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

void main() {
  test('Android content pipes spool off the platform thread with bounded cleanup', () {
    final plugin = File(
      'android/src/main/kotlin/dev/aimesoft/erika_flutter/'
      'ErikaFlutterPlugin.kt',
    ).readAsStringSync();
    final source = File(
      'android/src/main/kotlin/dev/aimesoft/erika_flutter/'
      'AndroidContentSource.kt',
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
  });
}
