package app.hocket.playback

import android.content.Context
import android.media.AudioDeviceCallback
import android.media.AudioDeviceInfo
import android.media.AudioManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * The phone's own audio outputs that are connected right now (the speaker, wired or USB headphones,
 * Bluetooth headphones and speakers), for the Connect sheet's "Play on" list, and the one Hocket
 * plays to. Picking one routes this app's playback there with ExoPlayer's preferred audio device,
 * in either direction (earphones to the phone's speaker and back), without touching other apps.
 *
 * Only connected devices are listed: an app cannot connect a paired-but-disconnected Bluetooth
 * device; that is the system's job (the sheet links to Bluetooth settings). In-process, shared
 * between [ExoBackend] (which applies the choice) and the UI (which lists and picks).
 */
object AudioOutputs {
    enum class Kind { Phone, Wired, Usb, Bluetooth, Other }

    data class Output(val id: Int, val name: String, val kind: Kind)

    data class State(
        val outputs: List<Output> = emptyList(),
        /** The output picked in Hocket; null = the system's choice (the newest headset, else the speaker). */
        val preferred: Int? = null,
    ) {
        /** What plays now: the pick while it is still connected, else the system's likely choice. */
        val current: Output? get() = outputs.firstOrNull { it.id == preferred } ?: outputs.maxByOrNull { priority(it.kind) }
    }

    private val state = MutableStateFlow(State())
    val outputs: StateFlow<State> = state.asStateFlow()

    private var manager: AudioManager? = null
    private val devices = HashMap<Int, AudioDeviceInfo>()
    /** Set by [ExoBackend]: applies a preferred device (null = the system's default) to the player. */
    @Volatile private var apply: ((AudioDeviceInfo?) -> Unit)? = null

    private val callback = object : AudioDeviceCallback() {
        override fun onAudioDevicesAdded(added: Array<out AudioDeviceInfo>) = refresh()
        override fun onAudioDevicesRemoved(removed: Array<out AudioDeviceInfo>) = refresh()
    }

    /** Starts listening for outputs coming and going. Idempotent. */
    fun start(context: Context) {
        if (manager != null) return
        val am = context.applicationContext.getSystemService(AudioManager::class.java) ?: return
        manager = am
        am.registerAudioDeviceCallback(callback, Handler(Looper.getMainLooper()))
        refresh()
    }

    /** The player that plays to the chosen output; the current choice applies at once. */
    fun attach(applyPreferred: (AudioDeviceInfo?) -> Unit) {
        apply = applyPreferred
        applyPreferred(state.value.preferred?.let { devices[it] })
    }

    fun detach() { apply = null }

    /** Plays to [id] (one of [outputs]); null hands the choice back to the system. */
    fun select(id: Int?) {
        val device = id?.let { devices[it] }
        state.value = state.value.copy(preferred = device?.id)
        apply?.invoke(device)
    }

    private fun refresh() {
        val am = manager ?: return
        devices.clear()
        val list = am.getDevices(AudioManager.GET_DEVICES_OUTPUTS).mapNotNull { d ->
            val kind = kindOf(d.type) ?: return@mapNotNull null
            devices[d.id] = d
            Output(d.id, if (kind == Kind.Phone) "" else d.productName?.toString().orEmpty(), kind)
        }
            // One row per physical device: Bluetooth shows up as A2DP and SCO, keep the first.
            .distinctBy { if (it.kind == Kind.Phone) "phone" else "${it.kind}:${it.name}" }
        val preferred = state.value.preferred?.takeIf { id -> list.any { it.id == id } }
        // The picked output went away (earphones unplugged): back to the system's choice.
        if (preferred == null && state.value.preferred != null) apply?.invoke(null)
        state.value = State(list, preferred)
    }

    private fun kindOf(type: Int): Kind? = when (type) {
        AudioDeviceInfo.TYPE_BUILTIN_SPEAKER -> Kind.Phone
        AudioDeviceInfo.TYPE_WIRED_HEADPHONES, AudioDeviceInfo.TYPE_WIRED_HEADSET, AudioDeviceInfo.TYPE_LINE_ANALOG -> Kind.Wired
        AudioDeviceInfo.TYPE_USB_HEADSET, AudioDeviceInfo.TYPE_USB_DEVICE -> Kind.Usb
        AudioDeviceInfo.TYPE_BLUETOOTH_A2DP -> Kind.Bluetooth
        else -> if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S && (type == AudioDeviceInfo.TYPE_BLE_HEADSET || type == AudioDeviceInfo.TYPE_BLE_SPEAKER)) Kind.Bluetooth else null
    }

    /** How the system tends to route media: a headset over the speaker, the speaker last. */
    private fun priority(kind: Kind): Int = when (kind) {
        Kind.Bluetooth -> 4
        Kind.Usb -> 3
        Kind.Wired -> 2
        Kind.Other -> 1
        Kind.Phone -> 0
    }
}
