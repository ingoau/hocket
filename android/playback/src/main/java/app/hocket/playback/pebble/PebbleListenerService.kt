package app.hocket.playback.pebble

import android.content.ComponentName
import android.content.Context
import android.content.ServiceConnection
import android.os.IBinder
import app.hocket.playback.PlaybackService
import app.hocket.playback.PlaybackServiceConnection
import io.rebble.pebblekit2.client.BasePebbleListenerService
import io.rebble.pebblekit2.common.model.PebbleDictionary
import io.rebble.pebblekit2.common.model.ReceiveResult
import io.rebble.pebblekit2.common.model.WatchIdentifier
import java.util.UUID
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.filterNotNull
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull

/**
 * What the Pebble app binds while the Hocket watchapp is open. It binds [PlaybackService] in turn
 * (the in-process core bind, which also keeps the service and its core alive while the watch is in
 * use) and hands everything to the service's [PebbleBridge].
 */
class PebbleListenerService : BasePebbleListenerService() {
    private val bridge = MutableStateFlow<PebbleBridge?>(null)
    private var bound = false

    private val connection = object : ServiceConnection {
        override fun onServiceConnected(name: ComponentName, service: IBinder?) {
            bridge.value = (service as? PlaybackService.LocalBinder)?.service?.pebble
            if (bridge.value == null) PebbleLog.w("playback service bound without a Pebble bridge")
        }

        override fun onServiceDisconnected(name: ComponentName) {
            PebbleLog.w("playback service disconnected")
            bridge.value = null
        }
    }

    override fun onCreate() {
        super.onCreate()
        bound = bindService(PlaybackServiceConnection.bindIntent(this), connection, Context.BIND_AUTO_CREATE)
        PebbleLog.i("Pebble app bound the listener${if (bound) "" else "; binding the playback service failed"}")
    }

    override fun onDestroy() {
        PebbleLog.i("Pebble app unbound the listener")
        if (bound) runCatching { unbindService(connection) }
        super.onDestroy()
    }

    private suspend fun bridge(): PebbleBridge? = withTimeoutOrNull(BIND_TIMEOUT_MS) { bridge.filterNotNull().first() }

    override suspend fun onMessageReceived(watchappUUID: UUID, data: PebbleDictionary, watch: WatchIdentifier): ReceiveResult {
        if (watchappUUID != PebbleProtocol.APP_UUID) return ReceiveResult.Nack
        val b = bridge() ?: run {
            PebbleLog.w("nacked a watch message: no playback service after ${BIND_TIMEOUT_MS / 1000} s")
            return ReceiveResult.Nack
        }
        b.onMessage(data)
        return ReceiveResult.Ack
    }

    override fun onAppOpened(watchappUUID: UUID, watch: WatchIdentifier) {
        if (watchappUUID == PebbleProtocol.APP_UUID) coroutineScope.launch { bridge()?.appOpened() ?: PebbleLog.w("watchapp opened, but no playback service after ${BIND_TIMEOUT_MS / 1000} s") }
    }

    override fun onAppClosed(watchappUUID: UUID, watch: WatchIdentifier) {
        if (watchappUUID == PebbleProtocol.APP_UUID) bridge.value?.appClosed() ?: PebbleLog.i("watchapp closed (no playback service bound)")
    }

    companion object {
        private const val BIND_TIMEOUT_MS = 10_000L
    }
}
