import 'dart:typed_data';

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:erika_flutter/erika_flutter.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  const playerChannel = MethodChannel('erika_flutter/player');
  const eventsChannel = MethodChannel('erika_flutter/events');

  late List<MethodCall> playerCalls;

  setUp(() {
    playerCalls = <MethodCall>[];
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(playerChannel, (MethodCall call) async {
      playerCalls.add(call);
      return switch (call.method) {
        'create' => 7,
        'dispose' => null,
        _ => null,
      };
    });
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(eventsChannel, (MethodCall call) async {
      return null;
    });
  });

  tearDown(() {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(playerChannel, null);
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(eventsChannel, null);
  });

  test('default player lets native choose output mode', () async {
    final player = ErikaPlayer();

    expect(await player.ensureCreated(), 7);

    final createCall = playerCalls.singleWhere(
      (MethodCall call) => call.method == 'create',
    );
    expect(createCall.arguments, isA<Map<Object?, Object?>>());
    expect(createCall.arguments as Map<Object?, Object?>, isEmpty);

    await player.dispose();
  });

  test('apple EDR output mode is passed to native create', () async {
    final player = ErikaPlayer(
      outputMode: ErikaOutputMode.appleEdr,
      edrHeadroom: 4.0,
    );

    expect(await player.ensureCreated(), 7);

    final createCall = playerCalls.singleWhere(
      (MethodCall call) => call.method == 'create',
    );
    final arguments = createCall.arguments as Map<Object?, Object?>;
    expect(arguments['outputMode'], ErikaOutputMode.appleEdr.nativeValue);
    expect(arguments['edrHeadroom'], 4.0);

    await player.dispose();
  });

  test('HDR debug flag is passed to native create when enabled', () async {
    final player = ErikaPlayer(hdrDebug: true);

    expect(await player.ensureCreated(), 7);

    final createCall = playerCalls.singleWhere(
      (MethodCall call) => call.method == 'create',
    );
    final arguments = createCall.arguments as Map<Object?, Object?>;
    expect(arguments['hdrDebug'], true);

    await player.dispose();
  });

  test('external subtitle add returns native track id', () async {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(playerChannel, (MethodCall call) async {
      playerCalls.add(call);
      return switch (call.method) {
        'create' => 7,
        'addExternalSubtitle' => 1000001,
        'dispose' => null,
        _ => null,
      };
    });
    final player = ErikaPlayer();

    final trackId = await player.addExternalSubtitle('/tmp/subs.srt');

    expect(trackId, 1000001);
    final call = playerCalls.singleWhere(
      (MethodCall call) => call.method == 'addExternalSubtitle',
    );
    expect(call.arguments, <String, Object?>{
      'playerId': 7,
      'uri': '/tmp/subs.srt',
    });

    await player.dispose();
  });

  test('external subtitle remove forwards track id', () async {
    final player = ErikaPlayer();

    await player.removeSubtitleTrack(1000001);

    final call = playerCalls.singleWhere(
      (MethodCall call) => call.method == 'removeSubtitleTrack',
    );
    expect(call.arguments, <String, Object?>{
      'playerId': 7,
      'trackId': 1000001,
    });

    await player.dispose();
  });

  test('screenshot forwards optional capture size', () async {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(playerChannel, (MethodCall call) async {
      playerCalls.add(call);
      return switch (call.method) {
        'create' => 7,
        'screenshot' => Uint8List.fromList(<int>[1, 2, 3, 4]),
        'dispose' => null,
        _ => null,
      };
    });
    final player = ErikaPlayer();

    final bytes = await player.screenshot(width: 320, height: 180);

    expect(bytes, <int>[1, 2, 3, 4]);
    final call = playerCalls.singleWhere(
      (MethodCall call) => call.method == 'screenshot',
    );
    expect(call.arguments, <String, Object?>{
      'playerId': 7,
      'width': 320,
      'height': 180,
    });

    await player.dispose();
  });

  test('track selection methods forward nullable track ids', () async {
    final player = ErikaPlayer();

    await player.selectAudioTrack(2);
    await player.selectSubtitleTrack(null);

    final audioCall = playerCalls.singleWhere(
      (MethodCall call) => call.method == 'selectAudioTrack',
    );
    expect(audioCall.arguments, <String, Object?>{
      'playerId': 7,
      'trackId': 2,
    });

    final subtitleCall = playerCalls.singleWhere(
      (MethodCall call) => call.method == 'selectSubtitleTrack',
    );
    expect(subtitleCall.arguments, <String, Object?>{
      'playerId': 7,
      'trackId': null,
    });

    await player.dispose();
  });

  test('playback rate is forwarded to native player clock', () async {
    final player = ErikaPlayer();

    await player.setPlaybackRate(1.5);

    final call = playerCalls.singleWhere(
      (MethodCall call) => call.method == 'setPlaybackRate',
    );
    expect(call.arguments, <String, Object?>{
      'playerId': 7,
      'rate': 1.5,
    });

    await player.dispose();
  });

  test('window overlay methods forward surface geometry', () async {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(playerChannel, (MethodCall call) async {
      playerCalls.add(call);
      return switch (call.method) {
        'create' => 7,
        'attachOverlay' => ErikaPlayer.windowOverlayViewId,
        'dispose' => null,
        _ => null,
      };
    });
    final player = ErikaPlayer();

    final viewId = await player.attachWindowOverlay();
    await player.setWindowOverlayFrame(
      frame: const Rect.fromLTWH(10, 20, 320, 180),
      visible: true,
      generation: 42,
      debugLabel: 'episode.mkv',
    );
    await player.detachWindowOverlay(generation: 42);

    expect(viewId, ErikaPlayer.windowOverlayViewId);
    expect(
      playerCalls
          .singleWhere((MethodCall call) => call.method == 'attachOverlay')
          .arguments,
      <String, Object?>{'playerId': 7},
    );
    expect(
      playerCalls
          .singleWhere((MethodCall call) => call.method == 'setOverlayFrame')
          .arguments,
      <String, Object?>{
        'viewId': ErikaPlayer.windowOverlayViewId,
        'generation': 42,
        'x': 10.0,
        'y': 20.0,
        'width': 320.0,
        'height': 180.0,
        'visible': true,
        'debugLabel': 'episode.mkv',
      },
    );
    expect(
      playerCalls
          .singleWhere((MethodCall call) => call.method == 'detachOverlay')
          .arguments,
      <String, Object?>{'playerId': 7, 'generation': 42},
    );

    await player.dispose();
  });

  test('upscaler mode is forwarded to native presenter', () async {
    final player = ErikaPlayer();

    await player.setUpscaler(ErikaUpscalerMode.artCnnC4F16);

    final call = playerCalls.singleWhere(
      (MethodCall call) => call.method == 'setUpscaler',
    );
    expect(call.arguments, <String, Object?>{
      'playerId': 7,
      'mode': ErikaUpscalerMode.artCnnC4F16.nativeValue,
    });

    await player.dispose();
  });

  test('upscaler status is decoded from native presenter', () async {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(playerChannel, (MethodCall call) async {
      playerCalls.add(call);
      return switch (call.method) {
        'create' => 7,
        'getUpscalerStatus' => <String, Object?>{
            'requestedMode': ErikaUpscalerMode.artCnnC4F32.nativeValue,
            'activeBackend':
                ErikaUpscalerBackendStatus.simdgroupMatrix.nativeValue,
            'fallbackCount': 1,
            'upscaledFrames': 42,
            'lastEncodeMicros': 1200,
            'lastGpuMicros': 3400,
          },
        'dispose' => null,
        _ => null,
      };
    });

    final player = ErikaPlayer();

    final status = await player.getUpscalerStatus();

    expect(status.requestedMode, ErikaUpscalerMode.artCnnC4F32);
    expect(status.activeBackend, ErikaUpscalerBackendStatus.simdgroupMatrix);
    expect(status.fallbackCount, 1);
    expect(status.upscaledFrames, 42);
    expect(status.lastEncodeDuration, const Duration(microseconds: 1200));
    expect(status.lastGpuDuration, const Duration(microseconds: 3400));

    final call = playerCalls.singleWhere(
      (MethodCall call) => call.method == 'getUpscalerStatus',
    );
    expect(call.arguments, <String, Object?>{'playerId': 7});

    await player.dispose();
  });

  test('danmaku config forwards block words as json', () async {
    final player = ErikaPlayer();

    await player.setDanmakuConfig(
      maxQuantity: 80,
      shadowStyle: 3,
      customFontFamily: 'DanmakuRuntime_abc',
      customFontFilePath: '/tmp/danmaku.otf',
      blockWords: <String>['spoiler', 'regex/[0-9]+/'],
    );

    final call = playerCalls.singleWhere(
      (MethodCall call) => call.method == 'setDanmakuConfig',
    );
    expect(call.arguments, <String, Object?>{
      'playerId': 7,
      'maxQuantity': 80,
      'shadowStyle': 3,
      'customFontFamily': 'DanmakuRuntime_abc',
      'customFontFilePath': '/tmp/danmaku.otf',
      'blockWordsJson': '["spoiler","regex/[0-9]+/"]',
    });

    await player.dispose();
  });

  test('danmaku config coalesces rapid updates', () async {
    final player = ErikaPlayer();

    final first = player.setDanmakuConfig(fontSize: 24.0);
    final second = player.setDanmakuConfig(fontSize: 30.0);
    final third = player.setDanmakuConfig(fontSize: 30.0, opacity: 0.75);
    await Future.wait(<Future<void>>[first, second, third]);

    final calls = playerCalls
        .where((MethodCall call) => call.method == 'setDanmakuConfig')
        .toList(growable: false);
    expect(calls, hasLength(1));
    expect(calls.single.arguments, <String, Object?>{
      'playerId': 7,
      'fontSize': 30.0,
      'opacity': 0.75,
    });

    await player.setDanmakuConfig(fontSize: 30.0, opacity: 0.75);
    final callsAfterDuplicate = playerCalls
        .where((MethodCall call) => call.method == 'setDanmakuConfig')
        .toList(growable: false);
    expect(callsAfterDuplicate, hasLength(1));

    await player.dispose();
  });

  test('danmaku track controls forward multi-track input', () async {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(playerChannel, (MethodCall call) async {
      playerCalls.add(call);
      return switch (call.method) {
        'create' => 7,
        'addDanmakuTrackFile' => 11,
        'addDanmakuTrackJson' => 12,
        'dispose' => null,
        _ => null,
      };
    });
    final player = ErikaPlayer();

    final fileTrack = await player.addDanmakuTrackFile(
      '/tmp/a.xml',
      name: 'A',
      offset: const Duration(milliseconds: -500),
    );
    final jsonTrack = await player.addDanmakuTrackJson(
      '{"comments":[]}',
      name: 'B',
      offset: const Duration(milliseconds: 250),
    );
    await player.setDanmakuTrackEnabled(fileTrack, false);
    await player.setDanmakuTrackOffset(jsonTrack, const Duration(seconds: 1));
    await player.setDanmakuGlobalOffset(const Duration(milliseconds: -100));
    await player.removeDanmakuTrack(fileTrack);

    expect(fileTrack, 11);
    expect(jsonTrack, 12);
    expect(
      playerCalls
          .singleWhere(
              (MethodCall call) => call.method == 'addDanmakuTrackFile')
          .arguments,
      <String, Object?>{
        'playerId': 7,
        'uri': '/tmp/a.xml',
        'name': 'A',
        'offsetMicros': -500000,
      },
    );
    expect(
      playerCalls
          .singleWhere(
              (MethodCall call) => call.method == 'addDanmakuTrackJson')
          .arguments,
      <String, Object?>{
        'playerId': 7,
        'json': '{"comments":[]}',
        'name': 'B',
        'offsetMicros': 250000,
      },
    );
    expect(
      playerCalls
          .singleWhere(
              (MethodCall call) => call.method == 'setDanmakuTrackEnabled')
          .arguments,
      <String, Object?>{'playerId': 7, 'trackId': 11, 'enabled': false},
    );
    expect(
      playerCalls
          .singleWhere(
              (MethodCall call) => call.method == 'setDanmakuTrackOffset')
          .arguments,
      <String, Object?>{'playerId': 7, 'trackId': 12, 'offsetMicros': 1000000},
    );
    expect(
      playerCalls
          .singleWhere(
              (MethodCall call) => call.method == 'setDanmakuGlobalOffset')
          .arguments,
      <String, Object?>{'playerId': 7, 'offsetMicros': -100000},
    );
    expect(
      playerCalls
          .singleWhere((MethodCall call) => call.method == 'removeDanmakuTrack')
          .arguments,
      <String, Object?>{'playerId': 7, 'trackId': 11},
    );

    await player.dispose();
  });

  test('danmaku tracks query parses native track list', () async {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(playerChannel, (MethodCall call) async {
      playerCalls.add(call);
      return switch (call.method) {
        'create' => 7,
        'danmakuTracks' => <Map<String, Object?>>[
            <String, Object?>{
              'id': 11,
              'enabled': true,
              'offsetMicros': -500000,
              'itemCount': 42,
              'name': 'A',
              'source': '/tmp/a.xml',
            },
          ],
        'dispose' => null,
        _ => null,
      };
    });
    final player = ErikaPlayer();

    final tracks = await player.danmakuTracks();

    expect(tracks, hasLength(1));
    expect(tracks.single.id, 11);
    expect(tracks.single.enabled, isTrue);
    expect(tracks.single.offset, const Duration(milliseconds: -500));
    expect(tracks.single.itemCount, 42);
    expect(tracks.single.name, 'A');
    expect(tracks.single.source, '/tmp/a.xml');

    await player.dispose();
  });

  test('tracks query parses native track list', () async {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(playerChannel, (MethodCall call) async {
      playerCalls.add(call);
      return switch (call.method) {
        'create' => 7,
        'tracks' => <Map<String, Object?>>[
            <String, Object?>{
              'id': 0,
              'kind': 0,
              'source': 0,
              'selected': true,
              'canRemove': false,
              'title': 'Main video',
              'language': null,
              'codec': 'hevc',
            },
            <String, Object?>{
              'id': 1000001,
              'kind': 2,
              'source': 1,
              'selected': true,
              'canRemove': true,
              'title': 'subs.srt',
              'language': 'jpn',
              'codec': 'subrip',
            },
          ],
        'dispose' => null,
        _ => null,
      };
    });
    final player = ErikaPlayer();

    final tracks = await player.tracks();

    expect(tracks, hasLength(2));
    expect(tracks.first.kind, ErikaTrackKind.video);
    expect(tracks.first.source, ErikaTrackSource.embedded);
    expect(tracks.first.selected, isTrue);
    expect(tracks.last.kind, ErikaTrackKind.subtitle);
    expect(tracks.last.source, ErikaTrackSource.external);
    expect(tracks.last.canRemove, isTrue);
    expect(tracks.last.title, 'subs.srt');

    await player.dispose();
  });

  test('player event parses track list and selection', () {
    final event = ErikaPlayerEvent.fromMap(<String, Object?>{
      'playerId': 7,
      'kind': ErikaEventKind.trackSelectionChanged.index,
      'state': ErikaPlaybackState.ready.index,
      'durationMicros': 0,
      'positionMicros': 0,
      'buffering': false,
      'video': <String, Object?>{},
      'tracks': <String, Object?>{'video': 1, 'audio': 1, 'subtitle': 1},
      'trackSelection': <String, Object?>{
        'video': 0,
        'audio': -1,
        'subtitle': 1000001,
      },
      'trackList': <Map<String, Object?>>[
        <String, Object?>{
          'id': 1000001,
          'kind': 2,
          'source': 1,
          'selected': true,
          'canRemove': true,
          'title': 'subs.ass',
          'language': null,
          'codec': 'ass',
        },
      ],
      'status': 0,
    });

    expect(event.kind, ErikaEventKind.trackSelectionChanged);
    expect(event.trackSelection.video, 0);
    expect(event.trackSelection.audio, isNull);
    expect(event.trackSelection.subtitle, 1000001);
    expect(event.trackList.single.isExternal, isTrue);
    expect(event.trackList.single.canRemove, isTrue);
  });
}
