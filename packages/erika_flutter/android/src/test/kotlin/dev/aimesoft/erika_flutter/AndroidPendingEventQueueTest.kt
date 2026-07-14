package dev.aimesoft.erika_flutter

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Test

class AndroidPendingEventQueueTest {
    @Test
    fun `events remain ordered until a listener can consume them`() {
        val queue = AndroidPendingEventQueue(capacity = 3)
        val first = successEvent(1)
        val second = AndroidPendingEvent.Error(
            code = "ERIKA_ERROR",
            message = "native poll failed",
            details = mapOf("playerId" to 7L),
        )
        val third = successEvent(3)

        assertNull(queue.enqueue(first))
        assertNull(queue.enqueue(second))
        assertNull(queue.enqueue(third))

        assertEquals(first, queue.firstOrNull())
        assertEquals(first, queue.removeFirst())
        assertEquals(second, queue.removeFirst())
        assertEquals(third, queue.removeFirst())
        assertNull(queue.firstOrNull())
    }

    @Test
    fun `overflow drops oldest event and reports cumulative loss`() {
        val queue = AndroidPendingEventQueue(capacity = 2)
        val first = successEvent(1)
        val second = successEvent(2)
        val third = successEvent(3)
        val fourth = successEvent(4)

        queue.enqueue(first)
        queue.enqueue(second)
        val firstOverflow = queue.enqueue(third)
        val secondOverflow = queue.enqueue(fourth)

        assertEquals(first, firstOverflow?.dropped)
        assertEquals(1L, firstOverflow?.droppedTotal)
        assertEquals(2, firstOverflow?.capacity)
        assertEquals(second, secondOverflow?.dropped)
        assertEquals(2L, secondOverflow?.droppedTotal)
        assertEquals(third, queue.removeFirst())
        assertEquals(fourth, queue.removeFirst())
    }

    @Test
    fun `clear removes events retained across listener cancellation`() {
        val queue = AndroidPendingEventQueue(capacity = 2)
        queue.enqueue(successEvent(1))
        queue.enqueue(successEvent(2))

        queue.clear()

        assertEquals(0, queue.size)
        assertNull(queue.firstOrNull())
    }

    @Test
    fun `events queued before listen and after cancel flush in original order`() {
        val queue = AndroidPendingEventQueue(capacity = 4)
        val delivered = mutableListOf<AndroidPendingEvent>()
        queue.enqueue(successEvent(1))

        delivered += queue.removeFirst()
        queue.enqueue(successEvent(2))
        queue.enqueue(successEvent(3))
        while (queue.firstOrNull() != null) {
            delivered += queue.removeFirst()
        }

        assertEquals(
            listOf(successEvent(1), successEvent(2), successEvent(3)),
            delivered,
        )
    }

    @Test
    fun `capacity must be positive`() {
        assertThrows(IllegalArgumentException::class.java) {
            AndroidPendingEventQueue(capacity = 0)
        }
    }

    private fun successEvent(sequence: Int): AndroidPendingEvent.Success =
        AndroidPendingEvent.Success(mapOf("sequence" to sequence))
}
