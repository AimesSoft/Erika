package dev.aimesoft.erika_flutter

import android.view.Surface

internal class AndroidPlayerHost(
    val handle: Long,
    val requestedOutputMode: Int,
    allowBackgroundPlayback: Boolean,
) {
    val requiresExtendedLinearSurface: Boolean
        get() = requestedOutputMode == 2
    var attachedView: ErikaAndroidVideoView? = null
    private val playbackTracker = AndroidPlaybackTracker()
    private val contentPreparations = AndroidContentPreparationRegistry()
    private val pendingEvents = AndroidPendingEventQueue(MAX_PENDING_EVENTS)
    var mediaState = AndroidMediaState(
        playerId = handle,
        allowBackgroundPlayback = allowBackgroundPlayback,
    )
        private set
    val playbackPhase: AndroidPlaybackPhase
        get() = playbackTracker.phase
    val surfaceAttached: Boolean
        get() = playbackTracker.surfaceAttached
    val shouldTick: Boolean
        get() = playbackTracker.shouldTick
    var lastRenderError: String? = null
    var lastSurfaceError: String? = null
    private var destroyed = false

    val isDestroyed: Boolean
        get() = destroyed

    fun enqueuePendingEvent(event: AndroidPendingEvent): AndroidPendingEventOverflow? =
        pendingEvents.enqueue(event)

    fun firstPendingEvent(): AndroidPendingEvent? = pendingEvents.firstOrNull()

    fun removeFirstPendingEvent(): AndroidPendingEvent = pendingEvents.removeFirst()

    fun requestPlayback() = playbackTracker.requestPlayback()

    fun playbackStarted(): Boolean = playbackTracker.playbackStarted()

    fun suspendPlayback(): Boolean = playbackTracker.suspendPlayback()

    fun handleFocusLoss(mayResume: Boolean): Boolean =
        playbackTracker.handleFocusLoss(mayResume)

    fun cancelPlaybackIntent(): Boolean = playbackTracker.cancelPlaybackIntent()

    fun setMediaMetadata(metadata: AndroidMediaMetadata?) {
        mediaState = mediaState.copy(metadata = metadata)
    }

    fun prepareForOpen(metadata: AndroidMediaMetadata?) {
        mediaState = mediaState.copy(
            metadata = metadata,
            playbackState = 0,
            positionMicros = 0L,
            durationMicros = 0L,
        )
    }

    fun setSystemMediaNavigation(arguments: Map<String, Any?>) {
        mediaState = updatedSystemMediaNavigation(mediaState, arguments)
    }

    fun setPlaybackRate(rate: Float) {
        mediaState = mediaState.copy(playbackRate = rate)
    }

    fun updateMediaState(event: Map<*, *>) {
        mediaState = updatedAndroidMediaState(mediaState, event)
    }

    fun requestRender() = playbackTracker.requestRender()

    fun markRenderAttempted() = playbackTracker.markRenderAttempted()

    fun beginContentPreparation(
        onCancel: (String) -> Unit,
    ): AndroidContentPreparationToken = contentPreparations.begin(onCancel)

    fun finishContentPreparation(token: AndroidContentPreparationToken): Boolean =
        !destroyed && contentPreparations.finish(token)

    fun cancelContentPreparations(reason: String): Int = contentPreparations.invalidate(reason)

    fun invoke(method: String, arguments: Map<String, Any?>): NativeResponse {
        return invokeEncoded(method, NativeJson.encodeArguments(arguments))
    }

    fun invokeEncoded(
        method: String,
        argumentsJson: String,
        ownedFd: Int = NO_OWNED_FD,
    ): NativeResponse {
        check(!destroyed) { "Erika player $handle has been destroyed" }
        return NativeJson.decodeResponse(
            ErikaNative.nativeInvoke(handle, method, argumentsJson, ownedFd),
        )
    }

    fun attachSurface(
        surface: Surface,
        width: Int,
        height: Int,
        scale: Double,
        extendedLinear: Boolean,
        directComposition: Boolean,
        desiredHeadroom: Float,
        fallbackReason: Int,
    ): NativeResponse {
        check(!destroyed) { "Erika player $handle has been destroyed" }
        val response = NativeJson.decodeResponse(
            ErikaNative.nativeAttachSurface(
                handle,
                surface,
                width,
                height,
                scale,
                extendedLinear,
                directComposition,
                desiredHeadroom,
                fallbackReason,
            ),
        )
        if (response.ok) {
            playbackTracker.attachSurface()
        }
        return response
    }

    fun resizeSurface(width: Int, height: Int, scale: Double): NativeResponse {
        if (!surfaceAttached || destroyed) {
            return NativeResponse.success()
        }
        val response = NativeJson.decodeResponse(
            ErikaNative.nativeResizeSurface(handle, width, height, scale),
        )
        if (response.ok) {
            playbackTracker.resizeSurface()
        }
        return response
    }

    fun setOutputHeadroom(headroom: Float, known: Boolean): NativeResponse =
        invoke(
            "setOutputHeadroom",
            mapOf(
                "headroom" to headroom,
                "known" to known,
            ),
        )

    fun detachSurface(): NativeResponse {
        if (!surfaceAttached || destroyed) {
            return NativeResponse.success()
        }
        val response = NativeJson.decodeResponse(ErikaNative.nativeDetachSurface(handle))
        if (response.ok) {
            playbackTracker.detachSurface()
        }
        return response
    }

    fun renderTick(timeSeconds: Double): NativeResponse {
        check(!destroyed) { "Erika player $handle has been destroyed" }
        return NativeJson.decodeResponse(ErikaNative.nativeRenderTick(handle, timeSeconds))
    }

    fun audioOnlyTick(): NativeResponse {
        check(!destroyed) { "Erika player $handle has been destroyed" }
        return NativeJson.decodeResponse(ErikaNative.nativeAudioOnlyTick(handle))
    }

    fun pollEvent(): NativeResponse? {
        if (destroyed) {
            return null
        }
        return ErikaNative.nativePollEvent(handle)?.let(NativeJson::decodeResponse)
    }

    fun captureFrame(width: Int, height: Int): ByteArray? {
        check(!destroyed) { "Erika player $handle has been destroyed" }
        return ErikaNative.nativeCaptureFrame(handle, width, height)
    }

    fun destroy() {
        if (destroyed) {
            return
        }
        cancelContentPreparations("player_disposed")
        val view = attachedView
        runCatching { view?.unbind(this) }
        attachedView = null
        runCatching { detachSurface() }
        try {
            ErikaNative.nativeDestroy(handle)
        } finally {
            pendingEvents.clear()
            destroyed = true
            view?.onPlayerDestroyed(this)
        }
    }

    private companion object {
        const val NO_OWNED_FD = -1
        const val MAX_PENDING_EVENTS = 1024
    }
}
