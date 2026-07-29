import 'dart:async';
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

import 'erika_event.dart';

enum ErikaOutputMode {
  sdr(0),
  appleEdr(1),
  extendedLinear(2);

  const ErikaOutputMode(this.nativeValue);

  final int nativeValue;

  static ErikaOutputMode fromNativeValue(int value) {
    return switch (value) {
      1 => ErikaOutputMode.appleEdr,
      2 => ErikaOutputMode.extendedLinear,
      _ => ErikaOutputMode.sdr,
    };
  }
}

enum ErikaActiveOutputEncoding {
  sdrSrgb(0),
  appleEdr(1),
  androidExtendedLinearScRgb(2),
  hdr10Pq(3);

  const ErikaActiveOutputEncoding(this.nativeValue);

  final int nativeValue;

  static ErikaActiveOutputEncoding fromNativeValue(int value) {
    return switch (value) {
      1 => ErikaActiveOutputEncoding.appleEdr,
      2 => ErikaActiveOutputEncoding.androidExtendedLinearScRgb,
      3 => ErikaActiveOutputEncoding.hdr10Pq,
      _ => ErikaActiveOutputEncoding.sdrSrgb,
    };
  }
}

enum ErikaOutputSurfaceFormat {
  eightBitUnorm(0),
  tenBitUnorm(1),
  sixteenBitFloat(2);

  const ErikaOutputSurfaceFormat(this.nativeValue);

  final int nativeValue;

  static ErikaOutputSurfaceFormat fromNativeValue(int value) {
    return switch (value) {
      1 => ErikaOutputSurfaceFormat.tenBitUnorm,
      2 => ErikaOutputSurfaceFormat.sixteenBitFloat,
      _ => ErikaOutputSurfaceFormat.eightBitUnorm,
    };
  }
}

enum ErikaOutputFallbackReason {
  none(0, 'none'),
  displayHdrUnsupported(1, 'display_hdr_unsupported'),
  hybridCompositionRequired(2, 'hybrid_composition_required'),
  wgpuBackendNotVulkan(3, 'wgpu_backend_not_vulkan'),
  rgba16FloatSurfaceFormatUnavailable(
    4,
    'rgba16float_surface_format_unavailable',
  ),
  nativeWindowDataSpaceApiUnavailable(
    5,
    'native_window_dataspace_api_unavailable',
  ),
  scrgbDataSpaceVerificationFailed(6, 'scrgb_dataspace_verification_failed'),
  surfaceConfigureFailed(7, 'surface_configure_failed'),
  legacyAppleEdrUnsupported(8, 'legacy_apple_edr_unsupported'),
  unknown(-1, 'unknown');

  const ErikaOutputFallbackReason(this.nativeValue, this.label);

  final int nativeValue;
  final String label;

  static ErikaOutputFallbackReason fromNativeValue(int value) {
    return switch (value) {
      0 => ErikaOutputFallbackReason.none,
      1 => ErikaOutputFallbackReason.displayHdrUnsupported,
      2 => ErikaOutputFallbackReason.hybridCompositionRequired,
      3 => ErikaOutputFallbackReason.wgpuBackendNotVulkan,
      4 => ErikaOutputFallbackReason.rgba16FloatSurfaceFormatUnavailable,
      5 => ErikaOutputFallbackReason.nativeWindowDataSpaceApiUnavailable,
      6 => ErikaOutputFallbackReason.scrgbDataSpaceVerificationFailed,
      7 => ErikaOutputFallbackReason.surfaceConfigureFailed,
      8 => ErikaOutputFallbackReason.legacyAppleEdrUnsupported,
      _ => ErikaOutputFallbackReason.unknown,
    };
  }
}

class ErikaOutputStatus {
  const ErikaOutputStatus({
    required this.requestedMode,
    required this.activeEncoding,
    required this.surfaceFormat,
    required this.nativeDataSpace,
    required this.requestedHeadroom,
    required this.activeHeadroom,
    required this.activeHeadroomKnown,
    required this.extendedLinearActive,
    required this.fallbackReason,
    required this.fallbackCount,
    required this.dataSpaceFailures,
    required this.headroomUpdates,
    required this.extendedLinearFrames,
  });

  final ErikaOutputMode requestedMode;
  final ErikaActiveOutputEncoding activeEncoding;
  final ErikaOutputSurfaceFormat surfaceFormat;
  final int nativeDataSpace;
  final double requestedHeadroom;
  final double activeHeadroom;
  final bool activeHeadroomKnown;
  final bool extendedLinearActive;
  final ErikaOutputFallbackReason fallbackReason;
  final int fallbackCount;
  final int dataSpaceFailures;
  final int headroomUpdates;
  final int extendedLinearFrames;

  factory ErikaOutputStatus.fromMap(Map<dynamic, dynamic> map) {
    return ErikaOutputStatus(
      requestedMode: ErikaOutputMode.fromNativeValue(
        (map['requestedMode'] as num?)?.toInt() ?? 0,
      ),
      activeEncoding: ErikaActiveOutputEncoding.fromNativeValue(
        (map['activeEncoding'] as num?)?.toInt() ?? 0,
      ),
      surfaceFormat: ErikaOutputSurfaceFormat.fromNativeValue(
        (map['surfaceFormat'] as num?)?.toInt() ?? 0,
      ),
      nativeDataSpace: (map['nativeDataSpace'] as num?)?.toInt() ?? -1,
      requestedHeadroom: (map['requestedHeadroom'] as num?)?.toDouble() ?? 1.0,
      activeHeadroom: (map['activeHeadroom'] as num?)?.toDouble() ?? 1.0,
      activeHeadroomKnown: map['activeHeadroomKnown'] == true,
      extendedLinearActive: map['extendedLinearActive'] == true,
      fallbackReason: ErikaOutputFallbackReason.fromNativeValue(
        (map['fallbackReason'] as num?)?.toInt() ?? 0,
      ),
      fallbackCount: (map['fallbackCount'] as num?)?.toInt() ?? 0,
      dataSpaceFailures: (map['dataSpaceFailures'] as num?)?.toInt() ?? 0,
      headroomUpdates: (map['headroomUpdates'] as num?)?.toInt() ?? 0,
      extendedLinearFrames: (map['extendedLinearFrames'] as num?)?.toInt() ?? 0,
    );
  }
}

