import 'package:erika_flutter/erika_flutter.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  const channel = MethodChannel('erika_flutter/player');
  const events = MethodChannel('erika_flutter/events');
  late List<MethodCall> calls;
  var nextPlayer = 0;

  setUp(() {
    calls = [];
    nextPlayer = 0;
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
      calls.add(call);
      return switch (call.method) {
        'create' => ++nextPlayer,
        'createTexture' => 13,
        'attachOverlay' => -1,
        _ => null,
      };
    });
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(events, (_) async => null);
  });

  tearDown(() {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, null);
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(events, null);
  });

  Widget host(Widget child) => Directionality(
        textDirection: TextDirection.ltr,
        child: Center(child: SizedBox(width: 320, height: 180, child: child)),
      );

  for (final outputMode in [
    null,
    ErikaOutputMode.auto,
    ErikaOutputMode.extendedLinear
  ]) {
    testWidgets('Windows default view preserves native output for $outputMode',
        (tester) async {
      debugDefaultTargetPlatformOverride = TargetPlatform.windows;
      final player = ErikaPlayer(outputMode: outputMode);
      try {
        await tester.pumpWidget(host(ErikaVideoView(player: player)));
        await tester.pump();
        await tester.pump();
        expect(calls.map((c) => c.method), contains('attachOverlay'));
        expect(calls.map((c) => c.method), isNot(contains('createTexture')));
        expect(find.byType(Texture), findsNothing);
      } finally {
        await tester.pumpWidget(const SizedBox.shrink());
        await tester.pump();
        await player.dispose();
        debugDefaultTargetPlatformOverride = null;
      }
    });
  }

  for (final texture in [false, true]) {
    testWidgets(
        'Windows overlay forwards blend/opacity (texture entry: $texture)',
        (tester) async {
      debugDefaultTargetPlatformOverride = TargetPlatform.windows;
      final player =
          ErikaPlayer(videoAlphaMode: ErikaVideoAlphaMode.packedAlphaRight);
      try {
        await tester.pumpWidget(host(texture
            ? ErikaTextureVideoView(
                player: player, blendMode: BlendMode.overlay, opacity: 0.25)
            : ErikaVideoView(
                player: player, blendMode: BlendMode.overlay, opacity: 0.25)));
        await tester.pump();
        await tester.pump();
        final attach = calls.firstWhere((c) => c.method == 'attachOverlay');
        expect((attach.arguments as Map)['blendMode'], 'overlay');
        expect((attach.arguments as Map)['opacity'], 0.25);
        expect(find.byType(Texture), findsNothing);
      } finally {
        await tester.pumpWidget(const SizedBox.shrink());
        await tester.pump();
        await player.dispose();
        debugDefaultTargetPlatformOverride = null;
      }
    });
  }

  testWidgets(
      'explicit Windows texture applies opacity and rebinds after player disposal',
      (tester) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.windows;
    final oldPlayer = ErikaPlayer();
    final newPlayer = ErikaPlayer();
    try {
      await tester.pumpWidget(
          host(ErikaTextureVideoView(player: oldPlayer, opacity: 0.4)));
      await tester.pump();
      await tester.pump();
      expect(find.byType(Texture), findsOneWidget);
      expect(tester.widget<Opacity>(find.byType(Opacity)).opacity, 0.4);
      await oldPlayer.dispose();
      await tester.pumpWidget(
          host(ErikaTextureVideoView(player: newPlayer, opacity: 0.4)));
      await tester.pump();
      expect(calls.where((c) => c.method == 'createTexture'), hasLength(1));
      final attached =
          calls.where((c) => c.method == 'attachView').last.arguments as Map;
      expect(attached['playerId'], 2);
      expect(attached['viewId'], 13);
    } finally {
      await tester.pumpWidget(const SizedBox.shrink());
      await tester.pump();
      await oldPlayer.dispose();
      await newPlayer.dispose();
      debugDefaultTargetPlatformOverride = null;
    }
  });

  testWidgets(
      'unsupported Windows blend modes fail explicitly before texture allocation',
      (tester) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.windows;
    final player = ErikaPlayer();
    try {
      await tester.pumpWidget(host(ErikaTextureVideoView(
          player: player, blendMode: BlendMode.multiply)));
      expect(tester.takeException(), isA<UnsupportedError>());
      expect(calls, isEmpty);
    } finally {
      await tester.pumpWidget(const SizedBox.shrink());
      await player.dispose();
      debugDefaultTargetPlatformOverride = null;
    }
  });
}
