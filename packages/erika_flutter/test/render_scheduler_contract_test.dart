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

  test('macOS Flutter texture path stays on IOSurface and Metal', () {
    final plugin = File(
      'macos/Classes/ErikaFlutterPlugin.swift',
    ).readAsStringSync();

    expect(plugin, contains('private final class ErikaFlutterTextureSurface'));
    expect(plugin, contains('kCVPixelBufferIOSurfacePropertiesKey'));
    expect(plugin, contains('CVMetalTextureCacheCreateTextureFromImage'));
    expect(plugin, contains('library.setFlutterTextureBuffer('));
    expect(plugin, contains('registry.textureFrameAvailable(id)'));
    expect(plugin, isNot(contains('CVPixelBufferLockBaseAddress')));
  });

  test('macOS transparent platform view applies native overlay compositing',
      () {
    final plugin = File(
      'macos/Classes/ErikaFlutterPlugin.swift',
    ).readAsStringSync();

    expect(plugin, contains('metalLayer.isOpaque = !alphaVideo'));
    expect(plugin, contains('NSColor.clear.cgColor'));
    expect(
      plugin,
      contains('metalLayer.compositingFilter = "overlayBlendMode"'),
    );
    expect(plugin, contains('metalLayer.opacity = Float('));
  });

  test('Windows transparent video is composed into the Flutter HWND', () {
    final plugin = File(
      'windows/erika_flutter_plugin.cpp',
    ).readAsStringSync();
    final rendererFile = File('../../crates/erika/src/renderer/d3d11.rs');
    if (!rendererFile.existsSync()) {
      return;
    }
    final renderer = rendererFile.readAsStringSync();

    expect(plugin, contains('config.video_alpha_mode ='));
    expect(plugin, contains('capabilities.direct_composition = true'));
    expect(plugin, contains('DCompositionCreateDevice2'));
    expect(plugin, contains('CreateTargetForHwnd'));
    expect(plugin, contains('CreateBlendEffect'));
    expect(plugin, contains('D2D1_BLEND_MODE_OVERLAY'));
    expect(plugin, contains('IDCompositionEffectGroup::SetOpacity'));
    expect(plugin, contains('root_visual->SetEffect(effect)'));
    expect(
      plugin,
      contains('erika_presenter_windows_composition_swapchain_iunknown'),
    );
    expect(renderer, contains('DXGI_ALPHA_MODE_PREMULTIPLIED'));
    expect(
      renderer,
      contains('composition && self.video_alpha_mode.has_alpha()'),
    );
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

  test('iOS retries audio-session activation after an allowed interruption',
      () {
    final plugin = File(
      'ios/Classes/ErikaFlutterPlugin.swift',
    ).readAsStringSync();

    expect(plugin, contains('AVAudioSession.interruptionNotification'));
    expect(plugin, contains('try session.setActive(true)'));
    expect(plugin, contains('private func resumeInterruptedPlayback'));
    expect(plugin, contains('interruptionResumeWorkItem'));
    expect(plugin, contains('guard attempt < maxAttempts else'));
  });

  test('iOS drops a pending interruption resume on an explicit command', () {
    final plugin = File(
      'ios/Classes/ErikaFlutterPlugin.swift',
    ).readAsStringSync();

    expect(
      plugin,
      contains(
          'private func cancelPendingInterruptionResume(ifPlayer playerId: Int64)'),
    );

    // Every explicit pause/stop/close path must disarm the retry before it
    // fires, or the deferred resume would play against the user's intent.
    for (final entry in <(String, String)>[
      ('      case "pause":', 'try host.pause()'),
      ('      case "stop":', 'try host.stop()'),
      ('      case "close":', 'try host.close()'),
    ]) {
      final caseStart = plugin.indexOf(entry.$1);
      expect(caseStart, greaterThanOrEqualTo(0), reason: entry.$1);
      final body = plugin.substring(
        caseStart,
        plugin.indexOf(entry.$2, caseStart) + entry.$2.length,
      );
      expect(
          body, contains('cancelPendingInterruptionResume(ifPlayer: host.id)'));
    }

    final remoteStart = plugin.indexOf(
      'private func performRemotePause()',
    );
    final remoteEnd =
        plugin.indexOf('private func performRemoteToggle()', remoteStart);
    expect(remoteStart, greaterThanOrEqualTo(0));
    expect(remoteEnd, greaterThan(remoteStart));
    expect(
      plugin.substring(remoteStart, remoteEnd),
      contains('cancelPendingInterruptionResume(ifPlayer: host.id)'),
    );
  });

  test('Android polls events on the presenter thread with adaptive scheduling',
      () {
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