enum ErikaUpscalerMode {
  off(0),
  artCnnC4F16(1),
  artCnnC4F32(2);

  const ErikaUpscalerMode(this.nativeValue);

  final int nativeValue;

  static ErikaUpscalerMode fromNativeValue(int value) {
    return switch (value) {
      1 => ErikaUpscalerMode.artCnnC4F16,
      2 => ErikaUpscalerMode.artCnnC4F32,
      _ => ErikaUpscalerMode.off,
    };
  }
}

enum ErikaUpscalerBackendStatus {
  off(0),
  inactive(1),
  building(2),
  scalar(3),
  simdgroupMatrix(4);

  const ErikaUpscalerBackendStatus(this.nativeValue);

  final int nativeValue;

  static ErikaUpscalerBackendStatus fromNativeValue(int value) {
    return switch (value) {
      1 => ErikaUpscalerBackendStatus.inactive,
      2 => ErikaUpscalerBackendStatus.building,
      3 => ErikaUpscalerBackendStatus.scalar,
      4 => ErikaUpscalerBackendStatus.simdgroupMatrix,
      _ => ErikaUpscalerBackendStatus.off,
    };
  }
}

class ErikaUpscalerStatus {
  const ErikaUpscalerStatus({
    required this.requestedMode,
    required this.activeBackend,
    required this.fallbackCount,
    required this.upscaledFrames,
    required this.lastEncodeDuration,
    required this.lastGpuDuration,
  });

  final ErikaUpscalerMode requestedMode;
  final ErikaUpscalerBackendStatus activeBackend;
  final int fallbackCount;
  final int upscaledFrames;
  final Duration lastEncodeDuration;
  final Duration lastGpuDuration;

  factory ErikaUpscalerStatus.fromMap(Map<dynamic, dynamic> map) {
    return ErikaUpscalerStatus(
      requestedMode: ErikaUpscalerMode.fromNativeValue(
        (map['requestedMode'] as num?)?.toInt() ?? 0,
      ),
      activeBackend: ErikaUpscalerBackendStatus.fromNativeValue(
        (map['activeBackend'] as num?)?.toInt() ?? 0,
      ),
      fallbackCount: (map['fallbackCount'] as num?)?.toInt() ?? 0,
      upscaledFrames: (map['upscaledFrames'] as num?)?.toInt() ?? 0,
      lastEncodeDuration: Duration(
        microseconds: (map['lastEncodeMicros'] as num?)?.toInt() ?? 0,
      ),
      lastGpuDuration: Duration(
        microseconds: (map['lastGpuMicros'] as num?)?.toInt() ?? 0,
      ),
    );
  }
}

class ErikaPresenterStats {
  const ErikaPresenterStats({
    required this.decodedVideoFrames,
    required this.renderedVideoFrames,
    required this.renderedTestFrames,
    required this.pushedAudioFrames,
    required this.overlayFrames,
    required this.danmakuFrames,
    required this.danmakuItems,
    required this.importFailures,
    required this.renderFailures,
    required this.audioFailures,
    required this.softwareVideoFrames,
    required this.hardwareVideoFrames,
    required this.zeroCopyVideoFrames,
    required this.cpuVideoFrameFallbacks,
    required this.lastRenderDuration,
    required this.lastRenderCurrentDuration,
    required this.audioClockReadFrames,
    required this.audioClockQueuedFrames,
    required this.audioClockUnderflowFrames,
    required this.audioRecoveryState,
    required this.audioLastErrorCode,
    required this.audioRecoveryAttempts,
    required this.audioRecoveryCount,
    required this.audioRecoveryFailures,
    required this.directZeroCopyVideoFrames,
    required this.sharedHandleVideoFrames,
    required this.hdrSourceFrames,
    required this.hdr10OutputFrames,
    required this.sdrTonemapFrames,
    required this.hdr10MetadataUpdates,
    required this.hdr10MetadataFailures,
    required this.hdr10OutputFailures,
    required this.hdr10OutputActive,
    required this.videoFrameBackpressureDrops,
  });

  final int decodedVideoFrames;
  final int renderedVideoFrames;
  final int renderedTestFrames;
  final int pushedAudioFrames;
  final int overlayFrames;
  final int danmakuFrames;
  final int danmakuItems;
  final int importFailures;
  final int renderFailures;
  final int audioFailures;
  final int softwareVideoFrames;
  final int hardwareVideoFrames;
  final int zeroCopyVideoFrames;
  final int cpuVideoFrameFallbacks;
  final Duration lastRenderDuration;
  final Duration lastRenderCurrentDuration;
  final int audioClockReadFrames;
  final int audioClockQueuedFrames;
  final int audioClockUnderflowFrames;
  final int audioRecoveryState;
  final int audioLastErrorCode;
  final int audioRecoveryAttempts;
  final int audioRecoveryCount;
  final int audioRecoveryFailures;
  final int directZeroCopyVideoFrames;
  final int sharedHandleVideoFrames;
  final int hdrSourceFrames;
  final int hdr10OutputFrames;
  final int sdrTonemapFrames;
  final int hdr10MetadataUpdates;
  final int hdr10MetadataFailures;
  final int hdr10OutputFailures;
  final bool hdr10OutputActive;
  final int videoFrameBackpressureDrops;

