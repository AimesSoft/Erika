package dev.aimesoft.erika_flutter

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class AndroidSurfaceLifecycleTest {
    @Test
    fun `failed native detach releases texture ownership and schedules retry`() {
        val decision = androidSurfaceDestroyDecision(nativeDetachSucceeded = false)

        assertTrue(decision.releaseSurfaceTexture)
        assertTrue(decision.retryNativeDetach)
    }

    @Test
    fun `successful native detach needs no retry`() {
        val decision = androidSurfaceDestroyDecision(nativeDetachSucceeded = true)

        assertTrue(decision.releaseSurfaceTexture)
        assertFalse(decision.retryNativeDetach)
    }

    @Test
    fun `surface recovery uses bounded exponential backoff`() {
        val delays = (1..ANDROID_SURFACE_RECOVERY_MAX_RETRIES)
            .map(::androidSurfaceRecoveryDelayMillis)

        assertEquals(listOf(16L, 32L, 64L, 128L, 256L, 512L), delays)
        assertNull(androidSurfaceRecoveryDelayMillis(0))
        assertNull(
            androidSurfaceRecoveryDelayMillis(ANDROID_SURFACE_RECOVERY_MAX_RETRIES + 1),
        )
    }

    @Test
    fun `invalidating recovery token rejects stale callbacks`() {
        val tokens = AndroidSurfaceRecoveryTokenSource()
        val initial = tokens.currentToken

        assertTrue(tokens.isCurrent(initial))
        tokens.invalidate()
        assertFalse(tokens.isCurrent(initial))

        val replacement = tokens.currentToken
        assertTrue(tokens.isCurrent(replacement))
        tokens.invalidate()
        assertFalse(tokens.isCurrent(replacement))
    }

    @Test
    fun `successful attached recovery resumes hdr headroom observation`() {
        assertTrue(
            androidShouldRefreshHdrHeadroomAfterRecovery(
                hostStillBound = true,
                surfaceAttached = true,
                disposed = false,
                disposeRequested = false,
                unbindRequested = false,
            ),
        )
    }

    @Test
    fun `recovery does not resume hdr observation without a live attached binding`() {
        val inactiveStates = listOf(
            androidShouldRefreshHdrHeadroomAfterRecovery(
                hostStillBound = false,
                surfaceAttached = true,
                disposed = false,
                disposeRequested = false,
                unbindRequested = false,
            ),
            androidShouldRefreshHdrHeadroomAfterRecovery(
                hostStillBound = true,
                surfaceAttached = false,
                disposed = false,
                disposeRequested = false,
                unbindRequested = false,
            ),
            androidShouldRefreshHdrHeadroomAfterRecovery(
                hostStillBound = true,
                surfaceAttached = true,
                disposed = true,
                disposeRequested = false,
                unbindRequested = false,
            ),
            androidShouldRefreshHdrHeadroomAfterRecovery(
                hostStillBound = true,
                surfaceAttached = true,
                disposed = false,
                disposeRequested = true,
                unbindRequested = false,
            ),
            androidShouldRefreshHdrHeadroomAfterRecovery(
                hostStillBound = true,
                surfaceAttached = true,
                disposed = false,
                disposeRequested = false,
                unbindRequested = true,
            ),
        )

        assertTrue(inactiveStates.all { shouldRefresh -> !shouldRefresh })
    }

    @Test
    fun `pending view bind resumes only for a live host and target`() {
        assertTrue(
            androidShouldResumePendingViewBind(
                hostDestroyed = false,
                targetDisposed = false,
                targetDisposeRequested = false,
                targetAcceptsHost = true,
                hostAcceptsTarget = true,
            ),
        )
        assertFalse(
            androidShouldResumePendingViewBind(
                hostDestroyed = true,
                targetDisposed = false,
                targetDisposeRequested = false,
                targetAcceptsHost = true,
                hostAcceptsTarget = true,
            ),
        )
        assertFalse(
            androidShouldResumePendingViewBind(
                hostDestroyed = false,
                targetDisposed = true,
                targetDisposeRequested = false,
                targetAcceptsHost = true,
                hostAcceptsTarget = true,
            ),
        )
        assertFalse(
            androidShouldResumePendingViewBind(
                hostDestroyed = false,
                targetDisposed = false,
                targetDisposeRequested = true,
                targetAcceptsHost = true,
                hostAcceptsTarget = true,
            ),
        )
        assertFalse(
            androidShouldResumePendingViewBind(
                hostDestroyed = false,
                targetDisposed = false,
                targetDisposeRequested = false,
                targetAcceptsHost = false,
                hostAcceptsTarget = true,
            ),
        )
        assertFalse(
            androidShouldResumePendingViewBind(
                hostDestroyed = false,
                targetDisposed = false,
                targetDisposeRequested = false,
                targetAcceptsHost = true,
                hostAcceptsTarget = false,
            ),
        )
    }
}
