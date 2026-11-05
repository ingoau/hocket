package app.hocket.playback

import android.annotation.SuppressLint
import android.bluetooth.BluetoothClass
import android.media.AudioDeviceInfo
import app.hocket.playback.AudioOutputs.Form
import app.hocket.playback.AudioOutputs.Kind

/**
 * What an audio output is, from what Android tells us about it: the link ([Kind]) from the
 * `AudioDeviceInfo` type, and the thing itself ([Form]) from the type, the Bluetooth Class of
 * Device when we can read it, and, last, the name (only to tell earbuds from over-ear headphones).
 * Pure, so it is unit-tested without a device.
 */
internal object OutputForms {
    /** The link of an output type Hocket lists, or null for one it doesn't (earpiece, telephony, buses). */
    // The types are compile-time constants; a device only reports the ones its API level has.
    @SuppressLint("InlinedApi")
    fun kindOf(type: Int): Kind? = when (type) {
        AudioDeviceInfo.TYPE_BUILTIN_SPEAKER -> Kind.Phone
        AudioDeviceInfo.TYPE_WIRED_HEADPHONES, AudioDeviceInfo.TYPE_WIRED_HEADSET, AudioDeviceInfo.TYPE_LINE_ANALOG -> Kind.Wired
        AudioDeviceInfo.TYPE_USB_HEADSET, AudioDeviceInfo.TYPE_USB_DEVICE -> Kind.Usb
        AudioDeviceInfo.TYPE_BLUETOOTH_A2DP, AudioDeviceInfo.TYPE_BLE_HEADSET, AudioDeviceInfo.TYPE_BLE_SPEAKER,
        AudioDeviceInfo.TYPE_HEARING_AID -> Kind.Bluetooth
        AudioDeviceInfo.TYPE_HDMI -> Kind.Hdmi
        AudioDeviceInfo.TYPE_DOCK, AudioDeviceInfo.TYPE_DOCK_ANALOG -> Kind.Dock
        else -> null
    }

    /**
     * What an output of [type] named [name] is. [deviceClass] is the Bluetooth device's
     * `BluetoothClass.getDeviceClass()`, null when unknown (no BLUETOOTH_CONNECT, not Bluetooth).
     */
    @SuppressLint("InlinedApi")
    fun formOf(type: Int, name: String, deviceClass: Int? = null): Form = when (type) {
        AudioDeviceInfo.TYPE_BUILTIN_SPEAKER -> Form.Phone
        AudioDeviceInfo.TYPE_WIRED_HEADPHONES, AudioDeviceInfo.TYPE_WIRED_HEADSET, AudioDeviceInfo.TYPE_USB_HEADSET -> headphones(name)
        // A line out feeds an amplifier or powered speakers, as does a dock.
        AudioDeviceInfo.TYPE_LINE_ANALOG, AudioDeviceInfo.TYPE_DOCK, AudioDeviceInfo.TYPE_DOCK_ANALOG,
        AudioDeviceInfo.TYPE_BLE_SPEAKER -> Form.Speaker
        AudioDeviceInfo.TYPE_HEARING_AID -> Form.HearingAid
        AudioDeviceInfo.TYPE_HDMI -> Form.Tv
        AudioDeviceInfo.TYPE_BLUETOOTH_A2DP, AudioDeviceInfo.TYPE_BLE_HEADSET -> bluetoothForm(deviceClass) ?: headphones(name)
        else -> Form.Unknown
    }

    /** A Bluetooth device's form by its Class of Device, null when the class doesn't say. */
    fun bluetoothForm(deviceClass: Int?): Form? = when (deviceClass) {
        BluetoothClass.Device.AUDIO_VIDEO_CAR_AUDIO, BluetoothClass.Device.AUDIO_VIDEO_HANDSFREE -> Form.Car
        BluetoothClass.Device.AUDIO_VIDEO_LOUDSPEAKER, BluetoothClass.Device.AUDIO_VIDEO_PORTABLE_AUDIO,
        BluetoothClass.Device.AUDIO_VIDEO_HIFI_AUDIO -> Form.Speaker
        BluetoothClass.Device.AUDIO_VIDEO_VIDEO_DISPLAY_AND_LOUDSPEAKER -> Form.Tv
        // HEADPHONES and WEARABLE_HEADSET: earbuds say either as often as over-ear ones, so the name decides.
        else -> null
    }

    /** Earbuds when the name says so, else headphones (over-ear, or we can't tell). */
    fun headphones(name: String): Form {
        val n = name.lowercase()
        return when {
            // "Buds" first: "QC Ultra Earbuds", "Studio Buds" are earbuds despite the over-ear brand words below.
            "buds" in n -> Form.Earbuds
            OVER_EAR.any { it in n } || QC.containsMatchIn(n) -> Form.Headphones
            EARBUDS.any { it in n } || EAR_WORD.containsMatchIn(n) -> Form.Earbuds
            else -> Form.Headphones
        }
    }

    private val OVER_EAR = listOf("airpods max", "wh-", "quietcomfort", "headphone", "over-ear", "over ear", "on-ear", "on ear")
    /** Bose "QC45", "QC Ultra", but not the earbud brand QCY. */
    private val QC = Regex("""\bqc(\d|\b)""")
    private val EARBUDS = listOf("airpods", "wf-", "earphone", "in-ear", "tws", "powerbeats", "beats fit", "qcy")
    /** "Nothing Ear (2)", "OnePlus Ear": "ear" as a word, not inside "heart" or "Earth". */
    private val EAR_WORD = Regex("""\bear\b""")
}
