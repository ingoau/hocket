package app.hocket.playback

import android.Manifest
import android.bluetooth.BluetoothManager
import android.content.Context
import android.content.pm.PackageManager
import android.media.AudioDeviceCallback
import android.media.AudioDeviceInfo
import android.media.AudioManager
import android.annotation.SuppressLint
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
    /** How an output is connected. */
    enum class Kind { Phone, Wired, Usb, Bluetooth, Hdmi, Dock }

    /**
     * What an output is, for its icon ([OutputForms]). Bluetooth cars, speakers and TVs need the
     * BLUETOOTH_CONNECT permission ([BLUETOOTH_PERMISSION]); without it they show as headphones.
     */
    enum class Form { Phone, Headphones, Earbuds, Speaker, Car, HearingAid, Tv, Unknown }

    data class Output(val id: Int, val name: String, val kind: Kind, val form: Form = Form.Unknown)

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

    /**
     * The runtime permission that reads a Bluetooth device's class (car, speaker, headphones), null
     * below Android 12, where the install-time BLUETOOTH permission covers it.
     */
    val BLUETOOTH_PERMISSION: String? = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) Manifest.permission.BLUETOOTH_CONNECT else null

    private var manager: AudioManager? = null
    private var appContext: Context? = null
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
        appContext = context.applicationContext
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

    /** Reads the outputs again; after BLUETOOTH_CONNECT is granted, so Bluetooth ones get their form. */
    fun refresh() {
        val am = manager ?: return
        devices.clear()
        val list = am.getDevices(AudioManager.GET_DEVICES_OUTPUTS).mapNotNull { d ->
            val kind = OutputForms.kindOf(d.type) ?: return@mapNotNull null
            devices[d.id] = d
            val name = if (kind == Kind.Phone) "" else d.productName?.toString().orEmpty()
            // Dual-mode devices list as LE and classic: either row gets the class, whichever is kept below.
            val deviceClass = if (kind == Kind.Bluetooth) bluetoothClass(d.address) else null
            Output(d.id, name, kind, OutputForms.formOf(d.type, name, deviceClass))
        }
            // One row per physical device: Bluetooth shows up as A2DP and SCO, keep the first.
            .distinctBy { if (it.kind == Kind.Phone) "phone" else "${it.kind}:${it.name}" }
        val preferred = state.value.preferred?.takeIf { id -> list.any { it.id == id } }
        // The picked output went away (earphones unplugged): back to the system's choice.
        if (preferred == null && state.value.preferred != null) apply?.invoke(null)
        state.value = State(list, preferred)
    }

    /** Whether Hocket may read Bluetooth devices' classes: granted, or not a runtime permission here. */
    fun canReadBluetoothClass(context: Context): Boolean =
        BLUETOOTH_PERMISSION == null || context.checkSelfPermission(BLUETOOTH_PERMISSION) == PackageManager.PERMISSION_GRANTED

    /**
     * The Class of Device of the Bluetooth device at [address], null without the permission. Without
     * BLUETOOTH_CONNECT, Android 14+ also anonymises the address ("XX:XX:XX:XX:12:34"), which
     * getRemoteDevice rejects, so the permission check only saves a doomed lookup there.
     */
    @SuppressLint("MissingPermission") // canReadBluetoothClass checks it.
    private fun bluetoothClass(address: String?): Int? {
        val context = appContext ?: return null
        if (address.isNullOrBlank() || !canReadBluetoothClass(context)) return null
        val adapter = context.getSystemService(BluetoothManager::class.java)?.adapter ?: return null
        return runCatching { adapter.getRemoteDevice(address).bluetoothClass?.deviceClass }.getOrNull()
    }

    /** How the system tends to route media: a headset over the speaker, the speaker last. */
    private fun priority(kind: Kind): Int = when (kind) {
        Kind.Bluetooth -> 5
        Kind.Usb -> 4
        Kind.Wired -> 3
        Kind.Hdmi -> 2
        Kind.Dock -> 1
        Kind.Phone -> 0
    }
}