  factory ErikaPresenterStats.fromMap(Map<dynamic, dynamic> map) {
    return ErikaPresenterStats(
      decodedVideoFrames: _intValue(map['decodedVideoFrames']),
      renderedVideoFrames: _intValue(map['renderedVideoFrames']),
      renderedTestFrames: _intValue(map['renderedTestFrames']),
      pushedAudioFrames: _intValue(map['pushedAudioFrames']),
      overlayFrames: _intValue(map['overlayFrames']),
      danmakuFrames: _intValue(map['danmakuFrames']),
      danmakuItems: _intValue(map['danmakuItems']),
      importFailures: _intValue(map['importFailures']),
      renderFailures: _intValue(map['renderFailures']),
      audioFailures: _intValue(map['audioFailures']),
      softwareVideoFrames: _intValue(map['softwareVideoFrames']),
      hardwareVideoFrames: _intValue(map['hardwareVideoFrames']),
      zeroCopyVideoFrames: _intValue(map['zeroCopyVideoFrames']),
      cpuVideoFrameFallbacks: _intValue(map['cpuVideoFrameFallbacks']),
      lastRenderDuration: Duration(
        microseconds: _intValue(map['lastRenderMicros']),
      ),
      lastRenderCurrentDuration: Duration(
        microseconds: _intValue(map['lastRenderCurrentMicros']),
      ),
      audioClockReadFrames: _intValue(map['audioClockReadFrames']),
      audioClockQueuedFrames: _intValue(map['audioClockQueuedFrames']),
      audioClockUnderflowFrames: _intValue(map['audioClockUnderflowFrames']),
      audioRecoveryState: _intValue(map['audioRecoveryState']),
      audioLastErrorCode: _intValue(map['audioLastErrorCode']),
      audioRecoveryAttempts: _intValue(map['audioRecoveryAttempts']),
      audioRecoveryCount: _intValue(map['audioRecoveryCount']),
      audioRecoveryFailures: _intValue(map['audioRecoveryFailures']),
      directZeroCopyVideoFrames: _intValue(map['directZeroCopyVideoFrames']),
      sharedHandleVideoFrames: _intValue(map['sharedHandleVideoFrames']),
      hdrSourceFrames: _intValue(map['hdrSourceFrames']),
      hdr10OutputFrames: _intValue(map['hdr10OutputFrames']),
      sdrTonemapFrames: _intValue(map['sdrTonemapFrames']),
      hdr10MetadataUpdates: _intValue(map['hdr10MetadataUpdates']),
      hdr10MetadataFailures: _intValue(map['hdr10MetadataFailures']),
      hdr10OutputFailures: _intValue(map['hdr10OutputFailures']),
      hdr10OutputActive: map['hdr10OutputActive'] == true,
      videoFrameBackpressureDrops: _intValue(
        map['videoFrameBackpressureDrops'],
      ),
    );
  }

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'decodedVideoFrames': decodedVideoFrames,
      'renderedVideoFrames': renderedVideoFrames,
      'renderedTestFrames': renderedTestFrames,
      'pushedAudioFrames': pushedAudioFrames,
      'overlayFrames': overlayFrames,
      'danmakuFrames': danmakuFrames,
      'danmakuItems': danmakuItems,
      'importFailures': importFailures,
      'renderFailures': renderFailures,
      'audioFailures': audioFailures,
      'softwareVideoFrames': softwareVideoFrames,
      'hardwareVideoFrames': hardwareVideoFrames,
      'zeroCopyVideoFrames': zeroCopyVideoFrames,
      'cpuVideoFrameFallbacks': cpuVideoFrameFallbacks,
      'lastRenderMicros': lastRenderDuration.inMicroseconds,
      'lastRenderCurrentMicros': lastRenderCurrentDuration.inMicroseconds,
      'audioClockReadFrames': audioClockReadFrames,
      'audioClockQueuedFrames': audioClockQueuedFrames,
      'audioClockUnderflowFrames': audioClockUnderflowFrames,
      'audioRecoveryState': audioRecoveryState,
      'audioLastErrorCode': audioLastErrorCode,
      'audioRecoveryAttempts': audioRecoveryAttempts,
      'audioRecoveryCount': audioRecoveryCount,
      'audioRecoveryFailures': audioRecoveryFailures,
      'directZeroCopyVideoFrames': directZeroCopyVideoFrames,
      'sharedHandleVideoFrames': sharedHandleVideoFrames,
      'hdrSourceFrames': hdrSourceFrames,
      'hdr10OutputFrames': hdr10OutputFrames,
      'sdrTonemapFrames': sdrTonemapFrames,
      'hdr10MetadataUpdates': hdr10MetadataUpdates,
      'hdr10MetadataFailures': hdr10MetadataFailures,
      'hdr10OutputFailures': hdr10OutputFailures,
      'hdr10OutputActive': hdr10OutputActive,
      'videoFrameBackpressureDrops': videoFrameBackpressureDrops,
    };
  }

  static int _intValue(Object? value) {
    if (value is int) {
      return value;
    }
    if (value is num) {
      return value.toInt();
    }
    return 0;
  }
}

class ErikaDanmakuTrackInfo {
  const ErikaDanmakuTrackInfo({
    required this.id,
    required this.enabled,
    required this.offset,
    required this.itemCount,
    this.name,
    this.source,
  });

  final int id;
  final bool enabled;
  final Duration offset;
  final int itemCount;
  final String? name;
  final String? source;

  factory ErikaDanmakuTrackInfo.fromMap(Map<dynamic, dynamic> map) {
    return ErikaDanmakuTrackInfo(
      id: (map['id'] as num?)?.toInt() ?? 0,
      enabled: map['enabled'] == true,
      offset: Duration(
        microseconds: (map['offsetMicros'] as num?)?.toInt() ?? 0,
      ),
      itemCount: (map['itemCount'] as num?)?.toInt() ?? 0,
      name: map['name'] as String?,
      source: map['source'] as String?,
    );
  }
}

