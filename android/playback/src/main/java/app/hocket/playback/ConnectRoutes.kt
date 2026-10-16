package app.hocket.playback

import app.hocket.core.api.DeviceInfo
import app.hocket.core.api.MediaSessionState
import app.hocket.core.api.Platform
import app.hocket.core.api.TransportState
import kotlin.math.roundToInt
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * The Connect devices as the system's output switcher sees them, shared in-process between
 * [PlaybackService] (which feeds it from core events) and [ConnectRouteProvider] (which publishes it
 * to MediaRouter2). Pure view state: every decision (who plays, handoff) stays in the core.
 *
 * When another Hocket device plays, the media session reports remote playback whose routing
 * controller id is [SESSION_ID], and the provider publishes a routing session with that id named
 * after the playing device. SystemUI matches the two (volume control id == routing session id, same
 * package) and shows the device's name on the media controls' output chip, the way Spotify Connect
 * shows "Ingo's MacBook Pro".
 */
object ConnectRoutes {
    /** The one routing session the provider owns; also the session's `routingControllerId`. */
    const val SESSION_ID = "hocket-connect"

    /** Route feature the app asks MediaRouter2 to discover; only [ConnectRouteProvider] offers it. */
    const val FEATURE = "app.hocket.feature.CONNECT"

    /** Volume steps of a remote device in the system controls: 5 % a press, like the core's volume up. */
    const val VOLUME_STEPS = 20

    /** A 0–1 volume as system volume steps. */
    fun steps(volume: Double): Int = (volume.coerceIn(0.0, 1.0) * VOLUME_STEPS).roundToInt()

    /** System volume steps as a 0–1 volume. */
    fun level(steps: Int): Double = steps.coerceIn(0, VOLUME_STEPS).toDouble() / VOLUME_STEPS

    data class State(
        /** Other Hocket devices in the session room that can play (never this one). */
        val devices: List<DeviceInfo> = emptyList(),
        /** The device playing when it is not this one and something is loaded; null otherwise. */
        val remote: DeviceInfo? = null,
        /** This device's Connect id, once the core has reported the room. */
        val selfId: String? = null,
        /** [remote]'s volume (0–1) once it has reported one; null otherwise (an older version). */
        val remoteVolume: Double? = null,
        val remoteMuted: Boolean = false,
    )

    private val _state = MutableStateFlow(State())
    val state: StateFlow<State> = _state.asStateFlow()

    private var devices: List<DeviceInfo> = emptyList()
    private var owner: String? = null
    private var transport: TransportState? = null
    private var session: MediaSessionState? = null

    /** Main thread. */
    fun onDevices(devices: List<DeviceInfo>) {
        this.devices = devices
        publish()
    }

    /** Main thread. The lease owner from the transport state. */
    fun onOwner(owner: String?) {
        this.owner = owner
        publish()
    }

    /** Main thread. The transport: its lease owner and the volume of the device the control drives. */
    fun onTransport(transport: TransportState) {
        this.transport = transport
        onOwner(transport.lease.owner)
    }

    /** Main thread. */
    fun onMediaSession(state: MediaSessionState) {
        session = state
        publish()
    }

    /** Back to nothing (the service is going). */
    fun clear() {
        devices = emptyList()
        owner = null
        transport = null
        session = null
        publish()
    }

    private fun publish() {
        val s = session
        val remote = if (s == null) null else remotePlayback(s, devices, owner)
        val t = transport?.takeIf { remote != null && it.volumeDevice == remote.id }
        _state.value = State(
            // A coordinator is a relay, never somewhere to play.
            devices = devices.filter { !it.isSelf && it.platform != Platform.Coordinator },
            remote = remote,
            selfId = devices.firstOrNull { it.isSelf }?.id,
            remoteVolume = t?.volume,
            remoteMuted = t?.muted == true,
        )
    }

    /**
     * The device the session plays on when that is not this one: the lease owner, else whichever
     * device says it plays. Null while this device owns transport or nothing is loaded.
     */
    fun remotePlayback(state: MediaSessionState, devices: List<DeviceInfo>, owner: String?): DeviceInfo? {
        if (state.ownsTransport || state.metadata == null) return null
        val others = devices.filter { !it.isSelf }
        return others.firstOrNull { it.id == owner } ?: others.firstOrNull { it.playing }
    }
}
