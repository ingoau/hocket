package app.hocket.playback

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.net.ConnectivityManager
import android.net.ConnectivityManager.NetworkCallback.FLAG_INCLUDE_LOCATION_INFO
import android.net.Network
import android.net.NetworkCapabilities
import android.net.wifi.WifiInfo
import android.net.wifi.WifiManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.PowerManager
import android.os.SystemClock
import app.hocket.core.Commands
import app.hocket.core.api.Command
import app.hocket.core.api.NetworkKind
import app.hocket.core.api.NetworkState
import java.security.MessageDigest

/**
 * Reports the default network to the core as [Command.SetNetworkState]: kind, metered flag and an
 * opaque network id so transcoding profiles can vary per network. The default network is the one
 * the app's traffic actually uses (a VPN, or Wi-Fi over mobile data), so kind and metered come from
 * its capabilities as the default-network callback reports them. The id is a hash of the SSID when
 * it is readable (needs location permission), "wired" on Ethernet, and none otherwise: every
 * metered connection shares the core's `metered` profile, so mobile data needs no id of its own.
 *
 * A network counts as online only when it can reach anything: not behind a captive portal, and
 * validated unless it is a local one (Wi-Fi or Ethernet), where a self-hosted server can still be
 * reachable without internet access ([usable]). Changes after the first report settle for
 * [debounceMs] before they're published (at most twice that while it keeps changing), so a handover that briefly has no default network (Wi-Fi
 * lost, mobile data a moment later) or a network that validates right after connecting reports
 * only where it ends up, not an `Offline` blip that resets recovery and artwork retries.
 */
