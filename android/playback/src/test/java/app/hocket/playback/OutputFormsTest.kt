package app.hocket.playback

import android.bluetooth.BluetoothClass.Device
import android.media.AudioDeviceInfo
import app.hocket.playback.AudioOutputs.Form
import app.hocket.playback.AudioOutputs.Kind
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** What an output shows as: by its type, its Bluetooth class, and (for headphones) its name. */
class OutputFormsTest {
    @Test
    fun typesMapToLinks() {
        assertEquals(Kind.Phone, OutputForms.kindOf(AudioDeviceInfo.TYPE_BUILTIN_SPEAKER))
        assertEquals(Kind.Wired, OutputForms.kindOf(AudioDeviceInfo.TYPE_LINE_ANALOG))
        assertEquals(Kind.Usb, OutputForms.kindOf(AudioDeviceInfo.TYPE_USB_DEVICE))
        assertEquals(Kind.Bluetooth, OutputForms.kindOf(AudioDeviceInfo.TYPE_BLE_SPEAKER))
        assertEquals(Kind.Bluetooth, OutputForms.kindOf(AudioDeviceInfo.TYPE_HEARING_AID))
        assertEquals(Kind.Hdmi, OutputForms.kindOf(AudioDeviceInfo.TYPE_HDMI))
        assertEquals(Kind.Dock, OutputForms.kindOf(AudioDeviceInfo.TYPE_DOCK))
        // The earpiece and phone-call routes aren't music outputs.
        assertNull(OutputForms.kindOf(AudioDeviceInfo.TYPE_BUILTIN_EARPIECE))
        assertNull(OutputForms.kindOf(AudioDeviceInfo.TYPE_BLUETOOTH_SCO))
    }

    @Test
    fun typesThatSayWhatTheyAre() {
        assertEquals(Form.Phone, OutputForms.formOf(AudioDeviceInfo.TYPE_BUILTIN_SPEAKER, ""))
        assertEquals(Form.Speaker, OutputForms.formOf(AudioDeviceInfo.TYPE_BLE_SPEAKER, "Kitchen"))
        assertEquals(Form.Headphones, OutputForms.formOf(AudioDeviceInfo.TYPE_BLE_HEADSET, ""))
        assertEquals(Form.Earbuds, OutputForms.formOf(AudioDeviceInfo.TYPE_BLE_HEADSET, "Galaxy Buds3 Pro"))
        assertEquals(Form.Speaker, OutputForms.formOf(AudioDeviceInfo.TYPE_BLE_HEADSET, "Party", android.bluetooth.BluetoothClass.Device.AUDIO_VIDEO_LOUDSPEAKER))
        assertEquals(Form.HearingAid, OutputForms.formOf(AudioDeviceInfo.TYPE_HEARING_AID, "Left"))
        assertEquals(Form.Headphones, OutputForms.formOf(AudioDeviceInfo.TYPE_WIRED_HEADPHONES, ""))
        assertEquals(Form.Headphones, OutputForms.formOf(AudioDeviceInfo.TYPE_WIRED_HEADSET, ""))
        assertEquals(Form.Speaker, OutputForms.formOf(AudioDeviceInfo.TYPE_LINE_ANALOG, ""))
        assertEquals(Form.Tv, OutputForms.formOf(AudioDeviceInfo.TYPE_HDMI, "LG TV"))
        assertEquals(Form.Unknown, OutputForms.formOf(AudioDeviceInfo.TYPE_USB_DEVICE, "USB-C to 3.5mm"))
        assertEquals(Form.Earbuds, OutputForms.formOf(AudioDeviceInfo.TYPE_USB_HEADSET, "Pixel USB-C earbuds"))
    }

    @Test
    fun bluetoothClassDecidesCarsAndSpeakers() {
        val a2dp = AudioDeviceInfo.TYPE_BLUETOOTH_A2DP
        assertEquals(Form.Car, OutputForms.formOf(a2dp, "Toyota", Device.AUDIO_VIDEO_CAR_AUDIO))
        assertEquals(Form.Car, OutputForms.formOf(a2dp, "MY CAR", Device.AUDIO_VIDEO_HANDSFREE))
        assertEquals(Form.Speaker, OutputForms.formOf(a2dp, "JBL Flip 6", Device.AUDIO_VIDEO_LOUDSPEAKER))
        assertEquals(Form.Speaker, OutputForms.formOf(a2dp, "Boom", Device.AUDIO_VIDEO_PORTABLE_AUDIO))
        assertEquals(Form.Speaker, OutputForms.formOf(a2dp, "Amp", Device.AUDIO_VIDEO_HIFI_AUDIO))
        // A car named like earbuds is still a car: the name only splits headphones.
        assertEquals(Form.Car, OutputForms.formOf(a2dp, "Buds' Car", Device.AUDIO_VIDEO_CAR_AUDIO))
        // Headphones and headsets: the name decides between earbuds and over-ear.
        assertEquals(Form.Earbuds, OutputForms.formOf(a2dp, "AirPods Pro", Device.AUDIO_VIDEO_HEADPHONES))
        assertEquals(Form.Headphones, OutputForms.formOf(a2dp, "WH-1000XM5", Device.AUDIO_VIDEO_WEARABLE_HEADSET))
    }

    @Test
    fun withoutTheClassBluetoothIsHeadphonesAsBefore() {
        val a2dp = AudioDeviceInfo.TYPE_BLUETOOTH_A2DP
        assertEquals(Form.Headphones, OutputForms.formOf(a2dp, "JBL Flip 6", null))
        assertEquals(Form.Headphones, OutputForms.formOf(a2dp, "Something", Device.COMPUTER_LAPTOP))
        assertEquals(Form.Earbuds, OutputForms.formOf(a2dp, "Pixel Buds Pro", null))
    }

    @Test
    fun namesSplitEarbudsFromOverEar() {
        val earbuds = listOf(
            "AirPods", "AirPods Pro", "Pixel Buds Pro 2", "Galaxy Buds2", "Sony WF-1000XM5", "HUAWEI FreeBuds 5i",
            "Bose QC Ultra Earbuds", "Beats Studio Buds", "Nothing Ear (2)", "OnePlus Ear", "Powerbeats Pro", "TWS-X9", "Soundcore in-ear", "QCY T13",
        )
        for (n in earbuds) assertEquals(n, Form.Earbuds, OutputForms.headphones(n))
        val overEar = listOf(
            "AirPods Max", "WH-1000XM4", "Bose QC45", "Bose QuietComfort 35", "Sennheiser Momentum 4", "Hearts", "Earth Speaker", "",
            "Jabra headphones", "Bose QC Ultra", "Marshall Major Over Ear", "Koss On Ear",
        )
        for (n in overEar) assertEquals(n, Form.Headphones, OutputForms.headphones(n))
    }
}
