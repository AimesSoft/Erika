enum ErikaPlaybackState {
  idle,
  opening,
  ready,
  playing,
  paused,
  stopped,
  closed,
  error,
}

enum ErikaEventKind {
  none,
  stateChanged,
  durationChanged,
  positionChanged,
  tracksChanged,
  bufferingChanged,
  videoParamsChanged,
  surfaceAttached,
  surfaceDetached,
  error,
  trackSelectionChanged,
}

enum ErikaTrackKind {
  video,
  audio,
  subtitle,
}

enum ErikaTrackSource {
  embedded,
  external,
}

class ErikaVideoParams {
  const ErikaVideoParams({
    required this.width,
    required this.height,
    required this.primaries,
    required this.transfer,
  });

  factory ErikaVideoParams.fromMap(Map<dynamic, dynamic>? map) {
    return ErikaVideoParams(
      width: _asInt(map?['width']),
      height: _asInt(map?['height']),
      primaries: _asInt(map?['primaries']),
      transfer: _asInt(map?['transfer']),
    );
  }

  final int width;
  final int height;
  final int primaries;
  final int transfer;

  static int _asInt(Object? value) {
    if (value is int) {
      return value;
    }
    if (value is num) {
      return value.toInt();
    }
    return 0;
  }
}

class ErikaTrackCounts {
  const ErikaTrackCounts({
    required this.video,
    required this.audio,
    required this.subtitle,
  });

  factory ErikaTrackCounts.fromMap(Map<dynamic, dynamic>? map) {
    return ErikaTrackCounts(
      video: _asInt(map?['video']),
      audio: _asInt(map?['audio']),
      subtitle: _asInt(map?['subtitle']),
    );
  }

  final int video;
  final int audio;
  final int subtitle;

  static int _asInt(Object? value) {
    if (value is int) {
      return value;
    }
    if (value is num) {
      return value.toInt();
    }
    return 0;
  }
}

class ErikaTrackSelection {
  const ErikaTrackSelection({
    this.video,
    this.audio,
    this.subtitle,
  });

  factory ErikaTrackSelection.fromMap(Map<dynamic, dynamic>? map) {
    return ErikaTrackSelection(
      video: _trackId(map?['video']),
      audio: _trackId(map?['audio']),
      subtitle: _trackId(map?['subtitle']),
    );
  }

  final int? video;
  final int? audio;
  final int? subtitle;

  static int? _trackId(Object? value) {
    final id = _asInt(value);
    return id >= 0 ? id : null;
  }

  static int _asInt(Object? value) {
    if (value is int) {
      return value;
    }
    if (value is num) {
      return value.toInt();
    }
    return -1;
  }
}

class ErikaTrackInfo {
  const ErikaTrackInfo({
    required this.id,
    required this.kind,
    required this.source,
    required this.selected,
    required this.canRemove,
    this.title,
    this.language,
    this.codec,
    this.width = 0,
    this.height = 0,
    this.sampleRate = 0,
    this.channels = 0,
    this.pixelFormat,
    this.sampleFormat,
    this.profile,
    this.level = 0,
  });

  factory ErikaTrackInfo.fromMap(Map<dynamic, dynamic> map) {
    return ErikaTrackInfo(
      id: _asInt(map['id']),
      kind: _trackKindFromIndex(_asInt(map['kind'])),
      source: _trackSourceFromIndex(_asInt(map['source'])),
      selected: map['selected'] == true,
      canRemove: map['canRemove'] == true,
      title: map['title'] as String?,
      language: map['language'] as String?,
      codec: map['codec'] as String?,
      width: _asInt(map['width']),
      height: _asInt(map['height']),
      sampleRate: _asInt(map['sampleRate']),
      channels: _asInt(map['channels']),
      pixelFormat: map['pixelFormat'] as String?,
      sampleFormat: map['sampleFormat'] as String?,
      profile: map['profile'] as String?,
      level: _asInt(map['level']),
    );
  }

