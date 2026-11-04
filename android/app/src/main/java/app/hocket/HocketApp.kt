package app.hocket

import android.app.Application
import androidx.lifecycle.ProcessLifecycleOwner
import app.hocket.core.CoreHandle
import app.hocket.core.api.Command
import app.hocket.core.api.CommandAnswerDiagnosticsRequestInner
import app.hocket.core.api.Query
import app.hocket.core.api.QueryResult
import app.hocket.core.api.ReportUploadState
import kotlinx.coroutines.flow.flatMapLatest
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.flow.map
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
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Job
import kotlinx.coroutines.withContext
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
        // Off the main thread: the first touch of the native library loads it, and the core's capture
        // reads back the previous run's log. Whatever needs either waits for it, not the first frame.
        Thread({ app.hocket.core.DevLog.install(java.io.File(filesDir, "hocket")) }, "hocket-devlog").start()
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
        scope.launch(Dispatchers.Default) { prefs.shortcuts.distinctUntilChanged().collect { AppShortcuts.publish(this@HocketApp, it) } }
        followReportUploads()
    }

    /**
     * A report upload (docs/reports.md) that is Uploading gets [app.hocket.diagnostics.ReportUploadService]
     * (foreground, progress notification), which stops itself once it is anything else. The first
     * time a core answers that no upload is pending, what an earlier process staged for one goes.
     */
    @OptIn(kotlinx.coroutines.ExperimentalCoroutinesApi::class)
    private fun followReportUploads() {
        val processStart = System.currentTimeMillis()
        scope.launch {
            client.flatMapLatest { it?.reportUpload ?: flowOf(null) }
                .map { it?.state == ReportUploadState.Uploading }
                .distinctUntilChanged()
                .collect { uploading -> if (uploading) app.hocket.diagnostics.ReportUploadService.start(this@HocketApp) }
        }
        scope.launch {
            val client = client.filterNotNull().first()
            client.started.first { it }
            val pending = client.query(Query.ReportUpload) as? QueryResult.ReportUpload ?: return@launch
            if (pending.data == null) withContext(Dispatchers.IO) { runCatching { app.hocket.diagnostics.ReportStaging.clearStale(this@HocketApp, processStart) } }
        }
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

    /**
     * Builds the core on a background thread ahead of the playback service, which the UI binds only
     * once the activity is started: the library loads and the database opens while the activity and
     * its first frame are being built, and the service's own [CoreHost.acquire] only has to start it
     * ([CoreHost.prewarm]). Only when UI is coming (MainActivity), so the service is sure to follow.
     */
    fun prewarmCore() {
        if (CoreHost.current != null) return
        Thread({ runCatching { CoreHost.prewarm(this) }.onFailure { android.util.Log.w("HocketApp", "core prewarm failed", it) } }, "hocket-prewarm").start()
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
     * share. One at a time, all of it off the main thread (a bundle can be tens of megabytes); a
     * request that isn't answered here (no client yet, a failure, one already being built) gets the
     * core's own diagnostics and log after 10 s.
     */
    private fun answerDiagnosticsRequests(client: CoreClient): Job =
        scope.launch {
            var building = false
            client.diagnosticsRequests.collect { request ->
                if (building) return@collect
                building = true
                launch {
                    try {
                        val report = app.hocket.diagnostics.DiagnosticsBundle.collect(this@HocketApp, client)
                        withContext(Dispatchers.Default) {
                            client.dispatch(Command.AnswerDiagnosticsRequest(CommandAnswerDiagnosticsRequestInner(request.request_id, report.text)))
                        }
                    } catch (e: CancellationException) {
                        throw e
                    } catch (t: Throwable) {
                        // Out of memory included: the core answers on its own.
                        android.util.Log.w("HocketApp", "couldn't answer a diagnostics request: ${t.javaClass.simpleName}")
                    } finally {
                        building = false
                    }
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
