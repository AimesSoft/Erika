import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';

import 'erika_player.dart';

bool get _supportsWindowOverlayVideoView =>
    !kIsWeb &&
    (defaultTargetPlatform == TargetPlatform.iOS ||
        defaultTargetPlatform == TargetPlatform.macOS);

/// Flutter platform-view video surface backed by AppKit/UIKit.
///
/// This is the compatibility surface. Full-player Apple hosts should usually
/// prefer [ErikaWindowOverlayVideoView] so Erika owns the native Metal video
/// plane outside Flutter's platform-view compositor.
class ErikaVideoView extends StatefulWidget {
  const ErikaVideoView({
    super.key,
    required this.player,
    this.debugLabel,
    this.onPlatformViewIdChanged,
  });

  final ErikaPlayer player;
  final String? debugLabel;
  final ValueChanged<int?>? onPlatformViewIdChanged;

  @override
  State<ErikaVideoView> createState() => _ErikaVideoViewState();
}

class _ErikaVideoViewState extends State<ErikaVideoView> {
  int? _viewId;

  @override
  void didUpdateWidget(covariant ErikaVideoView oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.player != widget.player) {
      final viewId = _viewId;
      if (viewId != null) {
        unawaited(oldWidget.player.detachView(viewId));
        unawaited(widget.player.attachView(viewId));
      }
    }
  }

  @override
  void dispose() {
    final viewId = _viewId;
    widget.onPlatformViewIdChanged?.call(null);
    if (viewId != null) {
      unawaited(widget.player.detachView(viewId));
    }
    super.dispose();
  }

  void _handlePlatformViewCreated(int id) {
    if (!mounted) {
      return;
    }
    _viewId = id;
    widget.onPlatformViewIdChanged?.call(id);
    unawaited(widget.player.attachView(id));
  }

  @override
  Widget build(BuildContext context) {
    if (kIsWeb) {
      return const SizedBox.shrink();
    }
    final creationParams = <String, Object?>{
      if (widget.debugLabel case final label?) 'debugLabel': label,
    };
    switch (defaultTargetPlatform) {
      case TargetPlatform.macOS:
        return AppKitView(
          viewType: 'erika_flutter/video_view',
          layoutDirection: TextDirection.ltr,
          creationParamsCodec: const StandardMessageCodec(),
          creationParams: creationParams,
          onPlatformViewCreated: _handlePlatformViewCreated,
        );
      case TargetPlatform.iOS:
        return UiKitView(
          viewType: 'erika_flutter/video_view',
          layoutDirection: TextDirection.ltr,
          creationParamsCodec: const StandardMessageCodec(),
          creationParams: creationParams,
          onPlatformViewCreated: _handlePlatformViewCreated,
          hitTestBehavior: PlatformViewHitTestBehavior.transparent,
          gestureRecognizers: const <Factory<OneSequenceGestureRecognizer>>{},
        );
      case TargetPlatform.android:
      case TargetPlatform.fuchsia:
      case TargetPlatform.linux:
      case TargetPlatform.windows:
        return const SizedBox.shrink();
    }
  }
}

/// Window-hosted native Metal video surface for macOS and iOS.
///
/// The widget reserves a Flutter layout rect while the platform plugin hosts a
/// sibling native view with a CAMetalLayer and keeps its frame in sync.
class ErikaWindowOverlayVideoView extends StatefulWidget {
  const ErikaWindowOverlayVideoView({
    super.key,
    required this.player,
    this.debugLabel,
    this.onPlatformViewIdChanged,
    this.onFrameRectChanged,
  });

  final ErikaPlayer player;
  final String? debugLabel;
  final ValueChanged<int?>? onPlatformViewIdChanged;
  final ValueChanged<Rect?>? onFrameRectChanged;

  @override
  State<ErikaWindowOverlayVideoView> createState() =>
      _ErikaWindowOverlayVideoViewState();
}

