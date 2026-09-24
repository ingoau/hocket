package app.hocket.core

import app.hocket.core.api.Command
import app.hocket.core.api.Event
import app.hocket.core.api.Query
import app.hocket.core.api.QueryResult
import kotlinx.coroutines.flow.SharedFlow

/** Which implementation is behind a [CoreHandle]. Shown in the debug "core not built" banner. */
enum class CoreKind { Native, Fake }

/**
 * The seam. Everything above this talks to the core through these three members and nothing else.
 * Implemented by [NativeCore] (UniFFI) and [app.hocket.core.fake.FakeCore] (in-process Kotlin).
 */
interface CoreHandle {
    val kind: CoreKind

    /** Fire and forget; never blocks. */
    fun dispatch(command: Command)

    /** Async request/response. */
    suspend fun query(query: Query): QueryResult

    /**
     * Every event the core emits, on an unspecified thread, in order and without loss (the native
     * seam buffers without bound). Replays nothing: subscribe before dispatching `Start` or
     * [Command.RequestSnapshot] (e.g. `launch(start = CoroutineStart.UNDISPATCHED)`).
     */
    val events: SharedFlow<Event>

    /**
     * Flush and stop the core. The handle is unusable afterwards. May block for the duration of the
     * flush (bounded): call it from a worker thread, never from the main thread.
     */
    fun close()
}
