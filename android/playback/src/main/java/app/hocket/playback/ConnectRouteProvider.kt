package app.hocket.playback

import android.content.Context
import android.media.MediaRoute2Info
import android.media.MediaRoute2ProviderService
import android.media.MediaRouter2
import android.media.RouteDiscoveryPreference
import android.media.RoutingSessionInfo
import android.os.Build
import android.os.Bundle
import android.os.SystemClock
import android.util.Log
import androidx.annotation.RequiresApi
import app.hocket.core.Commands
import app.hocket.core.api.Command
import app.hocket.core.api.DeviceInfo
import app.hocket.core.api.Platform
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch

/**
 * Publishes the other Hocket devices of the Connect room as MediaRouter2 routes, so they appear in
 * the system output switcher and the media controls' output chip names the device that plays.
 *
 * - Routes: one per other device ([ConnectRoutes.State.devices]), feature [ConnectRoutes.FEATURE]
 *   (the app's discovery preference, registered by [PlaybackService]) plus remote playback.
 * - While another device plays, one routing session [ConnectRoutes.SESSION_ID] is kept with that
 *   device selected and its name; the media session reports remote playback with the same id, which
 *   is how SystemUI ties the two. Created unprompted (`REQUEST_ID_NONE`) when playback moved there
 *   some other way (the picker, the other device), released when it comes back or stops.
 * - Picking a device in the output switcher is a `HandoffTo` it; picking this phone (the session
 *   released) is a `HandoffTo` this device. The core decides everything; this only translates.
 * - Volume is fixed: Connect volume is per device and the core does not forward it, so the switcher
 *   offers no slider for a remote device rather than one that moves nothing.
 */
@RequiresApi(Build.VERSION_CODES.R)
class ConnectRouteProvider : MediaRoute2ProviderService() {
    private companion object {
        const val TAG = "ConnectRouteProvider"
        /** How long a switcher pick keeps its session before the core confirms the handoff. */
        const val PENDING_MS = 10_000L
    }

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private var state = ConnectRoutes.State()
    /** The device a switcher pick asked for, and when, until the core's state shows it playing. */
    private var pending: Pair<String, Long>? = null
    private var sessionRoute: String? = null

    override fun onCreate() {
        super.onCreate()
        scope.launch { ConnectRoutes.state.collect { state = it; sync() } }
    }

    override fun onDestroy() {
        scope.cancel()
        super.onDestroy()
    }

    private fun dispatch(command: Command) {
        val core = CoreHost.current
        if (core == null) Log.w(TAG, "no core running; dropped $command") else core.dispatch(command)
    }

    /** The device the session should show as selected: the core's, or a pick still in flight. */
    private fun target(): DeviceInfo? {
        state.remote?.let { remote -> if (pending?.first == remote.id) pending = null; return remote }
        val (id, at) = pending ?: return null
        if (SystemClock.elapsedRealtime() - at > PENDING_MS) { pending = null; return null }
        return state.devices.firstOrNull { it.id == id }
    }

    private fun sync() {
        notifyRoutes(state.devices.map(::route))
        val target = target()
        when {
            target == null && sessionRoute != null -> {
                sessionRoute = null
                notifySessionReleased(ConnectRoutes.SESSION_ID)
            }
            target != null && sessionRoute == null -> {
                sessionRoute = target.id
                notifySessionCreated(REQUEST_ID_NONE, session(target))
            }
            target != null -> {
                sessionRoute = target.id
                notifySessionUpdated(session(target))
            }
        }
    }