class _ErikaDanmakuConfigPatch {
  _ErikaDanmakuConfigPatch({
    this.enabled,
    this.fontSize,
    this.opacity,
    this.displayArea,
    this.scrollDurationSeconds,
    this.scrollSpeedFactor,
    this.trackGapRatio,
    this.outlineWidth,
    this.shadowOffsetX,
    this.shadowOffsetY,
    this.shadowStyle,
    this.customFontFamily,
    this.customFontFilePath,
    this.mergeDuplicates,
    this.allowStacking,
    this.allowScrollOverwrite,
    this.maxQuantity,
    this.maxLinesPerMode,
    this.blockTop,
    this.blockBottom,
    this.blockScroll,
    List<String>? blockWords,
  }) : blockWords = blockWords == null
           ? null
           : List<String>.unmodifiable(blockWords);

  final bool? enabled;
  final double? fontSize;
  final double? opacity;
  final double? displayArea;
  final double? scrollDurationSeconds;
  final double? scrollSpeedFactor;
  final double? trackGapRatio;
  final double? outlineWidth;
  final double? shadowOffsetX;
  final double? shadowOffsetY;
  final int? shadowStyle;
  final String? customFontFamily;
  final String? customFontFilePath;
  final bool? mergeDuplicates;
  final bool? allowStacking;
  final bool? allowScrollOverwrite;
  final int? maxQuantity;
  final int? maxLinesPerMode;
  final bool? blockTop;
  final bool? blockBottom;
  final bool? blockScroll;
  final List<String>? blockWords;

  bool get isEmpty =>
      enabled == null &&
      fontSize == null &&
      opacity == null &&
      displayArea == null &&
      scrollDurationSeconds == null &&
      scrollSpeedFactor == null &&
      trackGapRatio == null &&
      outlineWidth == null &&
      shadowOffsetX == null &&
      shadowOffsetY == null &&
      shadowStyle == null &&
      customFontFamily == null &&
      customFontFilePath == null &&
      mergeDuplicates == null &&
      allowStacking == null &&
      allowScrollOverwrite == null &&
      maxQuantity == null &&
      maxLinesPerMode == null &&
      blockTop == null &&
      blockBottom == null &&
      blockScroll == null &&
      blockWords == null;

  _ErikaDanmakuConfigPatch merge(_ErikaDanmakuConfigPatch other) {
    return _ErikaDanmakuConfigPatch(
      enabled: other.enabled ?? enabled,
      fontSize: other.fontSize ?? fontSize,
      opacity: other.opacity ?? opacity,
      displayArea: other.displayArea ?? displayArea,
      scrollDurationSeconds:
          other.scrollDurationSeconds ?? scrollDurationSeconds,
      scrollSpeedFactor: other.scrollSpeedFactor ?? scrollSpeedFactor,
      trackGapRatio: other.trackGapRatio ?? trackGapRatio,
      outlineWidth: other.outlineWidth ?? outlineWidth,
      shadowOffsetX: other.shadowOffsetX ?? shadowOffsetX,
      shadowOffsetY: other.shadowOffsetY ?? shadowOffsetY,
      shadowStyle: other.shadowStyle ?? shadowStyle,
      customFontFamily: other.customFontFamily ?? customFontFamily,
      customFontFilePath: other.customFontFilePath ?? customFontFilePath,
      mergeDuplicates: other.mergeDuplicates ?? mergeDuplicates,
      allowStacking: other.allowStacking ?? allowStacking,
      allowScrollOverwrite: other.allowScrollOverwrite ?? allowScrollOverwrite,
      maxQuantity: other.maxQuantity ?? maxQuantity,
      maxLinesPerMode: other.maxLinesPerMode ?? maxLinesPerMode,
      blockTop: other.blockTop ?? blockTop,
      blockBottom: other.blockBottom ?? blockBottom,
      blockScroll: other.blockScroll ?? blockScroll,
      blockWords: other.blockWords ?? blockWords,
    );
  }

  _ErikaDanmakuConfigPatch differenceFrom(_ErikaDanmakuConfigPatch? previous) {
    return _ErikaDanmakuConfigPatch(
      enabled: _changed(enabled, previous?.enabled) ? enabled : null,
      fontSize: _changed(fontSize, previous?.fontSize) ? fontSize : null,
      opacity: _changed(opacity, previous?.opacity) ? opacity : null,
      displayArea: _changed(displayArea, previous?.displayArea)
          ? displayArea
          : null,
      scrollDurationSeconds:
          _changed(scrollDurationSeconds, previous?.scrollDurationSeconds)
          ? scrollDurationSeconds
          : null,
      scrollSpeedFactor:
          _changed(scrollSpeedFactor, previous?.scrollSpeedFactor)
          ? scrollSpeedFactor
          : null,
      trackGapRatio: _changed(trackGapRatio, previous?.trackGapRatio)
          ? trackGapRatio
          : null,
      outlineWidth: _changed(outlineWidth, previous?.outlineWidth)
          ? outlineWidth
          : null,
      shadowOffsetX: _changed(shadowOffsetX, previous?.shadowOffsetX)
          ? shadowOffsetX
          : null,
      shadowOffsetY: _changed(shadowOffsetY, previous?.shadowOffsetY)
          ? shadowOffsetY
          : null,
      shadowStyle: _changed(shadowStyle, previous?.shadowStyle)
          ? shadowStyle
          : null,
      customFontFamily: _changed(customFontFamily, previous?.customFontFamily)
          ? customFontFamily
          : null,
      customFontFilePath:
          _changed(customFontFilePath, previous?.customFontFilePath)
          ? customFontFilePath
          : null,
      mergeDuplicates: _changed(mergeDuplicates, previous?.mergeDuplicates)
          ? mergeDuplicates
          : null,
      allowStacking: _changed(allowStacking, previous?.allowStacking)
          ? allowStacking
          : null,
      allowScrollOverwrite:
          _changed(allowScrollOverwrite, previous?.allowScrollOverwrite)
          ? allowScrollOverwrite
          : null,
      maxQuantity: _changed(maxQuantity, previous?.maxQuantity)
          ? maxQuantity
          : null,
      maxLinesPerMode: _changed(maxLinesPerMode, previous?.maxLinesPerMode)
          ? maxLinesPerMode
          : null,
      blockTop: _changed(blockTop, previous?.blockTop) ? blockTop : null,
      blockBottom: _changed(blockBottom, previous?.blockBottom)
          ? blockBottom
          : null,
      blockScroll: _changed(blockScroll, previous?.blockScroll)
          ? blockScroll
          : null,
      blockWords: _changedList(blockWords, previous?.blockWords)
          ? blockWords
          : null,
    );
  }