class NetworkMonitor(
    private val context: Context,
    private val dispatch: (Command) -> Unit,
    /** Called with whether there is any connectivity, on every change of that (and once at start). */
    private val onConnectivity: (online: Boolean) -> Unit = {},
    /** Called with the default network's kind on every change of it (and once at start). */
    private val onKind: (NetworkKind) -> Unit = {},
    /** How long a change must hold before it's published; 0 publishes at once. */
    private val debounceMs: Long = DEBOUNCE_MS,
    private val handler: Handler = Handler(Looper.getMainLooper()),
) {
    private val cm = context.getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
    private var last: NetworkState? = null
    /** The default network and its capabilities, as the callback last reported them. */
    private var default: Network? = null
    private var caps: NetworkCapabilities? = null
    /** The system blocks the app's traffic on [default] (background data restriction, data saver): offline to us. */
    private var blocked = false
    /** The callback has said something, so the one-time start-up read ([seed]) is stale and unused. */
    private var heard = false
    /** A state waiting out [debounceMs] before it's published. */
    private var pending: NetworkState? = null
    /** When [pending] started waiting (uptime ms). */
    private var pendingSince = 0L
    /** The longest a change waits however often it changes again. */
    private val maxWait = debounceMs * 2
    private val flush = Runnable { flushPending() }

    // Capabilities always follow onAvailable (API 26+), so only they, the blocked status and onLost
    // publish. On 12+ the SSID is redacted from callback capabilities unless the callback asks for
    // location info.
    private val callback: ConnectivityManager.NetworkCallback =
        if (Build.VERSION.SDK_INT >= 31) {
            object : ConnectivityManager.NetworkCallback(FLAG_INCLUDE_LOCATION_INFO) {
                override fun onAvailable(network: Network) = available(network)
                override fun onCapabilitiesChanged(network: Network, networkCapabilities: NetworkCapabilities) = changed(network, networkCapabilities)
                override fun onBlockedStatusChanged(network: Network, blocked: Boolean) = blockedChanged(network, blocked)
                override fun onLost(network: Network) = lost(network)
            }
        } else {
            object : ConnectivityManager.NetworkCallback() {
                override fun onAvailable(network: Network) = available(network)
                override fun onCapabilitiesChanged(network: Network, networkCapabilities: NetworkCapabilities) = changed(network, networkCapabilities)
                // API 29+; never called before that.
                override fun onBlockedStatusChanged(network: Network, blocked: Boolean) = blockedChanged(network, blocked)
                override fun onLost(network: Network) = lost(network)
            }
        }

    fun start() {
        // The callback first, so no change between the read below and the registration is missed.
        cm.registerDefaultNetworkCallback(callback)
        // With no default network the callback never fires, and this read is the "offline" report the
        // core needs. (The active network is null while the app's traffic is blocked, so that is
        // offline too.)
        val network = cm.activeNetwork
        seed(network, network?.let { cm.getNetworkCapabilities(it) })
    }

    /**
     * The one-time start-up read, used only while the callback hasn't spoken yet: on 12+ this read
     * has the SSID redacted, and reporting it right before the callback's would publish an id-less
     * state and then the real one.
     */
    @Synchronized
    internal fun seed(network: Network?, capabilities: NetworkCapabilities?) {
        if (heard) return
        default = network
        caps = capabilities
        publish()
    }

    fun stop() {
        runCatching { cm.unregisterNetworkCallback(callback) }
        synchronized(this) {
            handler.removeCallbacks(flush)
            pending = null
        }
    }

    @Synchronized
    private fun available(network: Network) {
        heard = true
        // The new default's blocked status follows its capabilities.
        if (network != default) blocked = false
        default = network
    }

    @Synchronized
    private fun changed(network: Network, capabilities: NetworkCapabilities) {
        heard = true
        if (network != default) blocked = false
        default = network
        caps = capabilities
        publish()
    }

    @Synchronized
    private fun blockedChanged(network: Network, isBlocked: Boolean) {
        heard = true
        if (network != default) return
        blocked = isBlocked
        publish()
    }

    @Synchronized
    private fun lost(network: Network) {
        heard = true
        // A default-network callback loses only the last network it made available, and only when
        // nothing replaces it; the guard is for a late call about one already replaced.
        if (network != default) return
        default = null
        caps = null
        blocked = false
        publish()
    }

    /** The state [caps] stands for. */
    internal fun stateOf(caps: NetworkCapabilities?): NetworkState {
        val kind = when {
            caps == null || !usable(caps) -> NetworkKind.Offline
            caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) -> NetworkKind.Wifi
            caps.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR) -> NetworkKind.Cellular
            caps.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET) -> NetworkKind.Wired
            else -> NetworkKind.Unknown
        }
        val id = when (kind) {
            NetworkKind.Wifi -> ssidHash(caps)
            NetworkKind.Wired -> "wired"
            else -> null
        }
        return NetworkState(kind, kind != NetworkKind.Offline && caps != null && isMetered(caps), id)
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
        val state = stateOf(if (blocked) null else caps)
        // The first report goes out at once: the core waits for it.
        if (last == null || debounceMs <= 0) {
            handler.removeCallbacks(flush)
            pending = null
            emit(state)
            return
        }
        // Back where it was reported: nothing to publish. The same state again (capability noise
        // that changes nothing reported): the wait already running stands rather than starting over.
        if (state == last) {
            handler.removeCallbacks(flush)
            pending = null
            return
        }
        if (state == pending) return
        // Each different state waits [debounceMs] afresh, but never past [maxWait] since the first:
        // a network that keeps changing is still reported.
        val now = SystemClock.uptimeMillis()
        if (pending == null) pendingSince = now
        pending = state
        handler.removeCallbacks(flush)
        handler.postDelayed(flush, (pendingSince + maxWait - now).coerceIn(0L, debounceMs))
    }

    @Synchronized
    private fun flushPending() {
        val state = pending ?: return
        pending = null
        emit(state)
    }

    private fun emit(state: NetworkState) {
        val was = last
        if (state != was) {
            last = state
            dispatch(Commands.setNetworkState(state))
            val online = state.kind != NetworkKind.Offline
            if (was == null || (was.kind != NetworkKind.Offline) != online) onConnectivity(online)
            if (was == null || was.kind != state.kind) onKind(state.kind)
        }
    }

    internal companion object {
        /** How long a network change must hold before it's reported. */
        const val DEBOUNCE_MS = 1_000L

        /**
         * Whether [caps] can carry the app's traffic. A captive portal can't until the user signs in.
         * Otherwise the system's validation (it reached the internet) decides, except on Wi-Fi and
         * Ethernet: those may be a LAN with the Navidrome server on it and no internet, and the core
         * finds an unreachable server by probing it anyway.
         */
        fun usable(caps: NetworkCapabilities): Boolean {
            if (caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_CAPTIVE_PORTAL)) return false
            if (caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED)) return true
            return caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) || caps.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET)
        }

        /**
         * Metered unless the network says it isn't. A temporarily unmetered one (a carrier's
         * unmetered 5G, Android 11+) counts as unmetered while it says so.
         */
        fun isMetered(caps: NetworkCapabilities): Boolean {
            if (caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED)) return false
            return !(Build.VERSION.SDK_INT >= 30 && caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_TEMPORARILY_NOT_METERED))
        }
    }
}

/**
 * Mirrors the system's power saver into [Command.SetPowerSave]. The core owns battery saver: it
 * engages `battery.autoLevel` from this while `battery.autoEngage` is on.
 */
class BatterySaverMonitor(private val context: Context, private val dispatch: (Command) -> Unit) {
    private val pm = context.getSystemService(Context.POWER_SERVICE) as PowerManager
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
        val enabled = pm.isPowerSaveMode
        if (enabled != last) {
            last = enabled
            dispatch(Commands.setPowerSave(enabled))
        }
    }
}
