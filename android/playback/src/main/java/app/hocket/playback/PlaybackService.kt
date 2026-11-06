package app.hocket.playback

import android.content.Intent
import android.os.Binder
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import androidx.media3.session.MediaSession
import androidx.media3.session.MediaLibraryService
import app.hocket.core.CoreHandle
import app.hocket.core.SettingKeys
import app.hocket.core.Commands
import app.hocket.core.api.BackendCommand
import app.hocket.core.api.Command
import app.hocket.core.api.Event
import app.hocket.playback.pebble.PebbleBridge
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.collect
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch

/**
 * The Media3 [MediaLibraryService] that owns the process's core.
 *
 * - Creates the core (or the fake) through [CoreHost] and forwards its events:
 *   `Event.Backend` -> [ExoBackend], `Event.MediaSession` and `Event.QueueChanged` ->
 *   [MediaSessionBridge] (the session's state and its queue timeline).
 * - Publishes the library to other apps (Android Auto, Wear, Assistant, `MediaBrowser`s) through
 *   [LibraryBrowser] on the first server, telling subscribed browsers when it changes, only while
 *   "Allow control by other apps" (`media.externalControl`, [ExternalControl]) is on or the app has
 *   notification access.
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
 * - Started by a media key (playback resumption from a stopped service), holds the foreground until
 *   Media3's playback notification takes over or it is clear nothing will play ([ForegroundHold]).
 * - Keeps the home-screen [NowPlayingWidget] in step with the session and the playing Connect device.
 * - Owns the [PebbleBridge] (the Pebble watchapp's phone side), fed the same events; the watch's
 *   messages reach it through [app.hocket.playback.pebble.PebbleListenerService], which binds here.
 * - Exposes a [LocalBinder] so the app process can obtain the [CoreHandle] by binding. Only the
 *   app's own bind counts as a UI client: the service is exported for Media3, so the bind intent
 *   carries [CoreHost.bindToken], which no other process can know.
 */
class PlaybackService : MediaLibraryService() {
    companion object {
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
    lateinit var pebble: PebbleBridge
        private set
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val main = Handler(Looper.getMainLooper())
    private var idleJob: Job? = null
    private var boundClients = 0
    private var clockOffsetMs = 0.0
    private var routeDiscovery: Any? = null
    private var serverId: String? = null
    private var libraryNotify: Job? = null
    private val foregroundHold = ForegroundHold(this, scope) { isPlaybackOngoing }
    private val audioRoutes = AudioRouteLog(this)
    private var cast: CastController? = null
    /** The Cast receiver playback goes to, while a session is up. */
    private var casting: String? = null

    override fun onCreate() {
        super.onCreate()
        core = CoreHost.acquire(this)
        PlaybackLog.i("service created")
        backend = ExoBackend(this, scope, ::dispatch)
        val launch = packageManager.getLaunchIntentForPackage(packageName)
        val browser = LibraryBrowser(this, { core }, { serverId }, scope)
        val player = CoreSessionPlayer(Looper.getMainLooper(), ::dispatch, media = browser, artwork = { t -> t.coverArt?.let { ArtworkProvider.uri(this, it) } })
        bridge = MediaSessionBridge(this, player, ::dispatch, launch, browser, scope, ExternalControl(this))
        addSession(bridge.session)
        network = NetworkMonitor(this, ::dispatch, onConnectivity = { online -> main.post { backend.onConnectivityChanged(online) } })
        battery = BatterySaverMonitor(this, ::dispatch)
        pebble = PebbleBridge(this, { core }, { serverId }, scope)
        // Subscribed before the snapshot is requested, so its `Started` cannot be missed. Collected
        // on the core's event thread (Unconfined), not main: only what [onEvent] or the watch act
        // on is posted, so position ticks, download and sync progress never wake the main looper.
        scope.launch(Dispatchers.Unconfined, start = CoroutineStart.UNDISPATCHED) {
            val gate = ServiceEventGate()
            // Posted before onDestroy cancelled [scope] but run after: the bridge and backend are released.
            core.events.collect { event -> if (gate.admit(event)) main.post { if (scope.isActive) onEvent(event) } }
        }
        network.start()
        battery.start()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            routeDiscovery = runCatching { ConnectRouteDiscovery(this, CastDevices.provider?.routeFeatures(this).orEmpty()).also { it.start() } }
                .onFailure { PlaybackLog.w("route discovery unavailable: ${it.message}") }.getOrNull()
        }
        audioRoutes.start()
        startCast()
        core.dispatch(Command.RequestSnapshot)
        scheduleIdleStop()
    }

