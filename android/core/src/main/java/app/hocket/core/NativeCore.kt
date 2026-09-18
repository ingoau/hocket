package app.hocket.core

import android.util.Log
import app.hocket.core.api.Command
import app.hocket.core.api.CoreConfig
import app.hocket.core.api.Event
import app.hocket.core.api.Query
import app.hocket.core.api.QueryResult
import app.hocket.core.ffi.EventListener
import app.hocket.core.ffi.HocketCore
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.serialization.SerializationException

/**
 * The real core: a UniFFI [HocketCore] talked to in JSON strings of the typeshare types.
 *
 * Construction loads `libhocket_android.so` through JNA. [isAvailable] probes that without throwing so
 * the app can fall back to the fake core (and show its debug banner) when the native library has not
 * been built for this ABI.
 */
class NativeCore private constructor(private val core: HocketCore) : CoreHandle {
    override val kind: CoreKind = CoreKind.Native

    // Unbounded replay-less buffer: events are decoded on the core's thread and must never block it.
    private val _events = MutableSharedFlow<Event>(extraBufferCapacity = 512)
    override val events: SharedFlow<Event> = _events.asSharedFlow()

    init {
        core.setListener(object : EventListener {
            override fun onEvent(eventJson: String) {
                val event = try {
                    HocketJson.decodeEvent(eventJson)
                } catch (e: SerializationException) {
                    Log.w(TAG, "Dropping undecodable event: ${e.message}")
                    return
                }
                if (!_events.tryEmit(event)) Log.w(TAG, "Event buffer full; dropped ${event::class.simpleName}")
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

    override fun close() {
        dispatch(Command.Shutdown)
        core.close()
    }

    companion object {
        private const val TAG = "NativeCore"

        @Volatile
        private var available: Boolean? = null

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
