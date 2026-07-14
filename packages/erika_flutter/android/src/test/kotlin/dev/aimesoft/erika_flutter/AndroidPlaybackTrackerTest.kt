package dev.aimesoft.erika_flutter

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class AndroidPlaybackTrackerTest {
    @Test
    fun `explicit pause cancels delayed playback intent`() {
        val tracker = AndroidPlaybackTracker()

        tracker.requestPlayback()
        assertEquals(AndroidPlaybackPhase.PENDING, tracker.phase)

        tracker.cancelPlaybackIntent()

        assertEquals(AndroidPlaybackPhase.PAUSED, tracker.phase)
        assertFalse(tracker.playbackStarted())
        assertEquals(AndroidPlaybackPhase.PAUSED, tracker.phase)
    }

    @Test
    fun `lifecycle stop preserves resumable playback intent`() {
        val tracker = AndroidPlaybackTracker()
        tracker.requestPlayback()
        assertTrue(tracker.playbackStarted())

        assertTrue(tracker.suspendPlayback())
        assertEquals(AndroidPlaybackPhase.PENDING, tracker.phase)

        assertTrue(tracker.playbackStarted())
        assertEquals(AndroidPlaybackPhase.PLAYING, tracker.phase)
    }

    @Test
    fun `explicit stop or close cancels pending playback`() {
        val tracker = AndroidPlaybackTracker()

        tracker.requestPlayback()
        assertFalse(tracker.cancelPlaybackIntent())
        assertEquals(AndroidPlaybackPhase.PAUSED, tracker.phase)

        tracker.requestPlayback()
        assertFalse(tracker.cancelPlaybackIntent())
        assertEquals(AndroidPlaybackPhase.PAUSED, tracker.phase)
    }

    @Test
    fun `transient focus loss is resumable but permanent loss is not`() {
        val tracker = AndroidPlaybackTracker()
        tracker.requestPlayback()
        tracker.playbackStarted()

        assertTrue(tracker.handleFocusLoss(mayResume = true))
        assertEquals(AndroidPlaybackPhase.PENDING, tracker.phase)
        assertTrue(tracker.playbackStarted())

        assertTrue(tracker.handleFocusLoss(mayResume = false))
        assertEquals(AndroidPlaybackPhase.PAUSED, tracker.phase)
        assertFalse(tracker.playbackStarted())
    }

    @Test
    fun `ticking supports headless playback and surface render requests`() {
        val tracker = AndroidPlaybackTracker()

        assertFalse(tracker.shouldTick)
        tracker.requestRender()
        assertFalse(tracker.shouldTick)

        tracker.attachSurface()
        assertTrue(tracker.shouldTick)
        tracker.markRenderAttempted()
        assertFalse(tracker.shouldTick)

        tracker.requestPlayback()
        assertFalse(tracker.shouldTick)
        tracker.playbackStarted()
        assertTrue(tracker.shouldTick)
        tracker.markRenderAttempted()
        assertTrue(tracker.shouldTick)

        tracker.suspendPlayback()
        assertFalse(tracker.shouldTick)
        tracker.requestRender()
        assertTrue(tracker.shouldTick)
        tracker.detachSurface()
        assertFalse(tracker.shouldTick)

        tracker.requestPlayback()
        tracker.playbackStarted()
        assertTrue(tracker.shouldTick)
    }

    @Test
    fun `players keep independent playback and render state`() {
        val first = AndroidPlaybackTracker()
        val second = AndroidPlaybackTracker()

        first.attachSurface()
        first.markRenderAttempted()
        first.requestPlayback()
        first.playbackStarted()

        second.attachSurface()
        second.markRenderAttempted()
        second.requestPlayback()
        second.playbackStarted()

        assertEquals(AndroidPlaybackPhase.PLAYING, first.phase)
        assertTrue(first.shouldTick)
        assertEquals(AndroidPlaybackPhase.PLAYING, second.phase)
        assertTrue(second.shouldTick)

        first.cancelPlaybackIntent()
        assertEquals(AndroidPlaybackPhase.PAUSED, first.phase)
        assertEquals(AndroidPlaybackPhase.PLAYING, second.phase)
        assertTrue(second.shouldTick)
    }
}
