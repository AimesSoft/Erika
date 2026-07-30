import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
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

  test('open forwards HTTP headers without exposing them elsewhere', () async {
    final player = ErikaPlayer();

    await player.open(
      'https://example.test/video.mkv',
      httpHeaders: <String, String>{'Authorization': 'Bearer secret'},
    );

    final call = playerCalls.singleWhere((call) => call.method == 'open');
    expect(call.arguments, <String, Object?>{
      'playerId': 7,
      'uri': 'https://example.test/video.mkv',
      'httpHeaders': <String, String>{'Authorization': 'Bearer secret'},
    });
    await player.dispose();
  });

  test('open omits null and empty HTTP headers', () async {
    final player = ErikaPlayer();

    await player.open('https://example.test/null.mkv');
    await player.open(
      'https://example.test/empty.mkv',
      httpHeaders: <String, String>{},
    );

    final openCalls = playerCalls
        .where((MethodCall call) => call.method == 'open')
        .toList(growable: false);
    expect(openCalls, hasLength(2));
    expect(openCalls[0].arguments, <String, Object?>{
      'playerId': 7,
      'uri': 'https://example.test/null.mkv',
    });
    expect(openCalls[1].arguments, <String, Object?>{
      'playerId': 7,
      'uri': 'https://example.test/empty.mkv',
    });

    await player.dispose();
  });

  test('open preserves multiple HTTP headers and empty values', () async {
    final player = ErikaPlayer();

    await player.open(
      'https://example.test/multiple.mkv',
      httpHeaders: <String, String>{
        'Authorization': 'Bearer token',
        'X-Empty': '',
        'X-Request-ID': 'request-123',
      },
    );

    final call = playerCalls.singleWhere(
      (MethodCall call) => call.method == 'open',
    );
    expect(call.arguments, <String, Object?>{
      'playerId': 7,
      'uri': 'https://example.test/multiple.mkv',
      'httpHeaders': <String, String>{
        'Authorization': 'Bearer token',
        'X-Empty': '',
        'X-Request-ID': 'request-123',
      },
    });

    await player.dispose();
  });

  test('media metadata is forwarded for system now playing info', () async {
    final player = ErikaPlayer();
    final artwork = Uint8List.fromList(<int>[1, 2, 3, 4]);
    final metadata = ErikaMediaMetadata(
      title: 'Episode 1',
      artist: 'Erika',
      album: 'Season 1',
      artwork: artwork,
    );

    await player.open('https://example.test/video.mkv', metadata: metadata);
    await player.setMediaMetadata(metadata);

    final openCall = playerCalls.singleWhere(
      (MethodCall call) => call.method == 'open',
    );
    expect(openCall.arguments, <String, Object?>{
      'playerId': 7,
      'uri': 'https://example.test/video.mkv',
      'metadata': <String, Object>{
        'title': 'Episode 1',
        'artist': 'Erika',
        'album': 'Season 1',
        'artwork': artwork,
      },
    });
    final metadataCall = playerCalls.singleWhere(
      (MethodCall call) => call.method == 'setMediaMetadata',
    );
    expect(metadataCall.arguments, <String, Object?>{
      'playerId': 7,
      'metadata': <String, Object>{
        'title': 'Episode 1',
        'artist': 'Erika',
        'album': 'Season 1',
        'artwork': artwork,
      },
    });

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

  test('extended-linear output mode is passed to native create', () async {
    final player = ErikaPlayer(
      outputMode: ErikaOutputMode.extendedLinear,
      edrHeadroom: 3.0,
    );

    expect(await player.ensureCreated(), 7);

    final createCall = playerCalls.singleWhere(
      (MethodCall call) => call.method == 'create',
    );
    final arguments = createCall.arguments as Map<Object?, Object?>;
    expect(arguments['outputMode'], ErikaOutputMode.extendedLinear.nativeValue);
    expect(arguments['edrHeadroom'], 3.0);

    await player.dispose();
  });

  test('extended-linear output defaults shader headroom to four', () async {
    final player = ErikaPlayer(outputMode: ErikaOutputMode.extendedLinear);

    expect(await player.ensureCreated(), 7);

    final createCall = playerCalls.singleWhere(
      (MethodCall call) => call.method == 'create',
    );
    final arguments = createCall.arguments as Map<Object?, Object?>;
    expect(arguments['edrHeadroom'], 4.0);

    await player.dispose();
  });

  test('invalid explicit output headroom is rejected before native create', () {
    expect(
      () => ErikaPlayer(
        outputMode: ErikaOutputMode.extendedLinear,
        edrHeadroom: 0.5,
      ),
      throwsArgumentError,
    );
    expect(
      () => ErikaPlayer(
        outputMode: ErikaOutputMode.extendedLinear,
        edrHeadroom: double.nan,
      ),
      throwsArgumentError,
    );
    expect(playerCalls, isEmpty);
  });

  test('initial upscaler mode is passed to native create', () async {
    final player = ErikaPlayer(upscaler: ErikaUpscalerMode.artCnnC4F32);

    expect(await player.ensureCreated(), 7);

    final createCall = playerCalls.singleWhere(
      (MethodCall call) => call.method == 'create',
    );
    final arguments = createCall.arguments as Map<Object?, Object?>;
    expect(arguments['upscaler'], ErikaUpscalerMode.artCnnC4F32.nativeValue);

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

  test(
    'dispose waits for delayed create and blocks pending player calls',
    () async {
      final createCompleter = Completer<int>();
      final disposeCompleter = Completer<void>();
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
          .setMockMethodCallHandler(playerChannel, (MethodCall call) async {
        playerCalls.add(call);
        return switch (call.method) {
          'create' => createCompleter.future,
          'dispose' => disposeCompleter.future,
          _ => null,
        };
      });
      final player = ErikaPlayer();

      final openFuture = player.open('/tmp/delayed.mkv');
      final openExpectation = expectLater(
        openFuture,
        throwsA(isA<StateError>()),
      );
      await Future<void>.delayed(Duration.zero);
      expect(
        playerCalls.where((MethodCall call) => call.method == 'create'),
        hasLength(1),
      );

      final firstDispose = player.dispose();
      final secondDispose = player.dispose();
      expect(identical(firstDispose, secondDispose), isTrue);
      expect(() => player.ensureCreated(), throwsA(isA<StateError>()));

      createCompleter.complete(41);
      await openExpectation;
      await Future<void>.delayed(Duration.zero);

      expect(
        playerCalls.where((MethodCall call) => call.method == 'open'),
        isEmpty,
      );
      final disposeCalls = playerCalls
          .where((MethodCall call) => call.method == 'dispose')
          .toList(growable: false);
      expect(disposeCalls, hasLength(1));
      expect(disposeCalls.single.arguments, <String, Object?>{'playerId': 41});

      var cleanupCompleted = false;
      final observedDispose = firstDispose.whenComplete(
        () => cleanupCompleted = true,
      );
      await Future<void>.delayed(Duration.zero);
      expect(cleanupCompleted, isFalse);

      disposeCompleter.complete();
      await Future.wait(<Future<void>>[observedDispose, secondDispose]);
      expect(cleanupCompleted, isTrue);
      expect(player.id, isNull);
      await expectLater(player.play(), throwsA(isA<StateError>()));
    },
  );

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

  test('screenshot returns null when native has no current frame', () async {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(playerChannel, (MethodCall call) async {
      return switch (call.method) {
        'create' => 7,
        'screenshot' => null,
        'dispose' => null,
        _ => null,
      };
    });
    final player = ErikaPlayer();

    expect(await player.screenshot(width: 320, height: 180), isNull);

    await player.dispose();
  });

  test('track selection methods forward nullable track ids', () async {
    final player = ErikaPlayer();

    await player.selectAudioTrack(2);
    await player.selectSubtitleTrack(null);

    final audioCall = playerCalls.singleWhere(
      (MethodCall call) => call.method == 'selectAudioTrack',
    );
    expect(audioCall.arguments, <String, Object?>{'playerId': 7, 'trackId': 2});

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
    expect(call.arguments, <String, Object?>{'playerId': 7, 'rate': 1.5});

    await player.dispose();
  });

  test('debug HUD toggle is forwarded to native presenter', () async {
    final player = ErikaPlayer();

    await player.setDebugHudEnabled(true);
    await player.setDebugHudEnabled(false);

    final calls = playerCalls
        .where((MethodCall call) => call.method == 'setDebugHudEnabled')
        .toList();
    expect(calls, hasLength(2));
    expect(calls.first.arguments, <String, Object?>{
      'playerId': 7,
      'enabled': true,
    });
    expect(calls.last.arguments, <String, Object?>{
      'playerId': 7,
      'enabled': false,
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
        'playerId': 7,
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

  testWidgets('window overlay uses the Android TextureView platform view', (
    WidgetTester tester,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.android;
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(SystemChannels.platform_views, (
      MethodCall call,
    ) async {
      final arguments = call.arguments as Map<Object?, Object?>?;
      return switch (call.method) {
        'create' => 1,
        'resize' => <String, Object?>{
            'width': arguments!['width'],
            'height': arguments['height'],
          },
        _ => null,
      };
    });
    final player = ErikaPlayer();
    try {
      await tester.pumpWidget(
        Directionality(
          textDirection: TextDirection.ltr,
          child: SizedBox(
            width: 320,
            height: 180,
            child: ErikaWindowOverlayVideoView(
              player: player,
              debugLabel: 'android-video',
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();

      expect(find.byType(AndroidView), findsOneWidget);
      final attachCall = playerCalls.singleWhere(
        (MethodCall call) => call.method == 'attachView',
      );
      final attachArguments = attachCall.arguments as Map<Object?, Object?>;
      expect(attachArguments['playerId'], 7);
      expect(attachArguments['viewId'], isA<int>());
    } finally {
      await tester.pumpWidget(const SizedBox.shrink());
      await tester.pumpAndSettle();
      await player.dispose();
      debugDefaultTargetPlatformOverride = null;
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
          .setMockMethodCallHandler(SystemChannels.platform_views, null);
    }
  });

  testWidgets(
    'extended-linear Android view forces Hybrid Composition SurfaceView',
    (WidgetTester tester) async {
      debugDefaultTargetPlatformOverride = TargetPlatform.android;
      final platformViewCalls = <MethodCall>[];
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
          .setMockMethodCallHandler(SystemChannels.platform_views, (
        MethodCall call,
      ) async {
        platformViewCalls.add(call);
        return null;
      });
      final player = ErikaPlayer(
        outputMode: ErikaOutputMode.extendedLinear,
        edrHeadroom: 4.0,
      );
      try {
        await tester.pumpWidget(
          Directionality(
            textDirection: TextDirection.ltr,
            child: SizedBox(
              width: 320,
              height: 180,
              child: ErikaVideoView(
                player: player,
                debugLabel: 'android-hdr-video',
              ),
            ),
          ),
        );
        await tester.pumpAndSettle();

        expect(find.byType(PlatformViewLink), findsOneWidget);
        expect(find.byType(AndroidView), findsNothing);
        final createCall = platformViewCalls.singleWhere(
          (MethodCall call) => call.method == 'create',
        );
        final createArguments = createCall.arguments as Map<Object?, Object?>;
        expect(createArguments['viewType'], 'erika_flutter/hdr_video_view');
        expect(createArguments['hybrid'], isTrue);
        final encodedParams = createArguments['params'] as Uint8List;
        final creationParams = const StandardMessageCodec().decodeMessage(
          ByteData.sublistView(encodedParams),
        ) as Map<Object?, Object?>;
        expect(
          creationParams['outputMode'],
          ErikaOutputMode.extendedLinear.nativeValue,
        );
        expect(creationParams['requestedHdrHeadroom'], 4.0);
        expect(creationParams['composition'], 'hybrid');

        final attachCall = playerCalls.singleWhere(
          (MethodCall call) => call.method == 'attachView',
        );
        final attachArguments = attachCall.arguments as Map<Object?, Object?>;
        expect(attachArguments['playerId'], 7);
        expect(attachArguments['viewId'], createArguments['id']);
      } finally {
        await tester.pumpWidget(const SizedBox.shrink());
        await tester.pumpAndSettle();
        await player.dispose();
        debugDefaultTargetPlatformOverride = null;
        TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
            .setMockMethodCallHandler(SystemChannels.platform_views, null);
      }
    },
  );

  testWidgets('changing extended-linear headroom recreates the Android view', (
    WidgetTester tester,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.android;
    var nextPlayerId = 7;
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(playerChannel, (MethodCall call) async {
      playerCalls.add(call);
      return switch (call.method) {
        'create' => nextPlayerId++,
        'dispose' => null,
        _ => null,
      };
    });
    final platformViewCalls = <MethodCall>[];
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(SystemChannels.platform_views, (
      MethodCall call,
    ) async {
      platformViewCalls.add(call);
      return null;
    });
    final automatic = ErikaPlayer(outputMode: ErikaOutputMode.extendedLinear);
    final explicit = ErikaPlayer(
      outputMode: ErikaOutputMode.extendedLinear,
      edrHeadroom: 2.5,
    );
    try {
      Widget view(ErikaPlayer player) => Directionality(
            textDirection: TextDirection.ltr,
            child: SizedBox(
              width: 320,
              height: 180,
              child: ErikaVideoView(player: player),
            ),
          );

      await tester.pumpWidget(view(automatic));
      await tester.pumpAndSettle();
      await tester.pumpWidget(view(explicit));
      await tester.pumpAndSettle();

      final createCalls = platformViewCalls
          .where((MethodCall call) => call.method == 'create')
          .toList(growable: false);
      expect(createCalls, hasLength(2));
      final creationParams = createCalls.map((MethodCall call) {
        final arguments = call.arguments as Map<Object?, Object?>;
        return const StandardMessageCodec().decodeMessage(
          ByteData.sublistView(arguments['params'] as Uint8List),
        ) as Map<Object?, Object?>;
      }).toList(growable: false);
      expect(creationParams[0]['requestedHdrHeadroom'], 0.0);
      expect(creationParams[1]['requestedHdrHeadroom'], 2.5);
      expect(
        playerCalls.where((MethodCall call) => call.method == 'attachView'),
        hasLength(2),
      );
      expect(
        playerCalls.where((MethodCall call) => call.method == 'detachView'),
        isNotEmpty,
      );
    } finally {
      await tester.pumpWidget(const SizedBox.shrink());
      await tester.pumpAndSettle();
      await automatic.dispose();
      await explicit.dispose();
      debugDefaultTargetPlatformOverride = null;
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
          .setMockMethodCallHandler(SystemChannels.platform_views, null);
    }
  });

  testWidgets('Android player switch retries a transient attach failure', (
    WidgetTester tester,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.android;
    var nextPlayerId = 7;
    var replacementAttachAttempts = 0;
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(playerChannel, (MethodCall call) async {
      playerCalls.add(call);
      final arguments = call.arguments as Map<Object?, Object?>?;
      return switch (call.method) {
        'create' => nextPlayerId++,
        'attachView' when arguments?['playerId'] == 8 =>
          ++replacementAttachAttempts < 3
              ? throw PlatformException(
                  code: 'ERIKA_ERROR',
                  message: 'transient detach recovery',
                )
              : null,
        'dispose' => null,
        _ => null,
      };
    });
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(SystemChannels.platform_views, (
      MethodCall call,
    ) async {
      final arguments = call.arguments as Map<Object?, Object?>?;
      return switch (call.method) {
        'create' => 1,
        'resize' => <String, Object?>{
            'width': arguments!['width'],
            'height': arguments['height'],
          },
        _ => null,
      };
    });
    final initial = ErikaPlayer();
    final replacement = ErikaPlayer();
    try {
      Widget view(ErikaPlayer player) => Directionality(
            textDirection: TextDirection.ltr,
            child: SizedBox(
              width: 320,
              height: 180,
              child: ErikaVideoView(player: player),
            ),
          );

      await tester.pumpWidget(view(initial));
      await tester.pumpAndSettle();
      await tester.pumpWidget(view(replacement));
      await tester.pumpAndSettle();

      expect(replacementAttachAttempts, 3);
      expect(
        playerCalls.where((MethodCall call) {
          if (call.method != 'detachView') {
            return false;
          }
          final arguments = call.arguments as Map<Object?, Object?>;
          return arguments['playerId'] == 7;
        }),
        isNotEmpty,
      );
    } finally {
      await tester.pumpWidget(const SizedBox.shrink());
      await tester.pumpAndSettle();
      await initial.dispose();
      await replacement.dispose();
      debugDefaultTargetPlatformOverride = null;
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
          .setMockMethodCallHandler(SystemChannels.platform_views, null);
    }
  });

  testWidgets(
    'stale Android platform view creation cannot replace new output config',
    (WidgetTester tester) async {
      debugDefaultTargetPlatformOverride = TargetPlatform.android;
      var nextPlayerId = 7;
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
          .setMockMethodCallHandler(playerChannel, (MethodCall call) async {
        playerCalls.add(call);
        return switch (call.method) {
          'create' => nextPlayerId++,
          'dispose' => null,
          _ => null,
        };
      });
      final createCompleters = <Completer<void>>[];
      final createViewIds = <int>[];
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
          .setMockMethodCallHandler(SystemChannels.platform_views, (
        MethodCall call,
      ) {
        if (call.method != 'create') {
          return Future<Object?>.value(null);
        }
        final arguments = call.arguments as Map<Object?, Object?>;
        final completer = Completer<void>();
        createViewIds.add(arguments['id']! as int);
        createCompleters.add(completer);
        return completer.future.then<Object?>((_) => null);
      });
      final automatic = ErikaPlayer(outputMode: ErikaOutputMode.extendedLinear);
      final explicit = ErikaPlayer(
        outputMode: ErikaOutputMode.extendedLinear,
        edrHeadroom: 2.5,
      );
      try {
        expect(await automatic.ensureCreated(), 7);
        expect(await explicit.ensureCreated(), 8);
        Widget view(ErikaPlayer player) => Directionality(
              textDirection: TextDirection.ltr,
              child: SizedBox(
                width: 320,
                height: 180,
                child: ErikaVideoView(player: player),
              ),
            );

        await tester.pumpWidget(view(automatic));
        await tester.pump();
        expect(createCompleters, hasLength(1));

        await tester.pumpWidget(view(explicit));
        await tester.pump();
        expect(createCompleters, hasLength(2));

        createCompleters[1].complete();
        await tester.pump();
        createCompleters[0].complete();
        await tester.pump();

        final attachCalls = playerCalls
            .where((MethodCall call) => call.method == 'attachView')
            .toList(growable: false);
        expect(attachCalls, hasLength(1));
        expect(attachCalls.single.arguments, <String, Object?>{
          'playerId': 8,
          'viewId': createViewIds[1],
        });
      } finally {
        for (final completer in createCompleters) {
          if (!completer.isCompleted) {
            completer.complete();
          }
        }
        await tester.pumpWidget(const SizedBox.shrink());
        await tester.pumpAndSettle();
        await automatic.dispose();
        await explicit.dispose();
        debugDefaultTargetPlatformOverride = null;
        TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
            .setMockMethodCallHandler(SystemChannels.platform_views, null);
      }
    },
  );

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

  test(
    'extended-linear output status is decoded from native presenter',
    () async {
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
          .setMockMethodCallHandler(playerChannel, (MethodCall call) async {
        playerCalls.add(call);
        return switch (call.method) {
          'create' => 7,
          'getOutputStatus' => <String, Object?>{
              'requestedMode': ErikaOutputMode.extendedLinear.nativeValue,
              'activeEncoding': ErikaActiveOutputEncoding
                  .androidExtendedLinearScRgb.nativeValue,
              'surfaceFormat':
                  ErikaOutputSurfaceFormat.sixteenBitFloat.nativeValue,
              'nativeDataSpace': 0x18410000,
              'requestedHeadroom': 4.0,
              'activeHeadroom': 3.5,
              'activeHeadroomKnown': true,
              'extendedLinearActive': true,
              'fallbackReason': ErikaOutputFallbackReason.none.nativeValue,
              'fallbackCount': 0,
              'dataSpaceFailures': 0,
              'headroomUpdates': 2,
              'extendedLinearFrames': 42,
            },
          'dispose' => null,
          _ => null,
        };
      });

      final player = ErikaPlayer(outputMode: ErikaOutputMode.extendedLinear);
      final status = await player.getOutputStatus();

      expect(status.requestedMode, ErikaOutputMode.extendedLinear);
      expect(
        status.activeEncoding,
        ErikaActiveOutputEncoding.androidExtendedLinearScRgb,
      );
      expect(status.surfaceFormat, ErikaOutputSurfaceFormat.sixteenBitFloat);
      expect(status.nativeDataSpace, 0x18410000);
      expect(status.activeHeadroom, 3.5);
      expect(status.extendedLinearActive, isTrue);
      expect(status.fallbackReason, ErikaOutputFallbackReason.none);
      expect(status.extendedLinearFrames, 42);

      final call = playerCalls.singleWhere(
        (MethodCall call) => call.method == 'getOutputStatus',
      );
      expect(call.arguments, <String, Object?>{'playerId': 7});

      await player.dispose();
    },
  );

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
            (MethodCall call) => call.method == 'addDanmakuTrackFile',
          )
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
            (MethodCall call) => call.method == 'addDanmakuTrackJson',
          )
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
            (MethodCall call) => call.method == 'setDanmakuTrackEnabled',
          )
          .arguments,
      <String, Object?>{'playerId': 7, 'trackId': 11, 'enabled': false},
    );
    expect(
      playerCalls
          .singleWhere(
            (MethodCall call) => call.method == 'setDanmakuTrackOffset',
          )
          .arguments,
      <String, Object?>{'playerId': 7, 'trackId': 12, 'offsetMicros': 1000000},
    );
    expect(
      playerCalls
          .singleWhere(
            (MethodCall call) => call.method == 'setDanmakuGlobalOffset',
          )
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
              'width': 3840,
              'height': 2160,
              'bitRate': 18000000,
              'frameRateNumerator': 30000,
              'frameRateDenominator': 1001,
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
    expect(tracks.first.codec, 'hevc');
    expect(tracks.first.width, 3840);
    expect(tracks.first.height, 2160);
    expect(tracks.first.bitRate, 18000000);
    expect(tracks.first.frameRateNumerator, 30000);
    expect(tracks.first.frameRateDenominator, 1001);
    expect(tracks.first.framesPerSecond, closeTo(29.970, 0.001));
    expect(tracks.last.kind, ErikaTrackKind.subtitle);
    expect(tracks.last.source, ErikaTrackSource.external);
    expect(tracks.last.canRemove, isTrue);
    expect(tracks.last.title, 'subs.srt');
    expect(tracks.last.bitRate, isNull);
    expect(tracks.last.framesPerSecond, isNull);

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
      'error': 'decoder failed',
      'message': 'hardware decoder fallback exhausted',
    });

    expect(event.kind, ErikaEventKind.trackSelectionChanged);
    expect(event.trackSelection.video, 0);
    expect(event.trackSelection.audio, isNull);
    expect(event.trackSelection.subtitle, 1000001);
    expect(event.trackList.single.isExternal, isTrue);
    expect(event.trackList.single.canRemove, isTrue);
    expect(event.error, 'decoder failed');
    expect(event.message, 'hardware decoder fallback exhausted');
  });

  test('presenter stats preserves AAudio recovery diagnostics', () {
    final stats = ErikaPresenterStats.fromMap(<String, Object?>{
      'audioRecoveryState': 3,
      'audioLastErrorCode': -899,
      'audioRecoveryAttempts': 4,
      'audioRecoveryCount': 2,
      'audioRecoveryFailures': 2,
      'videoFrameBackpressureDrops': 7,
    });

    expect(stats.audioRecoveryState, 3);
    expect(stats.audioLastErrorCode, -899);
    expect(stats.audioRecoveryAttempts, 4);
    expect(stats.audioRecoveryCount, 2);
    expect(stats.audioRecoveryFailures, 2);
    expect(stats.videoFrameBackpressureDrops, 7);
    expect(stats.toMap(), containsPair('audioRecoveryState', 3));
    expect(stats.toMap(), containsPair('audioLastErrorCode', -899));
    expect(stats.toMap(), containsPair('audioRecoveryAttempts', 4));
    expect(stats.toMap(), containsPair('audioRecoveryCount', 2));
    expect(stats.toMap(), containsPair('audioRecoveryFailures', 2));
    expect(stats.toMap(), containsPair('videoFrameBackpressureDrops', 7));
  });

  test('player event parses video decoder fallback details', () {
    final event = ErikaPlayerEvent.fromMap(<String, Object?>{
      'playerId': 9,
      'kind': ErikaEventKind.videoDecoderChanged.index,
      'state': ErikaPlaybackState.playing.index,
      'decoder': <String, Object?>{
        'stage': 'renderer_import',
        'requestedBackend': 'mediacodec',
        'previousBackend': 'mediacodec',
        'activeBackend': 'software',
        'fallbackCount': 1,
        'codec': 'h264',
        'pixelFormat': 'nv12',
        'lineSizes': <int>[1920, 1920, 0, 0],
        'reason': 'unsupported MediaCodec buffer layout',
      },
    });

    expect(event.kind, ErikaEventKind.videoDecoderChanged);
    expect(event.decoder?.stage, 'renderer_import');
    expect(event.decoder?.requestedBackend, 'mediacodec');
    expect(event.decoder?.activeBackend, 'software');
    expect(event.decoder?.fallbackCount, 1);
    expect(event.decoder?.lineSizes, <int>[1920, 1920, 0, 0]);
    expect(event.decoder?.reason, contains('buffer layout'));
  });

  test(
    'player event parses AAudio recovery details without kind downgrade',
    () {
      expect(ErikaEventKind.audioOutputChanged.index, 12);
      final event = ErikaPlayerEvent.fromMap(<String, Object?>{
        'playerId': 7,
        'kind': ErikaEventKind.audioOutputChanged.index,
        'state': ErikaPlaybackState.playing.index,
        'audio': <String, Object?>{
          'event': 'audio_output_changed',
          'recoveryState': 'stable',
          'lastErrorCode': -899,
          'recoveryAttempts': 2,
          'recoveryCount': 1,
          'recoveryFailures': 1,
          'transitionSequence': 5,
        },
        'message': '{"event":"audio_output_changed"}',
      });

      expect(event.kind, ErikaEventKind.audioOutputChanged);
      expect(event.audio?.recoveryState, 'stable');
      expect(event.audio?.lastErrorCode, -899);
      expect(event.audio?.recoveryAttempts, 2);
      expect(event.audio?.recoveryCount, 1);
      expect(event.audio?.recoveryFailures, 1);
      expect(event.audio?.transitionSequence, 5);
      expect(event.message, contains('audio_output_changed'));
    },
  );
}
