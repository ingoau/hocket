package app.hocket.playback

import android.content.Context
import app.hocket.core.api.BackendCommand
import app.hocket.core.api.Command
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * Cast receivers this phone can play through, and the one it plays through now.
 *
 * Casting is a renderer of this device, not a Connect peer (design.md, Cast): the phone keeps the
 * transport lease, the core resolves server URLs the receiver fetches itself
 * (`Command.SetRenderer`), and [PlaybackService] hands the core's backend commands to the provider's
 * [RendererBackend] instead of [ExoBackend] while a session is up.
 *
 * The implementation lives outside this module so a FOSS build can leave it out: the app installs a
 * [CastProvider] at startup (the Google Cast SDK one in `:cast`), or none, and then [State.available]
 * stays false and the UI shows no Cast devices. A later Rust CastV2 renderer would be another
 * provider behind the same seam.
 *
 * Process-wide, like [AudioOutputs] and [ConnectRoutes]: the service owns the controller, the UI
 * reads [state] and calls [select], [stop] and [scan]. Main thread only.
 */
object CastDevices {
    data class Device(val id: String, val name: String, val description: String? = null)

    data class State(
        /** A provider is installed and working (Google Play services present). */
        val available: Boolean = false,
        /** Receivers on the network, the active one included. */
        val devices: List<Device> = emptyList(),
        /** The receiver a session is up with: playback goes there. */
        val active: Device? = null,
        /** A receiver being connected to. */
        val connecting: String? = null,
    )

    private val _state = MutableStateFlow(State())
    val state: StateFlow<State> = _state.asStateFlow()

    /** Installed by the app before the service starts; null in a FOSS build. */
    @Volatile
    var provider: CastProvider? = null

    private var controller: CastController? = null

    internal fun attach(controller: CastController?) {
        this.controller = controller
        if (controller == null) _state.value = State()
    }

    /** For providers: what they see. */
    fun publish(state: State) {
        _state.value = state
    }

    /** Start casting to [id] (the session's start switches playback over). */
    fun select(id: String) = controller?.select(id)

    /** End the session; playback comes back to this phone, paused. */
    fun stop() = controller?.stop()

    /** Look for receivers actively (the device picker is open) or passively. */
    fun scan(active: Boolean) = controller?.scan(active)
}

/** Makes the [CastController] for the service's lifetime. */
interface CastProvider {
    /** Null when casting can't work here (no Google Play services). */
    fun create(context: Context, scope: CoroutineScope, dispatch: (Command) -> Unit, listener: CastListener): CastController?

    /** MediaRouter2 features the system output switcher should list for this app (Cast receivers). */
    fun routeFeatures(context: Context): List<String>
}

interface CastListener {
    /** A session is up with [name] (started here, from the system output switcher, or resumed). */
    fun onSessionStarted(name: String)

    /** The session ended, from either end, or failed. [error] is shown when there is one. */
    fun onSessionEnded(error: String?)
}

interface CastController {
    val backend: RendererBackend

    fun select(id: String)
    fun stop()
    fun scan(active: Boolean)
    fun release()
}

/** The core's backend commands, played on a remote renderer. Main thread. */
interface RendererBackend {
    fun handle(command: BackendCommand)

    /** Playing, buffering or loading: the service must not stop. */
    fun isBusy(): Boolean

    /** Reports reach the core only while this backend is the active one. */
    fun setActive(active: Boolean)
}