    private fun dispatch(command: Command) = core.dispatch(command)

    private fun onEvent(event: Event) {
        pebble.onEvent(event)
        when (event) {
            is Event.Backend -> if (core.kind == app.hocket.core.CoreKind.Native) onBackendCommand(event.data.command) else Unit
            is Event.MediaSession -> {
                applySession(event.data.state)
                if (event.data.state.isPlaying) idleJob?.cancel() else scheduleIdleStop()
            }
            is Event.DevicesChanged -> {
                ConnectRoutes.onDevices(event.data.devices)
                applySession(bridge.player.state)
            }
            is Event.TransportChanged -> {
                val was = ConnectRoutes.state.value
                ConnectRoutes.onTransport(event.data.transport)
                val now = ConnectRoutes.state.value
                if (now.remote != was.remote || now.remoteVolume != was.remoteVolume) applySession(bridge.player.state)
            }
            is Event.QueueChanged -> {
                bridge.applyQueue(event.data.queue)
                backend.onQueueChanged()
            }
            is Event.ServersChanged -> serverId = event.data.servers.firstOrNull()?.id
            is Event.LibraryChanged -> scheduleLibraryNotify()
            is Event.ConnectionChanged -> clockOffsetMs = event.data.state.clockOffsetMs
            is Event.Started -> applySnapshot(event.data.snapshot)
            is Event.Snapshot -> applySnapshot(event.data.snapshot)
            is Event.SettingChanged -> when (event.data.setting.key) {
                SettingKeys.MEDIA_EXTERNAL_CONTROL -> bridge.setExternalControl(event.data.setting.value.trim() == "true")
            }
            else -> Unit
        }
    }

    /**
     * The core's backend command, to the player playing: the Cast renderer while a session is up,
     * ExoPlayer otherwise. Previews always play on the phone (they are a peek, not the play).
     */
    private fun onBackendCommand(command: BackendCommand) {
        val renderer = cast?.backend
        val preview = command is BackendCommand.PreviewLoad || command is BackendCommand.PreviewPlay ||
            command is BackendCommand.PreviewPause || command is BackendCommand.PreviewSeek || command is BackendCommand.PreviewStop
        if (casting != null && renderer != null && !preview) renderer.handle(command) else backend.handle(command)
    }

    private fun startCast() {
        val provider = CastDevices.provider ?: return
        val controller = runCatching {
            provider.create(this, scope, ::dispatch, object : CastListener {
                override fun onSessionStarted(name: String) = main.post { castTo(name) }.let { }
                override fun onSessionEnded(error: String?) = main.post { castTo(null, error) }.let { }
            })
        }.onFailure { PlaybackLog.w("cast unavailable: ${it.message}") }.getOrNull()
        cast = controller
        CastDevices.attach(controller)
    }

    /**
     * Playback moves to the Cast receiver [name], or back here (null). The phone's player stops and
     * goes quiet first, so nothing it says about what it played reaches the core; the core then
     * reloads the current item at its position on the player taking over (`Command.SetRenderer`):
     * on the receiver it carries on, back here it waits, paused.
     */
    private fun castTo(name: String?, error: String? = null) {
        val renderer = cast?.backend ?: return
        if (error != null) android.widget.Toast.makeText(this, error, android.widget.Toast.LENGTH_LONG).show()
        if (name == casting) return
        PlaybackLog.i(if (name != null) "casting to $name" else "casting ended")
        val was = casting
        casting = name
        if (name != null && was == null) {
            backend.active = false
            backend.handle(BackendCommand.Stop)
            renderer.setActive(true)
        } else if (name == null) {
            renderer.setActive(false)
            backend.active = true
        }
        dispatch(Commands.setRenderer(name))
        applySession(bridge.player.state)
    }

    /** `Started` (once per core) and `Snapshot` (every RequestSnapshot) carry the same state. */
    private fun applySnapshot(snapshot: app.hocket.core.api.Snapshot) {
        clockOffsetMs = snapshot.connection.clockOffsetMs
        serverId = snapshot.servers.firstOrNull()?.id
        bridge.applyQueue(snapshot.queue)
        ConnectRoutes.onDevices(snapshot.devices)
        ConnectRoutes.onTransport(snapshot.transport)
        applySession(snapshot.mediaSession)
        bridge.setExternalControl(snapshot.settings.firstOrNull { it.key == SettingKeys.MEDIA_EXTERNAL_CONTROL }?.value?.trim() == "true")
    }

