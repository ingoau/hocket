package app.hocket.playback

import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.os.IBinder
import android.util.Log
import app.hocket.core.CoreHandle
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * Binds the app to [PlaybackService] and exposes the [CoreHandle] the service delivered.
 *
 * [bind] runs while the process is in the foreground ([ForegroundBinder]), so a plain `startService`
 * is allowed: it keeps the service for its idle timeout after the UI unbinds, and Media3 promotes it
 * to foreground itself when playback begins. It is never `startForegroundService`: nothing on this
 * path guarantees a `startForeground` follows, and Media3 only calls it while something plays, so an
 * FGS start would time out (or throw when the process is restarted in the background).
 *
 * [unbind] keeps the delivered handle: the service, and with it the core, outlives the UI while
 * playing or until its idle stop. [app.hocket.HocketApp] pairs this flow with [CoreHost.handle] so
 * the UI's client exists only for a handle that is both delivered and still live.
 */
class PlaybackServiceConnection(private val context: Context) {
    private val _core = MutableStateFlow<CoreHandle?>(null)
    val core: StateFlow<CoreHandle?> = _core.asStateFlow()
    var bound = false
        private set

    private val connection = object : ServiceConnection {
        override fun onServiceConnected(name: ComponentName, service: IBinder?) {
            _core.value = (service as? PlaybackService.LocalBinder)?.core
        }

        override fun onServiceDisconnected(name: ComponentName) {
            _core.value = null
        }

        override fun onBindingDied(name: ComponentName) {
            _core.value = null
        }
    }

    fun bind() {
        if (bound) return
        try {
            context.startService(Intent(context, PlaybackService::class.java))
        } catch (e: IllegalStateException) {
            // Background execution limits: the bind alone still brings the service up (BIND_AUTO_CREATE).
            Log.w(TAG, "plain start refused (background); binding only: ${e.message}")
        }
        bound = context.bindService(bindIntent(context), connection, Context.BIND_AUTO_CREATE)
    }

    fun unbind() {
        if (!bound) return
        runCatching { context.unbindService(connection) }
        bound = false
    }

    companion object {
        private const val TAG = "PlaybackServiceConnection"

        /** The in-process bind: action plus the per-process token that [PlaybackService.onBind] checks. */
        fun bindIntent(context: Context): Intent = Intent(context, PlaybackService::class.java)
            .setAction(PlaybackService.ACTION_BIND_CORE)
            .putExtra(PlaybackService.EXTRA_BIND_TOKEN, CoreHost.bindToken)
    }
}
