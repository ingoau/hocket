package app.hocket.playback

import android.content.Intent
import android.os.Binder
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.util.Log
import androidx.media3.session.MediaSession
import androidx.media3.session.MediaLibraryService
import app.hocket.core.CoreHandle
import app.hocket.core.SettingKeys
import app.hocket.core.api.Command
import app.hocket.core.api.Event
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.collect
import kotlinx.coroutines.launch

/**
 * The Media3 [MediaLibraryService] that owns the process's core.
 *
 * - Creates the core (or the fake) through [CoreHost] and forwards its events:
 *   `Event.Backend` -> [ExoBackend], `Event.MediaSession` and `Event.QueueChanged` ->
 *   [MediaSessionBridge] (the session's state and its queue timeline).
 * - Publishes the library to other apps (Android Auto, Wear, Assistant, `MediaBrowser`s) through
 *   [LibraryBrowser] on the first server, telling subscribed browsers when it changes, only while
 *   "Allow control by other apps" (`media.externalControl`, [ExternalControl]) is on.
 * - Registers the [NetworkMonitor] and [BatterySaverMonitor].
 * - Feeds [ConnectRoutes] (devices, lease owner, session state) so that while another device plays
 *   the session reports remote playback and [ConnectRouteProvider] names that device as the output;
 *   holds the app's MediaRouter2 discovery preference ([ConnectRouteDiscovery], API 30+).
 * - Stays a foreground service (type `mediaPlayback`) while the media session says something is
 *   playing; Media3 handles the notification and foreground promotion. That only happens for a
 *   session the service knows about: Media3 adds one when a controller connects through
 *   [onGetSession], but the UI binds for the core, not as a controller, so the session is added
 *   explicitly in [onCreate]. Without that there was no notification, no lock-screen or quick-settings
 *   controls, and no foreground promotion, so a backgrounded process could be killed mid-song.
 *   When nothing has played for
 *   [IDLE_TIMEOUT_MS] and no UI client is bound, it stops itself; a player that is still buffering
 *   or recovering from a network error ([ExoBackend.isBusy]) keeps it, whatever the session says.
 * - Exposes a [LocalBinder] so the app process can obtain the [CoreHandle] by binding. Only the
 *   app's own bind counts as a UI client: the service is exported for Media3, so the bind intent
 *   carries [CoreHost.bindToken], which no other process can know.
 */
class PlaybackService : MediaLibraryService() {
    companion object {
        private const val TAG = "PlaybackService"
        const val IDLE_TIMEOUT_MS = 5 * 60_000L
        const val ACTION_BIND_CORE = "app.hocket.playback.BIND_CORE"
        const val EXTRA_BIND_TOKEN = "app.hocket.playback.BIND_TOKEN"
        /** Library changes arrive in bursts (a sync); browsers hear about them once they settle. */
        const val LIBRARY_NOTIFY_DEBOUNCE_MS = 2_000L

        /**
         * True for this process's own core bind. `onBind` runs outside a binder transaction, so
         * `Binder.getCallingUid()` cannot tell callers apart; the per-process token can.
         */
        fun isLocalBind(intent: Intent?, token: String = CoreHost.bindToken): Boolean =
            intent?.action == ACTION_BIND_CORE && intent.getStringExtra(EXTRA_BIND_TOKEN) == token && intent.data == bindUri(token)

        /** Makes the in-process bind intent distinct from any other under `Intent.filterEquals`. */
        fun bindUri(token: String): android.net.Uri = android.net.Uri.parse("hocket-bind://core/$token")
    }

    inner class LocalBinder : Binder() {
        val core: CoreHandle get() = this@PlaybackService.core
        val service: PlaybackService get() = this@PlaybackService
    }

    lateinit var core: CoreHandle
        private set
    private lateinit var backend: ExoBackend
    private lateinit var bridge: MediaSessionBridge
    private lateinit var network: NetworkMonitor
    private lateinit var battery: BatterySaverMonitor
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val main = Handler(Looper.getMainLooper())
    private var idleJob: Job? = null
    private var boundClients = 0
    private var clockOffsetMs = 0.0
    private var routeDiscovery: Any? = null
    private var serverId: String? = null
    private var libraryNotify: Job? = null

