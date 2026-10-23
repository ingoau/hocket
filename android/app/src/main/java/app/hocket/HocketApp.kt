package app.hocket

import android.app.Application
import androidx.lifecycle.ProcessLifecycleOwner
import app.hocket.core.CoreHandle
import app.hocket.core.api.Command
import app.hocket.core.api.CommandAnswerDiagnosticsRequestInner
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
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.filterNotNull
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.withTimeoutOrNull
import androidx.core.content.pm.ShortcutManagerCompat
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
    /** Answers the other devices' diagnostics requests for the current client. */
    private var diagnosticsAnswers: Job? = null

    override fun onCreate() {
        super.onCreate()
        // Log capture from the start (the same dir CoreHost gives the core), and uncaught exceptions
        // go to it (and to disk) before the process dies.
        app.hocket.core.DevLog.install(java.io.File(filesDir, "hocket"))
        app.hocket.core.DevLog.installCrashHandler()
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
        scope.launch { prefs.shortcuts.distinctUntilChanged().collect { AppShortcuts.publish(this@HocketApp, it) } }
    }

    /**
     * Runs a launcher shortcut ([AppShortcuts]): search opens the search screen; the rest wait for the
     * core and its server (a cold start from the launcher) and dispatch the shortcut's command.
     */
    fun runShortcut(shortcut: AppShortcut) {
        ShortcutManagerCompat.reportShortcutUsed(this, shortcut.id)
        if (shortcut == AppShortcut.Search) { AppLinks.openSearch(null); return }
        scope.launch {
            withTimeoutOrNull(SHORTCUT_WAIT_MS) {
                val client = client.filterNotNull().first()
                val server = client.servers.first { it.isNotEmpty() }.first()
                shortcut.command(this@HocketApp, server.id)?.let(client::dispatch)
            }
        }
    }

    companion object {
        /** How long a shortcut waits for the core to come up before it gives up. */
        const val SHORTCUT_WAIT_MS = 20_000L
    }

    /** Swap in a different core (UI tests). */
    fun attach(handle: CoreHandle?) {
        val previous = _client.value
        if (previous?.core !== handle) diagnosticsAnswers?.cancel()
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
        diagnosticsAnswers = answerDiagnosticsRequests(client)
    }

    /**
     * Another device in the Connect room asks for this phone's diagnostics (the core passes requests
     * on only while developer mode is on here): answer with the bundle the diagnostics page would
     * share. Without a client (no UI bound) the core answers with its own diagnostics and log.
     */
    private fun answerDiagnosticsRequests(client: CoreClient): Job =
        scope.launch {
            client.diagnosticsRequests.collect { request ->
                launch {
                    val report = runCatching { app.hocket.diagnostics.DiagnosticsBundle.collect(this@HocketApp, client) }.getOrNull() ?: return@launch
                    val text = kotlinx.coroutines.withContext(Dispatchers.Default) { report.text }
                    client.dispatch(Command.AnswerDiagnosticsRequest(CommandAnswerDiagnosticsRequestInner(request.request_id, text)))
                }
            }
        }

    override fun newImageLoader(context: coil3.PlatformContext): ImageLoader =
        ImageLoader.Builder(context)
            .memoryCache { MemoryCache.Builder().maxSizePercent(context, 0.2).build() }
            .diskCache { DiskCache.Builder().directory(cacheDir.resolve("coil").toOkioPath()).maxSizeBytes(64L * 1024 * 1024).build() }
            .crossfade(180)
            .build()
}
