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
 *   [IDLE_TIMEOUT_MS] and no client is bound, it stops itself.
 * - Exposes a [LocalBinder] so the app process can obtain the [CoreHandle] by binding.
 */
class PlaybackService : MediaSessionService() {
    companion object {
        private const val TAG = "PlaybackService"
        const val IDLE_TIMEOUT_MS = 5 * 60_000L
        const val ACTION_BIND_CORE = "app.hocket.playback.BIND_CORE"
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
        scope.launch { core.events.collect { event -> main.post { onEvent(event) } } }
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
            is Event.Started -> {
                clockOffsetMs = event.data.snapshot.connection.clockOffsetMs
                bridge.apply(event.data.snapshot.mediaSession, clockOffsetMs)
                battery.automatic = event.data.snapshot.settings.firstOrNull { it.key == SettingKeys.BATTERY_AUTO_ENGAGE }?.value?.trim() != "false"
            }
            is Event.SettingChanged -> if (event.data.setting.key == SettingKeys.BATTERY_AUTO_ENGAGE) battery.automatic = event.data.setting.value.trim() != "false"
            else -> Unit
        }
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
        if (intent?.action == ACTION_BIND_CORE) {
            boundClients++
            idleJob?.cancel()
            return LocalBinder()
        }
        return super.onBind(intent)
    }

    override fun onUnbind(intent: Intent?): Boolean {
        if (intent?.action == ACTION_BIND_CORE) {
            boundClients = (boundClients - 1).coerceAtLeast(0)
            if (!bridge.player.state.isPlaying) scheduleIdleStop()
            return true
        }
        return super.onUnbind(intent)
    }

    override fun onRebind(intent: Intent?) {
        if (intent?.action == ACTION_BIND_CORE) { boundClients++; idleJob?.cancel() } else super.onRebind(intent)
    }

    override fun onTaskRemoved(rootIntent: Intent?) {
        if (!bridge.player.state.isPlaying) pauseAllPlayersAndStopSelf()
    }

    override fun onDestroy() {
        network.stop()
        battery.stop()
        backend.release()
        bridge.release()
        scope.cancel()
        CoreHost.shutdown()
        super.onDestroy()
    }
}
