import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

void main() {
  test('macOS renders on CVDisplayLink without a per-frame GCD hop', () {
    final plugin = File(
      'macos/Classes/ErikaFlutterPlugin.swift',
    ).readAsStringSync();

    final driverStart = plugin.indexOf(
      'private final class ErikaDisplayLinkDriver',
    );
    final firstStruct = plugin.indexOf(
      'private struct ErikaVideoParamsC',
      driverStart,
    );
    expect(driverStart, greaterThanOrEqualTo(0));
    expect(firstStruct, greaterThan(driverStart));
    final driver = plugin.substring(driverStart, firstStruct);
    expect(driver, contains('CVDisplayLinkSetOutputCallback'));
    expect(driver, contains('onTick()'));
    expect(driver, isNot(contains('renderQueue.async')));
    expect(driver, isNot(contains('DispatchQueue.main.async')));
  });

  test('macOS event polling is coalescible and stops with the last player', () {
    final plugin = File(
      'macos/Classes/ErikaFlutterPlugin.swift',
    ).readAsStringSync();

    expect(plugin, contains('timer.tolerance = 0.01'));
    expect(plugin, contains('private func stopPollTimerIfIdle()'));
    expect(plugin, contains('players.removeValue(forKey: playerId)'));
    expect(plugin, contains('stopPollTimerIfIdle()'));
  });

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

  test('iOS retries audio-session activation after an allowed interruption', () {
    final plugin = File(
      'ios/Classes/ErikaFlutterPlugin.swift',
    ).readAsStringSync();

    expect(plugin, contains('AVAudioSession.interruptionNotification'));
    expect(plugin, contains('try session.setActive(true)'));
    expect(plugin, contains('private func resumeInterruptedPlayback'));
    expect(plugin, contains('interruptionResumeWorkItem'));
    expect(plugin, contains('guard attempt < maxAttempts else'));
  });

  test('Android polls events on the presenter thread with adaptive scheduling', () {
    final plugin = File(
      'android/src/main/kotlin/dev/aimesoft/erika_flutter/'
      'ErikaFlutterPlugin.kt',
    ).readAsStringSync();

    expect(plugin, contains('private val eventPollRunnable'));
    expect(plugin, contains('private fun scheduleEventPoll()'));
    expect(plugin, contains('presenterThread.post {'));
    expect(plugin, contains('androidEventPollDelayMillis('));

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