  Map<String, Object?> toArguments(int playerId) {
    return <String, Object?>{
      'playerId': playerId,
      if (enabled != null) 'enabled': enabled,
      if (fontSize != null) 'fontSize': fontSize,
      if (opacity != null) 'opacity': opacity,
      if (displayArea != null) 'displayArea': displayArea,
      if (scrollDurationSeconds != null)
        'scrollDurationSeconds': scrollDurationSeconds,
      if (scrollSpeedFactor != null) 'scrollSpeedFactor': scrollSpeedFactor,
      if (trackGapRatio != null) 'trackGapRatio': trackGapRatio,
      if (outlineWidth != null) 'outlineWidth': outlineWidth,
      if (shadowOffsetX != null) 'shadowOffsetX': shadowOffsetX,
      if (shadowOffsetY != null) 'shadowOffsetY': shadowOffsetY,
      if (shadowStyle != null) 'shadowStyle': shadowStyle,
      if (customFontFamily != null) 'customFontFamily': customFontFamily,
      if (customFontFilePath != null) 'customFontFilePath': customFontFilePath,
      if (mergeDuplicates != null) 'mergeDuplicates': mergeDuplicates,
      if (allowStacking != null) 'allowStacking': allowStacking,
      if (allowScrollOverwrite != null)
        'allowScrollOverwrite': allowScrollOverwrite,
      if (maxQuantity != null) 'maxQuantity': maxQuantity,
      if (maxLinesPerMode != null) 'maxLinesPerMode': maxLinesPerMode,
      if (blockTop != null) 'blockTop': blockTop,
      if (blockBottom != null) 'blockBottom': blockBottom,
      if (blockScroll != null) 'blockScroll': blockScroll,
      if (blockWords != null) 'blockWordsJson': jsonEncode(blockWords),
    };
  }

  static bool _changed<T>(T? value, T? previous) =>
      value != null && value != previous;

  static bool _changedList(List<String>? value, List<String>? previous) =>
      value != null && !listEquals(value, previous);
}

class ErikaPlayer {
  ErikaPlayer({
    this.outputMode,
    this.edrHeadroom,
    this.upscaler,
    this.hdrDebug = false,
  }) {
    final headroom = edrHeadroom;
    if (headroom != null &&
        (!headroom.isFinite || headroom < 1.0 || headroom > 10000.0)) {
      throw ArgumentError.value(
        headroom,
        'edrHeadroom',
        'must be finite and in [1, 10000]; omit it for system-auto headroom',
      );
    }
    _eventSubscription ??= _events.receiveBroadcastStream().listen(
      _dispatchNativeEvent,
      onError: (Object error, StackTrace stackTrace) {
        debugPrint('ErikaPlayer event stream error: $error');
      },
    );
  }

  static const MethodChannel _channel = MethodChannel('erika_flutter/player');
  static const EventChannel _events = EventChannel('erika_flutter/events');
  static const int windowOverlayViewId = -1;
  static final Map<int, StreamController<ErikaPlayerEvent>> _controllers =
      <int, StreamController<ErikaPlayerEvent>>{};
  static StreamSubscription<dynamic>? _eventSubscription;

  int? _id;
  Future<int>? _createFuture;
  Future<void>? _disposeFuture;
  bool _disposed = false;
  static const Duration _danmakuConfigCoalesceDelay = Duration(
    milliseconds: 50,
  );
  Timer? _danmakuConfigTimer;
  bool _danmakuConfigInFlight = false;
  _ErikaDanmakuConfigPatch? _pendingDanmakuConfig;
  _ErikaDanmakuConfigPatch? _lastAppliedDanmakuConfig;
  final List<Completer<void>> _pendingDanmakuConfigCompleters =
      <Completer<void>>[];

  final ErikaOutputMode? outputMode;
  final double? edrHeadroom;
  final ErikaUpscalerMode? upscaler;
  final bool hdrDebug;

  int? get id => _id;

  Stream<ErikaPlayerEvent> get events async* {
    final playerId = await ensureCreated();
    yield* _controllerFor(playerId).stream;
  }

  Future<int> ensureCreated() {
    if (_disposed) {
      throw StateError('ErikaPlayer has been disposed.');
    }
    final existing = _id;
    final player = existing != null
        ? Future<int>.value(existing)
        : (_createFuture ??= _create());
    return _requireActiveAfter(player);
  }

  Future<void> open(String uri, {Map<String, String>? httpHeaders}) async {
    final playerId = await ensureCreated();
    await _invoke('open', <String, Object?>{
      'playerId': playerId,
      'uri': uri,
      if (httpHeaders != null && httpHeaders.isNotEmpty)
        'httpHeaders': httpHeaders,
    });
  }