  final int id;
  final ErikaTrackKind kind;
  final ErikaTrackSource source;
  final bool selected;
  final bool canRemove;
  final String? title;
  final String? language;
  final String? codec;
  final int width;
  final int height;
  final int sampleRate;
  final int channels;
  final String? pixelFormat;
  final String? sampleFormat;
  final String? profile;
  final int level;

  Map<String, Object?> toMap() {
    return <String, Object?>{
      'id': id,
      'kind': kind.name,
      'source': source.name,
      'selected': selected,
      'canRemove': canRemove,
      'title': title,
      'language': language,
      'codec': codec,
      'width': width,
      'height': height,
      'sampleRate': sampleRate,
      'channels': channels,
      'pixelFormat': pixelFormat,
      'sampleFormat': sampleFormat,
      'profile': profile,
      'level': level,
    };
  }

  bool get isEmbedded => source == ErikaTrackSource.embedded;
  bool get isExternal => source == ErikaTrackSource.external;

  static int _asInt(Object? value) {
    if (value is int) {
      return value;
    }
    if (value is num) {
      return value.toInt();
    }
    return 0;
  }

  static ErikaTrackKind _trackKindFromIndex(int index) {
    if (index >= 0 && index < ErikaTrackKind.values.length) {
      return ErikaTrackKind.values[index];
    }
    return ErikaTrackKind.video;
  }

  static ErikaTrackSource _trackSourceFromIndex(int index) {
    if (index >= 0 && index < ErikaTrackSource.values.length) {
      return ErikaTrackSource.values[index];
    }
    return ErikaTrackSource.embedded;
  }
}

class ErikaPlayerEvent {
  const ErikaPlayerEvent({
    required this.playerId,
    required this.kind,
    required this.state,
    required this.duration,
    required this.position,
    required this.buffering,
    required this.video,
    required this.tracks,
    required this.trackList,
    required this.trackSelection,
    this.status = 0,
  });

  factory ErikaPlayerEvent.fromMap(Map<dynamic, dynamic> map) {
    return ErikaPlayerEvent(
      playerId: _asInt(map['playerId']),
      kind: _eventKindFromIndex(_asInt(map['kind'])),
      state: _stateFromIndex(_asInt(map['state'])),
      duration: Duration(microseconds: _asInt(map['durationMicros'])),
      position: Duration(microseconds: _asInt(map['positionMicros'])),
      buffering: map['buffering'] == true,
      video: ErikaVideoParams.fromMap(map['video'] as Map<dynamic, dynamic>?),
      tracks: ErikaTrackCounts.fromMap(
        map['tracks'] as Map<dynamic, dynamic>?,
      ),
      trackList: _trackListFromValue(map['trackList']),
      trackSelection: ErikaTrackSelection.fromMap(
        map['trackSelection'] as Map<dynamic, dynamic>?,
      ),
      status: _asInt(map['status']),
    );
  }

  final int playerId;
  final ErikaEventKind kind;
  final ErikaPlaybackState state;
  final Duration duration;
  final Duration position;
  final bool buffering;
  final ErikaVideoParams video;
  final ErikaTrackCounts tracks;
  final List<ErikaTrackInfo> trackList;
  final ErikaTrackSelection trackSelection;
  final int status;

  static int _asInt(Object? value) {
    if (value is int) {
      return value;
    }
    if (value is num) {
      return value.toInt();
    }
    return 0;
  }

  static ErikaEventKind _eventKindFromIndex(int index) {
    if (index >= 0 && index < ErikaEventKind.values.length) {
      return ErikaEventKind.values[index];
    }
    return ErikaEventKind.none;
  }

  static ErikaPlaybackState _stateFromIndex(int index) {
    if (index >= 0 && index < ErikaPlaybackState.values.length) {
      return ErikaPlaybackState.values[index];
    }
    return ErikaPlaybackState.error;
  }

  static List<ErikaTrackInfo> _trackListFromValue(Object? value) {
    if (value is! List) {
      return const <ErikaTrackInfo>[];
    }
    return value
        .whereType<Map<dynamic, dynamic>>()
        .map(ErikaTrackInfo.fromMap)
        .toList(growable: false);
  }
}
