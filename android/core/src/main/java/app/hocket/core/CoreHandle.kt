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

    /** Every event the core emits, on an unspecified thread. Replays nothing; call [Command.RequestSnapshot]. */
    val events: SharedFlow<Event>

    /** Stop the core. The handle is unusable afterwards. */
    fun close()
}