  Future<void> play() async {
    await _invokeForPlayer('play');
  }

  Future<void> pause() async {
    await _invokeForPlayer('pause');
  }

  Future<void> stop() async {
    await _invokeForPlayer('stop');
  }

  Future<void> close() async {
    await _invokeForPlayer('close');
  }

  Future<void> seek(Duration position) async {
    final playerId = await ensureCreated();
    await _invoke('seek', <String, Object?>{
      'playerId': playerId,
      'positionMicros': position.inMicroseconds,
    });
  }

  Future<void> setPlaybackRate(double rate) async {
    final playerId = await ensureCreated();
    await _invoke('setPlaybackRate', <String, Object?>{
      'playerId': playerId,
      'rate': rate,
    });
  }

  Future<void> setVolume(double volume) async {
    final playerId = await ensureCreated();
    await _invoke('setVolume', <String, Object?>{
      'playerId': playerId,
      'volume': volume.clamp(0.0, 1.0),
    });
  }

  Future<void> setUpscaler(ErikaUpscalerMode mode) async {
    final playerId = await ensureCreated();
    await _invoke('setUpscaler', <String, Object?>{
      'playerId': playerId,
      'mode': mode.nativeValue,
    });
  }

  Future<void> setSubtitleScale(double scale) async {
    final playerId = await ensureCreated();
    final clampedScale = scale.isFinite ? scale.clamp(0.25, 4.0) : 1.0;
    await _invoke('setSubtitleScale', <String, Object?>{
      'playerId': playerId,
      'scale': clampedScale,
    });
  }

  Future<ErikaUpscalerStatus> getUpscalerStatus() async {
    final playerId = await ensureCreated();
    final status = await _channel.invokeMethod<Map<dynamic, dynamic>>(
      'getUpscalerStatus',
      <String, Object?>{'playerId': playerId},
    );
    if (status == null) {
      throw StateError('Erika upscaler status returned null.');
    }
    return ErikaUpscalerStatus.fromMap(status);
  }

  Future<ErikaOutputStatus> getOutputStatus() async {
    final playerId = await ensureCreated();
    final status = await _channel.invokeMethod<Map<dynamic, dynamic>>(
      'getOutputStatus',
      <String, Object?>{'playerId': playerId},
    );
    if (status == null) {
      throw StateError('Erika output status returned null.');
    }
    return ErikaOutputStatus.fromMap(status);
  }

  Future<ErikaPresenterStats> getPresenterStats() async {
    final playerId = await ensureCreated();
    final stats = await _channel.invokeMethod<Map<dynamic, dynamic>>(
      'getPresenterStats',
      <String, Object?>{'playerId': playerId},
    );
    if (stats == null) {
      throw StateError('Erika presenter stats returned null.');
    }
    return ErikaPresenterStats.fromMap(stats);
  }

  Future<Uint8List?> screenshot({int? viewId, int? width, int? height}) async {
    final playerId = await ensureCreated();
    return _channel.invokeMethod<Uint8List>('screenshot', <String, Object?>{
      'playerId': playerId,
      if (viewId != null) 'viewId': viewId,
      if (width != null) 'width': width,
      if (height != null) 'height': height,
    });
  }

  Future<int> addExternalSubtitle(String uri) async {
    final playerId = await ensureCreated();
    final trackId = await _channel.invokeMethod<int>(
      'addExternalSubtitle',
      <String, Object?>{'playerId': playerId, 'uri': uri},
    );
    if (trackId == null) {
      throw StateError('Erika external subtitle add returned no track id.');
    }
    return trackId;
  }

  Future<void> removeSubtitleTrack(int trackId) async {
    final playerId = await ensureCreated();
    await _invoke('removeSubtitleTrack', <String, Object?>{
      'playerId': playerId,
      'trackId': trackId,
    });
  }

  Future<void> loadDanmakuFile(String uri) async {
    final playerId = await ensureCreated();
    await _invoke('loadDanmakuFile', <String, Object?>{
      'playerId': playerId,
      'uri': uri,
    });
  }

  Future<void> loadDanmakuJson(String json) async {
    final playerId = await ensureCreated();
    await _invoke('loadDanmakuJson', <String, Object?>{
      'playerId': playerId,
      'json': json,
    });
  }

  Future<int> addDanmakuTrackFile(
    String uri, {
    String? name,
    Duration offset = Duration.zero,
  }) async {
    final playerId = await ensureCreated();
    final trackId = await _channel
        .invokeMethod<int>('addDanmakuTrackFile', <String, Object?>{
          'playerId': playerId,
          'uri': uri,
          if (name != null) 'name': name,
          'offsetMicros': offset.inMicroseconds,
        });
    if (trackId == null || trackId <= 0) {
      throw StateError('Erika danmaku track add returned no track id.');
    }
    return trackId;
  }

  Future<int> addDanmakuTrackJson(
    String json, {
    String? name,
    Duration offset = Duration.zero,
  }) async {
    final playerId = await ensureCreated();
    final trackId = await _channel
        .invokeMethod<int>('addDanmakuTrackJson', <String, Object?>{
          'playerId': playerId,
          'json': json,
          if (name != null) 'name': name,
          'offsetMicros': offset.inMicroseconds,
        });
    if (trackId == null || trackId <= 0) {
      throw StateError('Erika danmaku track add returned no track id.');
    }
    return trackId;
  }

  Future<void> removeDanmakuTrack(int trackId) async {
    final playerId = await ensureCreated();
    await _invoke('removeDanmakuTrack', <String, Object?>{
      'playerId': playerId,
      'trackId': trackId,
    });
  }

  Future<void> setDanmakuTrackEnabled(int trackId, bool enabled) async {
    final playerId = await ensureCreated();
    await _invoke('setDanmakuTrackEnabled', <String, Object?>{
      'playerId': playerId,
      'trackId': trackId,
      'enabled': enabled,
    });
  }