    /** Session state to the bridge, remote when another Connect device plays it. */
    private fun applySession(state: app.hocket.core.api.MediaSessionState) {
        ConnectRoutes.onMediaSession(state)
        val routes = ConnectRoutes.state.value
        val remote = routes.remote
        // Casting plays elsewhere too: the session's volume (keys, slider) is the receiver's.
        val casting = casting != null && remote == null
        bridge.apply(state, clockOffsetMs, remote = remote != null || casting, remoteVolume = routes.remoteVolume != null || casting)
        NowPlayingWidget.update(this, WidgetState.of(state, remote?.name))
    }

    private fun scheduleLibraryNotify() {
        libraryNotify?.cancel()
        libraryNotify = scope.launch {
            delay(LIBRARY_NOTIFY_DEBOUNCE_MS)
            bridge.libraryChanged()
        }
    }

    private fun isBusy(): Boolean = backend.isBusy() || (casting != null && cast?.backend?.isBusy() == true)

    private fun scheduleIdleStop() {
        idleJob?.cancel()
        idleJob = scope.launch {
            delay(IDLE_TIMEOUT_MS)
            if (!bridge.player.state.isPlaying && boundClients == 0 && !isBusy()) {
                PlaybackLog.i("idle for ${IDLE_TIMEOUT_MS / 1000}s with no clients; stopping")
                pauseAllPlayersAndStopSelf()
            } else if (boundClients == 0) {
                // Still playing, buffering or recovering: look again later.
                scheduleIdleStop()
            }
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        PlaybackLog.i("start command #$startId: ${PlaybackLog.intent(intent)}")
        val result = super.onStartCommand(intent, flags, startId)
        if (intent?.action == Intent.ACTION_MEDIA_BUTTON) foregroundHold.hold()
        return result
    }

    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaLibrarySession = bridge.session

    override fun onBind(intent: Intent?): IBinder? {
        if (isLocalBind(intent)) {
            boundClients++
            PlaybackLog.d("app bound ($boundClients bound)")
            idleJob?.cancel()
            return LocalBinder()
        }
        // Media3 controllers, and anything else (a foreign BIND_CORE gets nothing and is not counted).
        return super.onBind(intent)
    }

    override fun onUnbind(intent: Intent?): Boolean {
        if (isLocalBind(intent)) {
            boundClients = (boundClients - 1).coerceAtLeast(0)
            PlaybackLog.d("app unbound ($boundClients bound)")
            if (!bridge.player.state.isPlaying) scheduleIdleStop()
            return true
        }
        return super.onUnbind(intent)
    }

    override fun onRebind(intent: Intent?) {
        if (isLocalBind(intent)) {
            boundClients++
            PlaybackLog.d("app rebound ($boundClients bound)")
            idleJob?.cancel()
        } else {
            super.onRebind(intent)
        }
    }

    override fun onTaskRemoved(rootIntent: Intent?) {
        // The UI unbinds shortly after (ForegroundBinder); with nothing playing the service then goes.
        // A player that is still buffering or recovering from a network error is not "nothing".
        val stop = !bridge.player.state.isPlaying && !isBusy()
        PlaybackLog.i(if (stop) "task removed with nothing playing; stopping" else "task removed while playing; staying")
        if (stop) pauseAllPlayersAndStopSelf()
    }

    override fun onDestroy() {
        PlaybackLog.i("service destroyed")
        audioRoutes.stop()
        // Whatever was showing stays, but nothing plays from here any more.
        NowPlayingWidget.update(this, WidgetState.of(bridge.player.state, null).copy(playing = false))
        network.stop()
        battery.stop()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) (routeDiscovery as? ConnectRouteDiscovery)?.stop()
        ConnectRoutes.clear()
        CastDevices.attach(null)
        cast?.release()
        backend.release()
        foregroundHold.release()
        bridge.release()
        pebble.release()
        scope.cancel()
        // Detaches the core and flushes it on a worker thread (the flush blocks; never on main).
        CoreHost.shutdown()
        super.onDestroy()
    }
}
