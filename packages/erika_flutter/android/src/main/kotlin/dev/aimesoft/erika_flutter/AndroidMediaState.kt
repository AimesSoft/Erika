package dev.aimesoft.erika_flutter

internal data class AndroidMediaMetadata(
    val title: String,
    val artist: String?,
    val album: String?,
    val artwork: ByteArray?,
)

internal data class AndroidMediaState(
    val playerId: Long,
    val metadata: AndroidMediaMetadata? = null,
    val playbackState: Int = 0,
    val positionMicros: Long = 0L,
    val durationMicros: Long = 0L,
    val playbackRate: Float = 1f,
    val allowBackgroundPlayback: Boolean = false,
)

internal fun AndroidMediaState.canPlay(activityActive: Boolean): Boolean =
    activityActive || allowBackgroundPlayback

internal fun androidMediaMetadata(arguments: Map<String, Any?>): AndroidMediaMetadata {
    val raw = arguments["metadata"] as? Map<*, *>
        ?: throw IllegalArgumentException("metadata is required")
    val title = (raw["title"] as? String)?.trim().orEmpty()
    require(title.isNotEmpty()) { "metadata.title is required" }
    return AndroidMediaMetadata(
        title = title,
        artist = (raw["artist"] as? String)?.takeIf(String::isNotBlank),
        album = (raw["album"] as? String)?.takeIf(String::isNotBlank),
        artwork = raw["artwork"] as? ByteArray,
    )
}

internal fun updatedAndroidMediaState(
    state: AndroidMediaState,
    event: Map<*, *>,
): AndroidMediaState {
    val kind = (event["kind"] as? Number)?.toInt()
    return state.copy(
        playbackState = if (kind == STATE_CHANGED_EVENT_KIND) {
            (event["state"] as? Number)?.toInt() ?: state.playbackState
        } else {
            state.playbackState
        },
        positionMicros = if (kind == 3) {
            ((event["positionMicros"] as? Number)?.toLong() ?: state.positionMicros).coerceAtLeast(0L)
        } else {
            state.positionMicros
        },
        durationMicros = if (kind == 2 || kind == STATE_CHANGED_EVENT_KIND) {
            ((event["durationMicros"] as? Number)?.toLong() ?: state.durationMicros).coerceAtLeast(0L)
        } else {
            state.durationMicros
        },
    )
}