  Future<void> setDanmakuTrackOffset(int trackId, Duration offset) async {
    final playerId = await ensureCreated();
    await _invoke('setDanmakuTrackOffset', <String, Object?>{
      'playerId': playerId,
      'trackId': trackId,
      'offsetMicros': offset.inMicroseconds,
    });
  }

  Future<void> setDanmakuGlobalOffset(Duration offset) async {
    final playerId = await ensureCreated();
    await _invoke('setDanmakuGlobalOffset', <String, Object?>{
      'playerId': playerId,
      'offsetMicros': offset.inMicroseconds,
    });
  }

  Future<List<ErikaDanmakuTrackInfo>> danmakuTracks() async {
    final playerId = await ensureCreated();
    final rawTracks = await _channel.invokeMethod<List<dynamic>>(
      'danmakuTracks',
      <String, Object?>{'playerId': playerId},
    );
    if (rawTracks == null) {
      return const <ErikaDanmakuTrackInfo>[];
    }
    return rawTracks
        .whereType<Map<dynamic, dynamic>>()
        .map(ErikaDanmakuTrackInfo.fromMap)
        .toList(growable: false);
  }

  Future<void> clearDanmaku() async {
    await _invokeForPlayer('clearDanmaku');
  }

  Future<void> setDanmakuEnabled(bool enabled) async {
    final playerId = await ensureCreated();
    await _invoke('setDanmakuEnabled', <String, Object?>{
      'playerId': playerId,
      'enabled': enabled,
    });
  }

  Future<void> setDanmakuConfig({
    bool? enabled,
    // NipaPlay/Flutter logical danmaku font size. Erika uses the NipaPlay
    // default danmaku font and applies the native surface scale internally.
    double? fontSize,
    double? opacity,
    double? displayArea,
    double? scrollDurationSeconds,
    double? scrollSpeedFactor,
    double? trackGapRatio,
    double? outlineWidth,
    double? shadowOffsetX,
    double? shadowOffsetY,
    int? shadowStyle,
    String? customFontFamily,
    String? customFontFilePath,
    bool? mergeDuplicates,
    bool? allowStacking,
    bool? allowScrollOverwrite,
    int? maxQuantity,
    int? maxLinesPerMode,
    bool? blockTop,
    bool? blockBottom,
    bool? blockScroll,
    List<String>? blockWords,
  }) async {
    if (_disposed) {
      return;
    }
    final playerId = await ensureCreated();
    if (enabled != null) {
      // Visibility changes are presentation-critical. Do not hold them behind
      // the style coalescing timer; the native presenter can hide immediately.
      await _invoke('setDanmakuEnabled', <String, Object?>{
        'playerId': playerId,
        'enabled': enabled,
      });
    }
    final patch = _ErikaDanmakuConfigPatch(
      enabled: null,
      fontSize: fontSize,
      opacity: opacity,
      displayArea: displayArea,
      scrollDurationSeconds: scrollDurationSeconds,
      scrollSpeedFactor: scrollSpeedFactor,
      trackGapRatio: trackGapRatio,
      outlineWidth: outlineWidth,
      shadowOffsetX: shadowOffsetX,
      shadowOffsetY: shadowOffsetY,
      shadowStyle: shadowStyle,
      customFontFamily: customFontFamily,
      customFontFilePath: customFontFilePath,
      mergeDuplicates: mergeDuplicates,
      allowStacking: allowStacking,
      allowScrollOverwrite: allowScrollOverwrite,
      maxQuantity: maxQuantity,
      maxLinesPerMode: maxLinesPerMode,
      blockTop: blockTop,
      blockBottom: blockBottom,
      blockScroll: blockScroll,
      blockWords: blockWords,
    );
    if (patch.isEmpty) {
      return;
    }

    final completer = Completer<void>();
    _pendingDanmakuConfig = _pendingDanmakuConfig?.merge(patch) ?? patch;
    _pendingDanmakuConfigCompleters.add(completer);
    _scheduleDanmakuConfigFlush(playerId);
    return completer.future;
  }

  void _scheduleDanmakuConfigFlush(int playerId) {
    if (_disposed || _danmakuConfigInFlight || _danmakuConfigTimer != null) {
      return;
    }
    _danmakuConfigTimer = Timer(_danmakuConfigCoalesceDelay, () {
      _danmakuConfigTimer = null;
      unawaited(_flushDanmakuConfig(playerId));
    });
  }

  Future<void> _flushDanmakuConfig(int playerId) async {
    if (_disposed || _danmakuConfigInFlight) {
      return;
    }

    final requestedPatch = _pendingDanmakuConfig;
    if (requestedPatch == null) {
      return;
    }
    final completers = List<Completer<void>>.of(
      _pendingDanmakuConfigCompleters,
    );
    _pendingDanmakuConfigCompleters.clear();
    _pendingDanmakuConfig = null;

    final outgoingPatch = requestedPatch.differenceFrom(
      _lastAppliedDanmakuConfig,
    );
    if (outgoingPatch.isEmpty) {
      for (final completer in completers) {
        if (!completer.isCompleted) {
          completer.complete();
        }
      }
      if (_pendingDanmakuConfig != null) {
        _scheduleDanmakuConfigFlush(playerId);
      }
      return;
    }

    _danmakuConfigInFlight = true;
    try {
      await _invoke('setDanmakuConfig', outgoingPatch.toArguments(playerId));
      _lastAppliedDanmakuConfig =
          _lastAppliedDanmakuConfig?.merge(requestedPatch) ?? requestedPatch;
      for (final completer in completers) {
        if (!completer.isCompleted) {
          completer.complete();
        }
      }
    } catch (error, stackTrace) {
      for (final completer in completers) {
        if (!completer.isCompleted) {
          completer.completeError(error, stackTrace);
        }
      }
    } finally {
      _danmakuConfigInFlight = false;
      if (_pendingDanmakuConfig != null) {
        _scheduleDanmakuConfigFlush(playerId);
      }
    }
  }

