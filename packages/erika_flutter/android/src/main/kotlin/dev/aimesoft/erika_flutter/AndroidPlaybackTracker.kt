package dev.aimesoft.erika_flutter

internal enum class AndroidPlaybackPhase {
    PAUSED,
    PENDING,
    PLAYING,
}

/**
 * Main-thread playback and render intent for one Android player.
 *
 * Native playback state remains owned by Erika. This tracker records only the
 * Android host's intent, including whether a delayed focus gain may resume the
 * player and whether the attached surface needs another render tick.
 */
internal class AndroidPlaybackTracker {
    var phase: AndroidPlaybackPhase = AndroidPlaybackPhase.PAUSED
        private set

    var surfaceAttached: Boolean = false
        private set

    var renderRequested: Boolean = false
        private set

    val shouldTick: Boolean
        get() = phase == AndroidPlaybackPhase.PLAYING ||
            (surfaceAttached && renderRequested)

    fun requestPlayback() {
        phase = AndroidPlaybackPhase.PENDING
    }

    fun playbackStarted(): Boolean {
        if (phase != AndroidPlaybackPhase.PENDING) {
            return false
        }
        phase = AndroidPlaybackPhase.PLAYING
        renderRequested = true
        return true
    }

    /** Returns true when native playback was running and must be paused. */
    fun suspendPlayback(): Boolean {
        val wasPlaying = phase == AndroidPlaybackPhase.PLAYING
        if (phase != AndroidPlaybackPhase.PAUSED) {
            phase = AndroidPlaybackPhase.PENDING
        }
        return wasPlaying
    }

    /** Returns true when native playback was running and must be paused. */
    fun handleFocusLoss(mayResume: Boolean): Boolean {
        val wasPlaying = phase == AndroidPlaybackPhase.PLAYING
        phase = if (mayResume && phase != AndroidPlaybackPhase.PAUSED) {
            AndroidPlaybackPhase.PENDING
        } else {
            AndroidPlaybackPhase.PAUSED
        }
        return wasPlaying
    }

    /** Returns true when native playback was running and must be paused. */
    fun cancelPlaybackIntent(): Boolean {
        val wasPlaying = phase == AndroidPlaybackPhase.PLAYING
        phase = AndroidPlaybackPhase.PAUSED
        return wasPlaying
    }

    fun attachSurface() {
        surfaceAttached = true
        renderRequested = true
    }

    fun resizeSurface() {
        if (surfaceAttached) {
            renderRequested = true
        }
    }

    fun detachSurface() {
        surfaceAttached = false
    }

    fun requestRender() {
        renderRequested = true
    }

    fun markRenderAttempted() {
        renderRequested = false
    }
}
