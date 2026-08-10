import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

void main() {
  for (final entry in <(String, String)>[
    ('ios', 'scheduleTick()'),
    ('tvos', 'scheduleRenderTick()'),
  ]) {
    final platform = entry.$1;
    final schedulerCall = entry.$2;

    test('$platform keeps presenter rendering off the main run loop', () {
      final plugin = File(
        '$platform/Classes/ErikaFlutterPlugin.swift',
      ).readAsStringSync();

      expect(plugin, contains('private let renderQueue: DispatchQueue'));
      expect(
          plugin, contains('private let nativeCallLock = NSRecursiveLock()'));
      expect(plugin, contains('renderQueue.async'));
      expect(plugin, contains('self?.$schedulerCall'));
      expect(plugin, contains('mainThread=\\(Thread.isMainThread)'));
      expect(plugin, contains('Timer(timeInterval: 0.05'));
      expect(plugin, isNot(contains('self?.renderTick(sendEvent:')));

      final renderStart = plugin.indexOf('  func renderTick() {');
      final pollStart = plugin.indexOf(
        '  func pollEvents(',
        renderStart,
      );
      expect(renderStart, greaterThanOrEqualTo(0));
      expect(pollStart, greaterThan(renderStart));
      expect(
        plugin.substring(renderStart, pollStart),
        isNot(contains('pollEvents(')),
      );
    });
  }

  test('Android decouples event polling without violating JNI affinity', () {
    final plugin = File(
      'android/src/main/kotlin/dev/aimesoft/erika_flutter/'
      'ErikaFlutterPlugin.kt',
    ).readAsStringSync();

    expect(plugin, contains('private val eventPollRunnable'));
    expect(plugin, contains('EVENT_POLL_INTERVAL_MS = 50L'));
    expect(plugin, contains('moving only this call returns wrong_thread'));

    final frameStart = plugin.indexOf(
      'private val frameCallback = Choreographer.FrameCallback',
    );
    final attachStart = plugin.indexOf(
      'override fun onAttachedToEngine',
      frameStart,
    );
    expect(frameStart, greaterThanOrEqualTo(0));
    expect(attachStart, greaterThan(frameStart));
    expect(
      plugin.substring(frameStart, attachStart),
      isNot(contains('drainEvents')),
    );
  });
}
