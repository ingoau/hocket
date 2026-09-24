package app.hocket

import android.app.Application
import androidx.lifecycle.ProcessLifecycleOwner
import app.hocket.core.CoreHandle
import app.hocket.core.client.CoreClient
import app.hocket.playback.CoreHost
import app.hocket.playback.ForegroundBinder
import app.hocket.playback.PlaybackServiceConnection
import coil3.ImageLoader
import coil3.SingletonImageLoader
import coil3.disk.DiskCache
import coil3.memory.MemoryCache
import coil3.request.crossfade
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.launch
import okio.Path.Companion.toOkioPath

/**
 * Process-wide wiring: binds to the playback service (which owns the core) while any UI is started
 * and builds the one [CoreClient] the UI reads. Screens never see a `CoreHandle` directly.
 */
class HocketApp : Application(), SingletonImageLoader.Factory {
    val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    lateinit var connection: PlaybackServiceConnection
        private set
    val prefs: AppPrefs by lazy { AppPrefs(this) }

    private val _client = MutableStateFlow<CoreClient?>(null)
    val client: StateFlow<CoreClient?> = _client.asStateFlow()

    override fun onCreate() {
        super.onCreate()
        connection = PlaybackServiceConnection(this)
        scope.launch {
            // The client exists for a handle the service delivered that CoreHost still runs: the
            // connection keeps the handle across an unbind, CoreHost drops it when the service stops.
            combine(connection.core, CoreHost.handle) { delivered, live -> delivered?.takeIf { it === live } }
                .collect { handle -> attach(handle) }
        }
        // Bound only while UI is visible (never from here): an idle service can then stop, and the
        // process can go after the task is removed. Media3 keeps it alive while playing.
        ProcessLifecycleOwner.get().lifecycle.addObserver(ForegroundBinder(connection))
    }

    /** Swap in a different core (UI tests). */
    fun attach(handle: CoreHandle?) {
        val previous = _client.value
        if (handle == null) {
            previous?.close()
            _client.value = null
            return
        }
        if (previous?.core === handle) return
        previous?.close()
        val client = CoreClient(handle, scope)
        _client.value = client
        client.requestSnapshot()
    }

    override fun newImageLoader(context: coil3.PlatformContext): ImageLoader =
        ImageLoader.Builder(context)
            .memoryCache { MemoryCache.Builder().maxSizePercent(context, 0.2).build() }
            .diskCache { DiskCache.Builder().directory(cacheDir.resolve("coil").toOkioPath()).maxSizeBytes(64L * 1024 * 1024).build() }
            .crossfade(180)
            .build()
}
