package app.hocket.playback

import android.app.Application
import android.content.Context
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.api.Command
import app.hocket.core.api.NetworkKind
import app.hocket.core.api.NetworkState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.shadows.ShadowNetwork
import org.robolectric.shadows.ShadowLooper
import org.robolectric.shadows.ShadowNetworkCapabilities
import java.util.concurrent.TimeUnit

/** What [NetworkMonitor] reports from the default network's capabilities. */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [34], application = Application::class)
class NetworkMonitorTest {
    private val context: Context = ApplicationProvider.getApplicationContext()

    /** Validated capabilities for [transport], plus [capabilities]. */
    private fun caps(transport: Int, vararg capabilities: Int): NetworkCapabilities =
        unvalidated(transport, NetworkCapabilities.NET_CAPABILITY_VALIDATED, *capabilities)

    private fun unvalidated(transport: Int, vararg capabilities: Int): NetworkCapabilities =
        ShadowNetworkCapabilities.newInstance().also { c ->
            shadowOf(c).addTransportType(transport)
            capabilities.forEach { shadowOf(c).addCapability(it) }
        }

    private val monitor = NetworkMonitor(context, {})

    @Test
    fun noDefaultNetworkIsOffline() {
        assertEquals(NetworkState(NetworkKind.Offline, false, null), monitor.stateOf(null))
    }

    @Test
    fun mobileDataIsMeteredAndHasNoNetworkId() {
        // Every metered connection shares the `metered` profile: no per-network id for mobile data.
        assertEquals(NetworkState(NetworkKind.Cellular, true, null), monitor.stateOf(caps(NetworkCapabilities.TRANSPORT_CELLULAR)))
    }

    @Test
    fun temporarilyUnmeteredCountsAsUnmetered() {
        val c = caps(NetworkCapabilities.TRANSPORT_CELLULAR, NetworkCapabilities.NET_CAPABILITY_TEMPORARILY_NOT_METERED)
        assertFalse(NetworkMonitor.isMetered(c))
        assertFalse(monitor.stateOf(c).metered)
    }

    @Test
    fun wifiWithoutNotMeteredIsAHotspot() {
        val hotspot = monitor.stateOf(caps(NetworkCapabilities.TRANSPORT_WIFI))
        assertEquals(NetworkKind.Wifi, hotspot.kind)
        assertTrue(hotspot.metered)
        assertFalse(monitor.stateOf(caps(NetworkCapabilities.TRANSPORT_WIFI, NetworkCapabilities.NET_CAPABILITY_NOT_METERED)).metered)
    }

    @Test
    fun ethernetIsWired() {
        assertEquals(NetworkState(NetworkKind.Wired, false, "wired"), monitor.stateOf(caps(NetworkCapabilities.TRANSPORT_ETHERNET, NetworkCapabilities.NET_CAPABILITY_NOT_METERED)))
    }

    @Test
    fun captivePortalsAndUnvalidatedMobileDataAreOffline() {
        val offline = NetworkState(NetworkKind.Offline, false, null)
        assertEquals(offline, monitor.stateOf(caps(NetworkCapabilities.TRANSPORT_WIFI, NetworkCapabilities.NET_CAPABILITY_CAPTIVE_PORTAL)))
        assertEquals(offline, monitor.stateOf(unvalidated(NetworkCapabilities.TRANSPORT_WIFI, NetworkCapabilities.NET_CAPABILITY_CAPTIVE_PORTAL)))
        assertEquals(offline, monitor.stateOf(unvalidated(NetworkCapabilities.TRANSPORT_CELLULAR)))
    }

    @Test
    fun anUnvalidatedLocalNetworkStaysOnlineForALanServer() {
        // No internet, but a self-hosted server on the LAN may answer; the core probes it.
        assertEquals(NetworkKind.Wifi, monitor.stateOf(unvalidated(NetworkCapabilities.TRANSPORT_WIFI, NetworkCapabilities.NET_CAPABILITY_NOT_METERED)).kind)
        assertEquals(NetworkKind.Wired, monitor.stateOf(unvalidated(NetworkCapabilities.TRANSPORT_ETHERNET)).kind)
    }

    @Test
    fun aHandoverThroughNoNetworkIsReportedOnlyWhereItEnds() {
        val cm = context.getSystemService(ConnectivityManager::class.java)
        val sent = mutableListOf<NetworkState>()
        val online = mutableListOf<Boolean>()
        val m = NetworkMonitor(context, { c -> if (c is Command.SetNetworkState) sent += c.data.state }, onConnectivity = { online += it })
        m.start()
        val callback = shadowOf(cm).networkCallbacks.single()
        val wifi = ShadowNetwork.newInstance(1)
        val cell = ShadowNetwork.newInstance(2)
        callback.onAvailable(wifi)
        callback.onCapabilitiesChanged(wifi, caps(NetworkCapabilities.TRANSPORT_WIFI, NetworkCapabilities.NET_CAPABILITY_NOT_METERED))
        ShadowLooper.idleMainLooper(NetworkMonitor.DEBOUNCE_MS, TimeUnit.MILLISECONDS)
        assertEquals(NetworkKind.Wifi, sent.last().kind)
        val before = sent.size
        val onlineBefore = online.size
        // Wi-Fi goes, mobile data takes over a moment later: no Offline in between.
        callback.onLost(wifi)
        ShadowLooper.idleMainLooper(NetworkMonitor.DEBOUNCE_MS / 2, TimeUnit.MILLISECONDS)
        callback.onAvailable(cell)
        callback.onCapabilitiesChanged(cell, caps(NetworkCapabilities.TRANSPORT_CELLULAR))
        assertEquals(before, sent.size)
        ShadowLooper.idleMainLooper(NetworkMonitor.DEBOUNCE_MS, TimeUnit.MILLISECONDS)
        assertEquals(listOf(NetworkState(NetworkKind.Cellular, true, null)), sent.drop(before))
        assertEquals(onlineBefore, online.size)
        // A loss that holds is reported.
        callback.onLost(cell)
        ShadowLooper.idleMainLooper(NetworkMonitor.DEBOUNCE_MS, TimeUnit.MILLISECONDS)
        assertEquals(NetworkState(NetworkKind.Offline, false, null), sent.last())
        assertEquals(false, online.last())
        m.stop()
    }

