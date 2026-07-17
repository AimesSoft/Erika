package dev.aimesoft.erika_flutter

internal sealed interface AndroidPendingEvent {
    data class Success(val value: Map<String, Any?>) : AndroidPendingEvent

    data class Error(
        val code: String,
        val message: String,
        val details: Map<String, Any?>,
    ) : AndroidPendingEvent
}

internal data class AndroidPendingEventOverflow(
    val dropped: AndroidPendingEvent,
    val droppedTotal: Long,
    val capacity: Int,
)

/**
 * Per-player FIFO used while Flutter has no active EventChannel listener.
 *
 * The queue deliberately drops the oldest item on overflow. This preserves the
 * most recent playback/error state while keeping memory bounded if Dart stays
 * detached for an extended playback session.
 */
internal class AndroidPendingEventQueue(
    val capacity: Int,
) {
    private val events = ArrayDeque<AndroidPendingEvent>()
    private var droppedTotal = 0L

    init {
        require(capacity > 0) { "Pending event queue capacity must be positive" }
    }

    val size: Int
        get() = events.size

    fun enqueue(event: AndroidPendingEvent): AndroidPendingEventOverflow? {
        val dropped = if (events.size >= capacity) events.removeFirst() else null
        events.addLast(event)
        if (dropped == null) {
            return null
        }
        droppedTotal += 1
        return AndroidPendingEventOverflow(dropped, droppedTotal, capacity)
    }

    fun firstOrNull(): AndroidPendingEvent? = events.firstOrNull()

    fun removeFirst(): AndroidPendingEvent = events.removeFirst()

    fun clear() {
        events.clear()
    }
}
