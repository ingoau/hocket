package app.hocket.core

import android.util.Log
import app.hocket.core.api.Command
import app.hocket.core.api.CoreConfig
import app.hocket.core.api.Event
import app.hocket.core.api.Query
import app.hocket.core.api.QueryResult
import app.hocket.core.ffi.EventListener
import app.hocket.core.ffi.HocketCore
import app.hocket.core.ffi.StreamException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.launch

/**
 * The real core: a UniFFI [HocketCore] talked to in JSON strings of the typeshare types.
 *
 * Construction loads `libhocket_android.so` through JNA. [isAvailable] probes that without throwing so
 * the app can fall back to the fake core (and show its debug banner) when the native library has not
 * been built for this ABI.
 *
 * Events: the core calls [EventListener.onEvent] synchronously on its actor thread, so nothing there
 * may block or throw. Decoded events go into an unbounded [Channel] (never blocks, never drops) and a
 * pump coroutine forwards them into the shared [events] flow with a suspending `emit`, so a slow
 * subscriber (the main thread mid-composition) applies back-pressure to the pump, not to the core,
 * and `Started` / `ServersChanged` / `Backend` can never be dropped.
 */
class NativeCore private constructor(private val core: HocketCore) : CoreHandle, CoreStreams {
    override val kind: CoreKind = CoreKind.Native

    private val pumpScope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private val pump = EventPump(pumpScope)
    override val events: SharedFlow<Event> = pump.events

    init {
        core.setListener(object : EventListener {
            override fun onEvent(eventJson: String) {
                // Anything escaping here surfaces in Rust as a callback failure (the crate contains
                // it, but the event would still be lost and the failure logged as a panic); catch
                // everything, including errors, and log only the event type: a decode failure's
                // message quotes the JSON, which for `Backend.Load` holds the stream URL with its
                // auth token.
                val event = try {
                    HocketJson.decodeEvent(eventJson)
                } catch (t: Throwable) {
                    Log.w(TAG, undecodable(eventJson, t))
                    return
                }
                try {
                    pump.offer(event)
                } catch (t: Throwable) {
                    Log.w(TAG, "Dropping event ${event::class.simpleName}: ${t.javaClass.simpleName}")
                }
            }
        })
    }

    override fun dispatch(command: Command) {
        try {
            core.dispatch(HocketJson.encodeCommand(command))
        } catch (e: Exception) {
            Log.e(TAG, "dispatch failed for ${command::class.simpleName}", e)
        }
    }

    override suspend fun query(query: Query): QueryResult =
        HocketJson.decodeQueryResult(core.query(HocketJson.encodeQuery(query)))

    // -- CoreStreams: blocking, for ExoPlayer's loader threads ------------------------------------

    override fun streamOpen(url: String, offset: Long, length: Long?): CoreStreamInfo = streamCall {
        val info = core.streamOpen(url, offset.coerceAtLeast(0).toULong(), length?.coerceAtLeast(0)?.toULong())
        CoreStreamInfo(info.handle.toLong(), info.offset.toLong(), info.totalLength?.toLong(), info.length?.toLong(), info.contentType)
    }

    override fun streamRead(handle: Long, maxBytes: Int): ByteArray = streamCall {
        core.streamRead(handle.toULong(), maxBytes.coerceIn(1, CoreStreams.MAX_READ).toUInt())
    }

    override fun streamClose(handle: Long) {
        try {
            core.streamClose(handle.toULong())
        } catch (e: Exception) {
            // Freed core: its handles are gone with it.
        }
    }

    /**
     * Maps UniFFI's [StreamException] to [CoreStreamException]. A call on a core that has been freed
     * (UniFFI throws `IllegalStateException`) is [CoreStreamException.Kind.ShutDown], so a loader
     * racing a service restart fails like an I/O error instead of crashing the player thread.
     */
    private inline fun <T> streamCall(block: () -> T): T = try {
        block()
    } catch (e: StreamException) {
        throw mapStreamException(e)
    } catch (e: IllegalStateException) {
        throw CoreStreamException(CoreStreamException.Kind.ShutDown)
    }

    /**
     * Flushes and stops the core (blocking up to [SHUTDOWN_TIMEOUT_MS]), then frees it. Blocks the
     * calling thread: never call it on the main thread.
     */
    override fun close() {
        try {
            if (!core.shutdown(SHUTDOWN_TIMEOUT_MS.toULong())) Log.w(TAG, "core shutdown did not finish within ${SHUTDOWN_TIMEOUT_MS}ms; freeing anyway")
        } catch (e: Exception) {
            Log.e(TAG, "core shutdown failed", e)
        }
        pump.close()
        pumpScope.cancel()
        core.close()
    }

