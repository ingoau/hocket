package app.hocket.playback

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.net.wifi.WifiInfo
import android.net.wifi.WifiManager
import android.os.Build
import android.os.PowerManager
import app.hocket.core.Commands
import app.hocket.core.api.Command
import app.hocket.core.api.NetworkKind
import app.hocket.core.api.NetworkState
import java.security.MessageDigest

/**
 * Reports connectivity to the core as [Command.SetNetworkState]: kind, metered flag and an opaque
 * network id so transcoding profiles can vary per network. The id is a hash of the SSID when it is
 * readable (needs location permission on 8.1+), otherwise the transport type.
 */
class NetworkMonitor(private val context: Context, private val dispatch: (Command) -> Unit) {
    private val cm = context.getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
    private var last: NetworkState? = null

    private val callback = object : ConnectivityManager.NetworkCallback() {
        override fun onAvailable(network: Network) = publish()
        override fun onLost(network: Network) = publish()
        override fun onCapabilitiesChanged(network: Network, networkCapabilities: NetworkCapabilities) = publish()
    }

    fun start() {
        cm.registerNetworkCallback(NetworkRequest.Builder().addCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET).build(), callback)
        publish()
    }

    fun stop() {
        runCatching { cm.unregisterNetworkCallback(callback) }
    }

    fun current(): NetworkState {
        val caps = cm.activeNetwork?.let { cm.getNetworkCapabilities(it) }
        val kind = when {
            caps == null -> NetworkKind.Offline
            caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) -> NetworkKind.Wifi
            caps.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR) -> NetworkKind.Cellular
            caps.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET) -> NetworkKind.Wired
            else -> NetworkKind.Unknown
        }
        val metered = caps?.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED)?.not() ?: false
        val id = when (kind) {
            NetworkKind.Wifi -> ssidHash(caps) ?: "wifi"
            NetworkKind.Cellular -> "cellular"
            NetworkKind.Wired -> "wired"
            NetworkKind.Offline -> null
            NetworkKind.Unknown -> "unknown"
        }
        return NetworkState(kind, metered, id)
    }

    @Suppress("DEPRECATION")
    private fun ssidHash(caps: NetworkCapabilities?): String? {
        val ssid: String? = try {
            val info = if (Build.VERSION.SDK_INT >= 29) caps?.transportInfo as? WifiInfo else null
            val raw = info?.ssid ?: (context.applicationContext.getSystemService(Context.WIFI_SERVICE) as? WifiManager)?.connectionInfo?.ssid
            raw?.takeIf { it != WifiManager.UNKNOWN_SSID && it != "<unknown ssid>" && it.isNotBlank() }
        } catch (e: SecurityException) {
            null
        }
        ssid ?: return null
        val digest = MessageDigest.getInstance("SHA-256").digest(ssid.toByteArray())
        return "ssid:" + digest.take(8).joinToString("") { "%02x".format(it) }
    }

    private fun publish() {
        val state = current()
        if (state != last) {
            last = state
            dispatch(Commands.setNetworkState(state))
        }
    }
}

/**
 * Mirrors the OS power-save mode into [Command.SetBatterySaver] while the "engage automatically on
 * battery" setting is on. The core owns the mode itself; this only feeds the trigger.
 */
class BatterySaverMonitor(private val context: Context, private val dispatch: (Command) -> Unit) {
    private val pm = context.getSystemService(Context.POWER_SERVICE) as PowerManager
    @Volatile
    var automatic: Boolean = true
        set(value) {
            field = value
            publish()
        }
    private var last: Boolean? = null

    private val receiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) = publish()
    }

    fun start() {
        context.registerReceiver(receiver, IntentFilter(PowerManager.ACTION_POWER_SAVE_MODE_CHANGED))
        publish()
    }

    fun stop() {
        runCatching { context.unregisterReceiver(receiver) }
    }

    private fun publish() {
        val enabled = automatic && pm.isPowerSaveMode
        if (enabled != last) {
            last = enabled
            dispatch(Commands.setBatterySaver(enabled))
        }
    }
}