    override fun onCreate() {
        super.onCreate()
        core = CoreHost.acquire(this)
        backend = ExoBackend(this, scope, ::dispatch)
        val launch = packageManager.getLaunchIntentForPackage(packageName)
        val browser = LibraryBrowser(this, { core }, { serverId }, scope)
        val player = CoreSessionPlayer(Looper.getMainLooper(), ::dispatch, media = browser, artwork = { t -> t.coverArt?.let { ArtworkProvider.uri(this, it) } })
        bridge = MediaSessionBridge(this, player, ::dispatch, launch, browser, scope, ExternalControl(this))
        addSession(bridge.session)
        network = NetworkMonitor(this, ::dispatch, onConnectivity = { online -> main.post { backend.onConnectivityChanged(online) } })
        battery = BatterySaverMonitor(this, ::dispatch)
        // Subscribed before the snapshot is requested, so its `Started` cannot be missed.
        scope.launch(start = CoroutineStart.UNDISPATCHED) { core.events.collect { event -> main.post { onEvent(event) } } }
        network.start()
        battery.start()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            routeDiscovery = runCatching { ConnectRouteDiscovery(this).also { it.start() } }
                .onFailure { Log.w(TAG, "route discovery unavailable: ${it.message}") }.getOrNull()
        }
        core.dispatch(Command.RequestSnapshot)
        scheduleIdleStop()
    }

    private fun dispatch(command: Command) = core.dispatch(command)

    private fun onEvent(event: Event) {
        when (event) {
            is Event.Backend -> if (core.kind == app.hocket.core.CoreKind.Native) backend.handle(event.data.command) else Unit
            is Event.MediaSession -> {
                applySession(event.data.state)
                if (event.data.state.isPlaying) idleJob?.cancel() else scheduleIdleStop()
            }
            is Event.DevicesChanged -> {
                ConnectRoutes.onDevices(event.data.devices)
                applySession(bridge.player.state)
            }
            is Event.TransportChanged -> {
                val wasRemote = ConnectRoutes.state.value.remote
                ConnectRoutes.onOwner(event.data.transport.lease.owner)
                if (ConnectRoutes.state.value.remote != wasRemote) applySession(bridge.player.state)
            }
            is Event.QueueChanged -> bridge.applyQueue(event.data.queue)
            is Event.ServersChanged -> serverId = event.data.servers.firstOrNull()?.id
            is Event.LibraryChanged -> scheduleLibraryNotify()
            is Event.ConnectionChanged -> clockOffsetMs = event.data.state.clockOffsetMs
            is Event.Started -> applySnapshot(event.data.snapshot)
            is Event.Snapshot -> applySnapshot(event.data.snapshot)
            is Event.SettingChanged -> when (event.data.setting.key) {
                SettingKeys.BATTERY_AUTO_ENGAGE -> battery.automatic = event.data.setting.value.trim() != "false"
                SettingKeys.MEDIA_EXTERNAL_CONTROL -> bridge.setExternalControl(event.data.setting.value.trim() == "true")
            }
            else -> Unit
        }
    }

    /** `Started` (once per core) and `Snapshot` (every RequestSnapshot) carry the same state. */
    private fun applySnapshot(snapshot: app.hocket.core.api.Snapshot) {
        clockOffsetMs = snapshot.connection.clockOffsetMs
        serverId = snapshot.servers.firstOrNull()?.id
        bridge.applyQueue(snapshot.queue)
        ConnectRoutes.onDevices(snapshot.devices)
        ConnectRoutes.onOwner(snapshot.transport.lease.owner)
        applySession(snapshot.mediaSession)
        battery.automatic = snapshot.settings.firstOrNull { it.key == SettingKeys.BATTERY_AUTO_ENGAGE }?.value?.trim() != "false"
        bridge.setExternalControl(snapshot.settings.firstOrNull { it.key == SettingKeys.MEDIA_EXTERNAL_CONTROL }?.value?.trim() == "true")
    }

    /** Session state to the bridge, remote when another Connect device plays it. */
    private fun applySession(state: app.hocket.core.api.MediaSessionState) {
        ConnectRoutes.onMediaSession(state)
        bridge.apply(state, clockOffsetMs, remote = ConnectRoutes.state.value.remote != null)
    }

    private fun scheduleLibraryNotify() {
        libraryNotify?.cancel()
        libraryNotify = scope.launch {
            delay(LIBRARY_NOTIFY_DEBOUNCE_MS)
            bridge.libraryChanged()
        }
    }

    private fun scheduleIdleStop() {
        idleJob?.cancel()
        idleJob = scope.launch {
            delay(IDLE_TIMEOUT_MS)
            if (!bridge.player.state.isPlaying && boundClients == 0 && !backend.isBusy()) {
                Log.i(TAG, "Idle for ${IDLE_TIMEOUT_MS / 1000}s with no clients; stopping")
                pauseAllPlayersAndStopSelf()
            } else if (boundClients == 0) {
                // Still playing, buffering or recovering: look again later.
                scheduleIdleStop()
            }
        }
    }

    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaLibrarySession = bridge.session

    override fun onBind(intent: Intent?): IBinder? {
        if (isLocalBind(intent)) {
            boundClients++
            idleJob?.cancel()
            return LocalBinder()
        }
        // Media3 controllers, and anything else (a foreign BIND_CORE gets nothing and is not counted).
        return super.onBind(intent)
    }

    override fun onUnbind(intent: Intent?): Boolean {
        if (isLocalBind(intent)) {
            boundClients = (boundClients - 1).coerceAtLeast(0)
            if (!bridge.player.state.isPlaying) scheduleIdleStop()
            return true
        }
        return super.onUnbind(intent)
    }

    override fun onRebind(intent: Intent?) {
        if (isLocalBind(intent)) { boundClients++; idleJob?.cancel() } else super.onRebind(intent)
    }

    override fun onTaskRemoved(rootIntent: Intent?) {
        // The UI unbinds shortly after (ForegroundBinder); with nothing playing the service then goes.
        // A player that is still buffering or recovering from a network error is not "nothing".
        if (!bridge.player.state.isPlaying && !backend.isBusy()) pauseAllPlayersAndStopSelf()
    }

    override fun onDestroy() {
        network.stop()
        battery.stop()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) (routeDiscovery as? ConnectRouteDiscovery)?.stop()
        ConnectRoutes.clear()
        backend.release()
        bridge.release()
        scope.cancel()
        // Detaches the core and flushes it on a worker thread (the flush blocks; never on main).
        CoreHost.shutdown()
        super.onDestroy()
    }
}