  Future<void> selectAudioTrack(int? trackId) async {
    final playerId = await ensureCreated();
    await _invoke('selectAudioTrack', <String, Object?>{
      'playerId': playerId,
      'trackId': trackId,
    });
  }

  Future<void> selectSubtitleTrack(int? trackId) async {
    final playerId = await ensureCreated();
    await _invoke('selectSubtitleTrack', <String, Object?>{
      'playerId': playerId,
      'trackId': trackId,
    });
  }

  Future<List<ErikaTrackInfo>> tracks() async {
    final playerId = await ensureCreated();
    final rawTracks = await _channel.invokeMethod<List<dynamic>>(
      'tracks',
      <String, Object?>{'playerId': playerId},
    );
    if (rawTracks == null) {
      return const <ErikaTrackInfo>[];
    }
    return rawTracks
        .whereType<Map<dynamic, dynamic>>()
        .map(ErikaTrackInfo.fromMap)
        .toList(growable: false);
  }

  Future<void> attachView(int viewId) async {
    final playerId = await ensureCreated();
    await _invoke('attachView', <String, Object?>{
      'playerId': playerId,
      'viewId': viewId,
    });
  }

  Future<void> detachView(int viewId) async {
    final playerId = _id;
    if (playerId == null || _disposed) {
      return;
    }
    await _invoke('detachView', <String, Object?>{
      'playerId': playerId,
      'viewId': viewId,
    });
  }

  Future<int> attachWindowOverlay() async {
    final playerId = await ensureCreated();
    final viewId = await _channel.invokeMethod<int>(
      'attachOverlay',
      <String, Object?>{'playerId': playerId},
    );
    return viewId ?? windowOverlayViewId;
  }

  Future<void> detachWindowOverlay({int? generation}) async {
    final playerId = _id;
    if (playerId == null || _disposed) {
      return;
    }
    await _invoke('detachOverlay', <String, Object?>{
      'playerId': playerId,
      if (generation != null) 'generation': generation,
    });
  }

  Future<void> setWindowOverlayFrame({
    required Rect frame,
    required bool visible,
    required int generation,
    String? debugLabel,
  }) async {
    final playerId = await ensureCreated();
    await _invoke('setOverlayFrame', <String, Object?>{
      'playerId': playerId,
      'viewId': windowOverlayViewId,
      'generation': generation,
      'x': frame.left,
      'y': frame.top,
      'width': frame.width,
      'height': frame.height,
      'visible': visible,
      if (debugLabel != null) 'debugLabel': debugLabel,
    });
  }

  Future<void> dispose() {
    final existing = _disposeFuture;
    if (existing != null) {
      return existing;
    }
    _disposed = true;
    return _disposeFuture = _dispose();
  }

  Future<void> _dispose() async {
    _danmakuConfigTimer?.cancel();
    _danmakuConfigTimer = null;
    for (final completer in _pendingDanmakuConfigCompleters) {
      if (!completer.isCompleted) {
        completer.complete();
      }
    }
    _pendingDanmakuConfigCompleters.clear();
    _pendingDanmakuConfig = null;

    final createFuture = _createFuture;
    if (createFuture != null) {
      try {
        await createFuture;
      } catch (_) {
        // Creation callers retain the original error. Disposal only needs to
        // clean up a native player if creation produced one.
      }
    }

    final playerId = _id;
    _id = null;
    _createFuture = null;
    if (playerId == null) {
      return;
    }
    try {
      await _invoke('dispose', <String, Object?>{'playerId': playerId});
    } finally {
      final controller = _controllers.remove(playerId);
      await controller?.close();
    }
  }

  Future<int> _create() async {
    final requestedHeadroom =
        edrHeadroom ??
        (outputMode == ErikaOutputMode.extendedLinear ? 4.0 : null);
    final arguments = <String, Object?>{
      if (outputMode case final mode?) 'outputMode': mode.nativeValue,
      if (requestedHeadroom case final headroom?) 'edrHeadroom': headroom,
      if (upscaler case final mode?) 'upscaler': mode.nativeValue,
      if (hdrDebug) 'hdrDebug': true,
    };
    if (hdrDebug) {
      debugPrint('ErikaHDR[Dart]: create arguments=$arguments');
    }
    final playerId = await _channel.invokeMethod<int>('create', arguments);
    if (playerId == null || playerId <= 0) {
      throw StateError('Erika presenter creation failed.');
    }
    _id = playerId;
    _controllerFor(playerId);
    return playerId;
  }

  Future<int> _requireActiveAfter(Future<int> player) async {
    final playerId = await player;
    if (_disposed) {
      throw StateError('ErikaPlayer has been disposed.');
    }
    return playerId;
  }

  Future<void> _invokeForPlayer(String method) async {
    final playerId = await ensureCreated();
    await _invoke(method, <String, Object?>{'playerId': playerId});
  }

  Future<void> _invoke(String method, Map<String, Object?> arguments) async {
    await _channel.invokeMethod<void>(method, arguments);
  }

  static StreamController<ErikaPlayerEvent> _controllerFor(int playerId) {
    return _controllers.putIfAbsent(
      playerId,
      () => StreamController<ErikaPlayerEvent>.broadcast(),
    );
  }

  static void _dispatchNativeEvent(dynamic rawEvent) {
    if (rawEvent is! Map) {
      return;
    }
    final event = ErikaPlayerEvent.fromMap(rawEvent);
    final controller = _controllers[event.playerId];
    controller?.add(event);
  }
}
