package dev.aimesoft.erika_flutter

internal data class AndroidSurfaceDestroyDecision(
    val releaseSurfaceTexture: Boolean,
    val retryNativeDetach: Boolean,
)

internal fun androidSurfaceDestroyDecision(
    nativeDetachSucceeded: Boolean,
): AndroidSurfaceDestroyDecision = AndroidSurfaceDestroyDecision(
    // Returning true delegates SurfaceTexture release to TextureView. Returning
    // false without retaining and eventually releasing it would leak the buffer queue.
    releaseSurfaceTexture = true,
    // Native attachment state is committed only on success, so a failed detach
    // remains explicit and can be retried before binding the next SurfaceTexture.
    retryNativeDetach = !nativeDetachSucceeded,
)

internal const val ANDROID_SURFACE_RECOVERY_MAX_RETRIES = 6

private val androidSurfaceRecoveryDelaysMillis = listOf(16L, 32L, 64L, 128L, 256L, 512L)

/** Returns null once the bounded recovery budget has been exhausted. */
internal fun androidSurfaceRecoveryDelayMillis(retryAttempt: Int): Long? {
    if (retryAttempt <= 0) {
        return null
    }
    return androidSurfaceRecoveryDelaysMillis.getOrNull(retryAttempt - 1)
}

internal fun androidShouldRefreshHdrHeadroomAfterRecovery(
    hostStillBound: Boolean,
    surfaceAttached: Boolean,
    disposed: Boolean,
    disposeRequested: Boolean,
    unbindRequested: Boolean,
): Boolean = hostStillBound &&
    surfaceAttached &&
    !disposed &&
    !disposeRequested &&
    !unbindRequested

internal fun androidShouldResumePendingViewBind(
    hostDestroyed: Boolean,
    targetDisposed: Boolean,
    targetDisposeRequested: Boolean,
    targetAcceptsHost: Boolean,
    hostAcceptsTarget: Boolean,
): Boolean = !hostDestroyed &&
    !targetDisposed &&
    !targetDisposeRequested &&
    targetAcceptsHost &&
    hostAcceptsTarget

internal class AndroidSurfaceRecoveryTokenSource {
    var currentToken: Long = 0L
        private set

    fun invalidate() {
        currentToken += 1L
    }

    fun isCurrent(token: Long): Boolean = token == currentToken
}