class _ErikaWindowOverlayVideoViewState
    extends State<ErikaWindowOverlayVideoView> with WidgetsBindingObserver {
  Timer? _retryTimer;
  Timer? _frameTimer;
  int _bindAttempts = 0;
  bool _isBound = false;
  late final int _surfaceGeneration;
  String? _lastFrameSignature;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _surfaceGeneration = identityHashCode(this);
    widget.onPlatformViewIdChanged?.call(ErikaPlayer.windowOverlayViewId);
    _startFrameTimer();
    _scheduleAttach();
  }

  @override
  void didUpdateWidget(covariant ErikaWindowOverlayVideoView oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.player != widget.player) {
      _retryTimer?.cancel();
      _bindAttempts = 0;
      _isBound = false;
      _lastFrameSignature = null;
      unawaited(
        oldWidget.player.detachWindowOverlay(generation: _surfaceGeneration),
      );
      widget.onPlatformViewIdChanged?.call(ErikaPlayer.windowOverlayViewId);
      _scheduleAttach();
    }
  }

  @override
  void didChangeMetrics() {
    _scheduleFrameUpdate(force: true);
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _retryTimer?.cancel();
    _frameTimer?.cancel();
    widget.onPlatformViewIdChanged?.call(null);
    unawaited(_hideOverlayFrame());
    unawaited(
      widget.player.detachWindowOverlay(generation: _surfaceGeneration),
    );
    super.dispose();
  }

  void _startFrameTimer() {
    _frameTimer?.cancel();
    _frameTimer = Timer.periodic(
      const Duration(milliseconds: 250),
      (_) => _scheduleFrameUpdate(),
    );
  }

  void _scheduleAttach() {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted) {
        return;
      }
      unawaited(_attachOverlaySurface());
      _scheduleFrameUpdate(force: true);
    });
  }

  Future<void> _attachOverlaySurface() async {
    if (!mounted || _isBound || !_supportsWindowOverlayVideoView) {
      return;
    }

    try {
      await widget.player.attachWindowOverlay();
      _isBound = true;
      _scheduleFrameUpdate(force: true);
    } catch (error) {
      debugPrint('ErikaWindowOverlayVideoView: bind failed: $error');
      _scheduleRetry();
    }
  }

  void _scheduleRetry() {
    if (_isBound || !mounted) {
      return;
    }
    final attempt = _bindAttempts;
    _bindAttempts += 1;
    final delay = switch (attempt) {
      0 => const Duration(milliseconds: 150),
      1 => const Duration(milliseconds: 300),
      2 => const Duration(milliseconds: 600),
      3 => const Duration(milliseconds: 1200),
      _ => const Duration(seconds: 2),
    };
    _retryTimer?.cancel();
    _retryTimer = Timer(delay, () => unawaited(_attachOverlaySurface()));
  }

  void _scheduleFrameUpdate({bool force = false}) {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted) {
        return;
      }
      unawaited(_sendOverlayFrame(visible: true, force: force));
    });
  }

  Future<void> _sendOverlayFrame({
    required bool visible,
    bool force = false,
  }) async {
    if (!_supportsWindowOverlayVideoView) {
      return;
    }

    final Rect rect;
    if (visible) {
      if (!mounted) {
        return;
      }
      final renderObject = context.findRenderObject();
      if (renderObject is! RenderBox) {
        return;
      }
      final box = renderObject;
      if (!box.hasSize || box.size.isEmpty) {
        return;
      }
      final origin = box.localToGlobal(Offset.zero);
      rect = origin & box.size;
    } else {
      rect = Rect.zero;
    }

    final signature = <Object>[
      visible,
      rect.left.toStringAsFixed(2),
      rect.top.toStringAsFixed(2),
      rect.width.toStringAsFixed(2),
      rect.height.toStringAsFixed(2),
    ].join('|');
    if (!force && signature == _lastFrameSignature) {
      return;
    }
    _lastFrameSignature = signature;
    widget.onFrameRectChanged?.call(visible ? rect : null);

    try {
      await widget.player.setWindowOverlayFrame(
        frame: rect,
        visible: visible,
        generation: _surfaceGeneration,
        debugLabel: widget.debugLabel,
      );
    } catch (error) {
      debugPrint('ErikaWindowOverlayVideoView: frame update failed: $error');
    }
  }

  Future<void> _hideOverlayFrame() async {
    try {
      await widget.player.setWindowOverlayFrame(
        frame: Rect.zero,
        visible: false,
        generation: _surfaceGeneration,
        debugLabel: widget.debugLabel,
      );
    } catch (error) {
      debugPrint('ErikaWindowOverlayVideoView: hide overlay failed: $error');
    }
  }

  @override
  Widget build(BuildContext context) {
    if (!_supportsWindowOverlayVideoView) {
      return const SizedBox.shrink();
    }
    _scheduleFrameUpdate();
    return const SizedBox.expand();
  }
}
