package app.hocket.playback

import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.os.IBinder
import androidx.core.content.ContextCompat
import app.hocket.core.CoreHandle
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * Binds the app to [PlaybackService] and exposes the service's [CoreHandle] once connected. The
 * service is also started (as a plain start; Media3 promotes it to foreground when playback begins)
 * so that it outlives the activity while something plays.
 */
class PlaybackServiceConnection(private val context: Context) {
    private val _core = MutableStateFlow<CoreHandle?>(null)
    val core: StateFlow<CoreHandle?> = _core.asStateFlow()
    private var bound = false

    private val connection = object : ServiceConnection {
        override fun onServiceConnected(name: ComponentName, service: IBinder) {
            _core.value = (service as PlaybackService.LocalBinder).core
        }

        override fun onServiceDisconnected(name: ComponentName) {
            _core.value = null
        }
    }

    fun bind() {
        if (bound) return
        val intent = Intent(context, PlaybackService::class.java).setAction(PlaybackService.ACTION_BIND_CORE)
        ContextCompat.startForegroundService(context, Intent(context, PlaybackService::class.java))
        bound = context.bindService(intent, connection, Context.BIND_AUTO_CREATE)
    }

    fun unbind() {
        if (!bound) return
        runCatching { context.unbindService(connection) }
        bound = false
        _core.value = null
    }
}
