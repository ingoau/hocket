package app.hocket.playback

import android.content.Intent
import android.os.Binder
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.util.Log
import androidx.media3.session.MediaSession
import androidx.media3.session.MediaSessionService
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
 * The Media3 [MediaSessionService] that owns the process's core.
 *
 * - Creates the core (or the fake) through [CoreHost] and forwards its events:
 *   `Event.Backend` -> [ExoBackend], `Event.MediaSession` -> [MediaSessionBridge].
 * - Registers the [NetworkMonitor] and [BatterySaverMonitor].
 * - Stays a foreground service (type `mediaPlayback`) while the media session says something is
 *   playing; Media3 handles the notification and foreground promotion. When nothing has played for
 *   [IDLE_TIMEOUT_MS] and no UI client is bound, it stops itself.
 * - Exposes a [LocalBinder] so the app process can obtain the [CoreHandle] by binding. Only the
 *   app's own bind counts as a UI client: the service is exported for Media3, so the bind intent
 *   carries [CoreHost.bindToken], which no other process can know.
 */
class PlaybackService : MediaSessionService() {
    companion object {
        private const val TAG = "PlaybackService"
        const val IDLE_TIMEOUT_MS = 5 * 60_000L
        const val ACTION_BIND_CORE = "app.hocket.playback.BIND_CORE"
        const val EXTRA_BIND_TOKEN = "app.hocket.playback.BIND_TOKEN"

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

    override fun onCreate() {
        super.onCreate()
        core = CoreHost.acquire(this)
        backend = ExoBackend(this, scope, ::dispatch)
        val launch = packageManager.getLaunchIntentForPackage(packageName)
        bridge = MediaSessionBridge(this, CoreSessionPlayer(Looper.getMainLooper(), ::dispatch), ::dispatch, launch)
        network = NetworkMonitor(this, ::dispatch)
        battery = BatterySaverMonitor(this, ::dispatch)
        // Subscribed before the snapshot is requested, so its `Started` cannot be missed.
        scope.launch(start = CoroutineStart.UNDISPATCHED) { core.events.collect { event -> main.post { onEvent(event) } } }
        network.start()
        battery.start()
        core.dispatch(Command.RequestSnapshot)
        scheduleIdleStop()
    }

    private fun dispatch(command: Command) = core.dispatch(command)

    private fun onEvent(event: Event) {
        when (event) {
            is Event.Backend -> if (core.kind == app.hocket.core.CoreKind.Native) backend.handle(event.data.command) else Unit
            is Event.MediaSession -> {
                bridge.apply(event.data.state, clockOffsetMs)
                if (event.data.state.isPlaying) idleJob?.cancel() else scheduleIdleStop()
            }
            is Event.ConnectionChanged -> clockOffsetMs = event.data.state.clockOffsetMs
            is Event.Started -> applySnapshot(event.data.snapshot)
            is Event.Snapshot -> applySnapshot(event.data.snapshot)
            is Event.SettingChanged -> if (event.data.setting.key == SettingKeys.BATTERY_AUTO_ENGAGE) battery.automatic = event.data.setting.value.trim() != "false"
            else -> Unit
        }
    }

    /** `Started` (once per core) and `Snapshot` (every RequestSnapshot) carry the same state. */
    private fun applySnapshot(snapshot: app.hocket.core.api.Snapshot) {
        clockOffsetMs = snapshot.connection.clockOffsetMs
        bridge.apply(snapshot.mediaSession, clockOffsetMs)
        battery.automatic = snapshot.settings.firstOrNull { it.key == SettingKeys.BATTERY_AUTO_ENGAGE }?.value?.trim() != "false"
    }

    private fun scheduleIdleStop() {
        idleJob?.cancel()
        idleJob = scope.launch {
            delay(IDLE_TIMEOUT_MS)
            if (!bridge.player.state.isPlaying && boundClients == 0) {
                Log.i(TAG, "Idle for ${IDLE_TIMEOUT_MS / 1000}s with no clients; stopping")
                pauseAllPlayersAndStopSelf()
            }
        }
    }

    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaSession = bridge.session

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
        if (!bridge.player.state.isPlaying) pauseAllPlayersAndStopSelf()
    }

    override fun onDestroy() {
        network.stop()
        battery.stop()
        backend.release()
        bridge.release()
        scope.cancel()
        // Detaches the core and flushes it on a worker thread (the flush blocks; never on main).
        CoreHost.shutdown()
        super.onDestroy()
    }
}