    @Test
    fun followsTheDefaultNetworkCallbackAndSkipsRepeats() {
        val cm = context.getSystemService(ConnectivityManager::class.java)
        val sent = mutableListOf<NetworkState>()
        val online = mutableListOf<Boolean>()
        val m = NetworkMonitor(context, { c -> if (c is Command.SetNetworkState) sent += c.data.state }, onConnectivity = { online += it }, debounceMs = 0)
        m.start()
        // Whatever the default network is now, it is reported once at start.
        assertEquals(1, sent.size)
        val callback = shadowOf(cm).networkCallbacks.single()
        val wifi = ShadowNetwork.newInstance(1)
        val cell = ShadowNetwork.newInstance(2)
        callback.onAvailable(wifi)
        callback.onCapabilitiesChanged(wifi, caps(NetworkCapabilities.TRANSPORT_WIFI, NetworkCapabilities.NET_CAPABILITY_NOT_METERED))
        assertEquals(NetworkKind.Wifi to false, sent.last().kind to sent.last().metered)
        // Unchanged capabilities say nothing new.
        val afterWifi = sent.size
        callback.onCapabilitiesChanged(wifi, caps(NetworkCapabilities.TRANSPORT_WIFI, NetworkCapabilities.NET_CAPABILITY_NOT_METERED))
        assertEquals(afterWifi, sent.size)
        // The default moves to mobile data: no onLost for Wi-Fi, just the new network.
        callback.onAvailable(cell)
        callback.onCapabilitiesChanged(cell, caps(NetworkCapabilities.TRANSPORT_CELLULAR))
        assertEquals(NetworkState(NetworkKind.Cellular, true, null), sent.last())
        // A late loss of the old network changes nothing; losing the current one is offline.
        val afterCell = sent.size
        callback.onLost(wifi)
        assertEquals(afterCell, sent.size)
        callback.onLost(cell)
        assertEquals(NetworkState(NetworkKind.Offline, false, null), sent.last())
        assertEquals(false, online.last())
        assertTrue(sent.zipWithNext().none { (a, b) -> a == b })
        m.stop()
        assertTrue(shadowOf(cm).networkCallbacks.isEmpty())
    }

    @Test
    fun aBlockedDefaultNetworkIsOffline() {
        val sent = mutableListOf<NetworkState>()
        val online = mutableListOf<Boolean>()
        val m = NetworkMonitor(context, { c -> if (c is Command.SetNetworkState) sent += c.data.state }, onConnectivity = { online += it }, debounceMs = 0)
        m.start()
        val callback = shadowOf(context.getSystemService(ConnectivityManager::class.java)).networkCallbacks.single()
        val wifi = ShadowNetwork.newInstance(1)
        callback.onAvailable(wifi)
        callback.onCapabilitiesChanged(wifi, caps(NetworkCapabilities.TRANSPORT_WIFI, NetworkCapabilities.NET_CAPABILITY_NOT_METERED))
        assertEquals(NetworkKind.Wifi, sent.last().kind)
        // Background data restricted: the network is there, but none of the app's traffic gets through.
        callback.onBlockedStatusChanged(wifi, true)
        assertEquals(NetworkState(NetworkKind.Offline, false, null), sent.last())
        assertEquals(false, online.last())
        // A status for some other network says nothing about the default.
        val before = sent.size
        callback.onBlockedStatusChanged(ShadowNetwork.newInstance(2), false)
        assertEquals(before, sent.size)
        callback.onBlockedStatusChanged(wifi, false)
        assertEquals(NetworkState(NetworkKind.Wifi, false, null), sent.last())
        assertEquals(true, online.last())
        m.stop()
    }

    @Test
    fun theStartUpReadIsDroppedOnceTheCallbackHasSpoken() {
        val sent = mutableListOf<NetworkState>()
        val m = NetworkMonitor(context, { c -> if (c is Command.SetNetworkState) sent += c.data.state }, debounceMs = 0)
        m.start()
        val callback = shadowOf(context.getSystemService(ConnectivityManager::class.java)).networkCallbacks.single()
        val cell = ShadowNetwork.newInstance(2)
        callback.onAvailable(cell)
        callback.onCapabilitiesChanged(cell, caps(NetworkCapabilities.TRANSPORT_CELLULAR))
        val reported = sent.toList()
        // A start-up read landing after the callback (on 12+ with the SSID redacted) reports nothing.
        m.seed(ShadowNetwork.newInstance(1), caps(NetworkCapabilities.TRANSPORT_WIFI, NetworkCapabilities.NET_CAPABILITY_NOT_METERED))
        assertEquals(reported, sent)
        m.stop()
    }
}