    companion object {
        private const val TAG = "NativeCore"
        const val SHUTDOWN_TIMEOUT_MS = 8_000L

        @Volatile
        private var available: Boolean? = null

        /**
         * The log line for an event that did not decode: its type and the error without the JSON
         * excerpt kotlinx appends (for `Backend.Load` that excerpt is the stream URL with its token).
         */
        internal fun undecodable(json: String, t: Throwable): String =
            "Dropping undecodable event ${eventType(json)}: ${t.javaClass.simpleName}: ${t.message?.substringBefore("JSON input")?.trim()}"

        internal fun mapStreamException(e: StreamException): CoreStreamException = when (e) {
            is StreamException.UnknownToken -> CoreStreamException(CoreStreamException.Kind.UnknownToken)
            is StreamException.UnknownHandle -> CoreStreamException(CoreStreamException.Kind.UnknownHandle)
            is StreamException.TooManyHandles -> CoreStreamException(CoreStreamException.Kind.TooManyHandles)
            is StreamException.NoServer -> CoreStreamException(CoreStreamException.Kind.NoServer)
            is StreamException.RangeNotSatisfiable -> CoreStreamException(CoreStreamException.Kind.RangeNotSatisfiable)
            is StreamException.Status -> CoreStreamException(CoreStreamException.Kind.Status, httpStatus = e.code.toInt())
            is StreamException.ErrorEnvelope -> CoreStreamException(CoreStreamException.Kind.ErrorEnvelope)
            is StreamException.Network -> CoreStreamException(CoreStreamException.Kind.Network, detail = e.reason)
            is StreamException.Io -> CoreStreamException(CoreStreamException.Kind.Io, detail = e.reason)
            is StreamException.Closed -> CoreStreamException(CoreStreamException.Kind.Closed)
            is StreamException.ShutDown -> CoreStreamException(CoreStreamException.Kind.ShutDown)
        }

        /** The `"type"` tag of an event JSON without decoding it (for log lines only). */
        internal fun eventType(json: String): String =
            Regex("\"type\"\\s*:\\s*\"([A-Za-z0-9_]+)\"").find(json)?.groupValues?.get(1) ?: "?"

        /** True when the native library can be loaded for this ABI. Cached after the first probe. */
        fun isAvailable(): Boolean = available ?: synchronized(this) {
            available ?: try {
                // Touching the generated singleton forces JNA to resolve the library.
                app.hocket.core.ffi.coreVersion()
                true
            } catch (e: UnsatisfiedLinkError) {
                Log.w(TAG, "libhocket_android not available: ${e.message}")
                false
            } catch (e: NoClassDefFoundError) {
                Log.w(TAG, "UniFFI glue missing: ${e.message}")
                false
            }.also { available = it }
        }

        /** Version string reported by the Rust crate, or null when not loadable. */
        fun version(): String? = if (isAvailable()) app.hocket.core.ffi.coreVersion() else null

        /** Creates and starts a core. Throws when the library is not loadable; check [isAvailable] first. */
        fun create(config: CoreConfig, logLevel: String = "info"): NativeCore {
            app.hocket.core.ffi.initLogging(logLevel)
            val configJson = HocketJson.json.encodeToString(CoreConfig.serializer(), config)
            return NativeCore(HocketCore(configJson))
        }
    }
}

/**
 * The hand-off from the core's actor thread to [events]: [offer] never blocks and never drops (an
 * unbounded channel), and one coroutine forwards in order with a suspending `emit`, so a slow
 * subscriber holds up this pump rather than the core, and nothing (`Started`, `ServersChanged`,
 * `Backend`) is lost to back-pressure. Like any [SharedFlow] it replays nothing to late subscribers.
 */
internal class EventPump(scope: CoroutineScope) {
    private val inbox = Channel<Event>(Channel.UNLIMITED)
    private val _events = MutableSharedFlow<Event>(extraBufferCapacity = 256)
    val events: SharedFlow<Event> = _events.asSharedFlow()

    init {
        scope.launch { for (event in inbox) _events.emit(event) }
    }

    /** From any thread; false only once [close]d. */
    fun offer(event: Event): Boolean = inbox.trySend(event).isSuccess

    fun close() {
        inbox.close()
    }
}