    private fun route(device: DeviceInfo): MediaRoute2Info = MediaRoute2Info.Builder(device.id, device.name)
        .addFeature(ConnectRoutes.FEATURE)
        .addFeature(MediaRoute2Info.FEATURE_REMOTE_PLAYBACK)
        .setVolumeHandling(MediaRoute2Info.PLAYBACK_VOLUME_FIXED)
        .setConnectionState(if (device.id == state.remote?.id) MediaRoute2Info.CONNECTION_STATE_CONNECTED else MediaRoute2Info.CONNECTION_STATE_DISCONNECTED)
        .apply { if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) setType(routeType(device.platform)) }
        .build()

    private fun session(device: DeviceInfo): RoutingSessionInfo = RoutingSessionInfo.Builder(ConnectRoutes.SESSION_ID, packageName)
        .setName(device.name)
        .addSelectedRoute(device.id)
        .apply { state.devices.filter { it.id != device.id }.forEach { addTransferableRoute(it.id) } }
        .setVolumeHandling(MediaRoute2Info.PLAYBACK_VOLUME_FIXED)
        .build()

    @RequiresApi(Build.VERSION_CODES.UPSIDE_DOWN_CAKE)
    private fun routeType(platform: Platform): Int = when (platform) {
        Platform.Android -> MediaRoute2Info.TYPE_REMOTE_SMARTPHONE
        Platform.Linux, Platform.MacOs, Platform.Windows -> MediaRoute2Info.TYPE_REMOTE_COMPUTER
        Platform.Coordinator -> MediaRoute2Info.TYPE_REMOTE_SPEAKER
    }

    private fun handoffTo(deviceId: String) {
        pending = deviceId to SystemClock.elapsedRealtime()
        dispatch(Commands.handoffTo(deviceId))
    }

    override fun onCreateSession(requestId: Long, packageName: String, routeId: String, sessionHints: Bundle?) {
        val device = state.devices.firstOrNull { it.id == routeId }
        if (device == null || packageName != this.packageName) {
            notifyRequestFailed(requestId, REASON_ROUTE_NOT_AVAILABLE)
            return
        }
        handoffTo(device.id)
        val created = sessionRoute == null
        sessionRoute = device.id
        if (created) notifySessionCreated(requestId, session(device)) else notifySessionUpdated(session(device))
    }

    override fun onReleaseSession(requestId: Long, sessionId: String) {
        if (sessionId != ConnectRoutes.SESSION_ID) return
        // Back to this phone: bring playback here.
        pending = null
        sessionRoute = null
        notifySessionReleased(sessionId)
        val self = state.selfId
        if (state.remote != null && self != null) dispatch(Commands.handoffTo(self))
    }

    override fun onTransferToRoute(requestId: Long, sessionId: String, routeId: String) {
        val device = state.devices.firstOrNull { it.id == routeId }
        if (device == null) {
            notifyRequestFailed(requestId, REASON_ROUTE_NOT_AVAILABLE)
            return
        }
        handoffTo(device.id)
        sessionRoute = device.id
        notifySessionUpdated(session(device))
    }

    override fun onSelectRoute(requestId: Long, sessionId: String, routeId: String) =
        notifyRequestFailed(requestId, REASON_INVALID_COMMAND) // one device plays at a time

    override fun onDeselectRoute(requestId: Long, sessionId: String, routeId: String) =
        notifyRequestFailed(requestId, REASON_INVALID_COMMAND)

    override fun onSetRouteVolume(requestId: Long, routeId: String, volume: Int) =
        notifyRequestFailed(requestId, REASON_INVALID_COMMAND)

    override fun onSetSessionVolume(requestId: Long, sessionId: String, volume: Int) =
        notifyRequestFailed(requestId, REASON_INVALID_COMMAND)
}

/**
 * The app's MediaRouter2 discovery preference for [ConnectRoutes.FEATURE]. It keeps the system bound
 * to [ConnectRouteProvider] and scopes the output switcher's list for this app to Connect devices
 * (no Cast or other providers' routes this app could not play to). Held by [PlaybackService].
 */
@RequiresApi(Build.VERSION_CODES.R)
internal class ConnectRouteDiscovery(private val context: Context) {
    private val router = MediaRouter2.getInstance(context)
    private val callback = object : MediaRouter2.RouteCallback() {}

    fun start() = router.registerRouteCallback(
        context.mainExecutor,
        callback,
        RouteDiscoveryPreference.Builder(listOf(ConnectRoutes.FEATURE), false).build(),
    )

    fun stop() = router.unregisterRouteCallback(callback)
}
