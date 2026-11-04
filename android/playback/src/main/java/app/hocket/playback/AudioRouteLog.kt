package app.hocket.playback

import android.content.Context
import android.media.AudioDeviceCallback
import android.media.AudioDeviceInfo
import android.media.AudioManager
import android.os.Handler
import android.os.Looper

/**
 * Logs output devices coming and going ([PlaybackLog]) while the playback service runs: a headset or
 * car connecting often sends a play key of its own, and one leaving makes the player pause (becoming
 * noisy), so these lines explain playback that starts or stops with nobody touching the app. The
 * devices present at [start] are logged once as a list; after that, each change.
 */
internal class AudioRouteLog(private val context: Context) {
    private var manager: AudioManager? = null
    private var started = false

    private val callback = object : AudioDeviceCallback() {
        override fun onAudioDevicesAdded(addedDevices: Array<out AudioDeviceInfo>) = log("connected", addedDevices)
        override fun onAudioDevicesRemoved(removedDevices: Array<out AudioDeviceInfo>) = log("disconnected", removedDevices)
    }

    private fun log(what: String, devices: Array<out AudioDeviceInfo>) {
        if (started) devices.filter { it.isSink }.forEach { PlaybackLog.i("audio output $what: ${PlaybackLog.device(it)}") }
    }

    fun start() {
        val am = context.getSystemService(AudioManager::class.java) ?: return
        manager = am
        val present = am.getDevices(AudioManager.GET_DEVICES_OUTPUTS).joinToString { PlaybackLog.device(it) }
        PlaybackLog.i("audio outputs: ${present.ifEmpty { "none" }}")
        // Registering replays every present device as "added"; [started] flips after that replay,
        // which the callback receives on this (main) looper before anything later.
        am.registerAudioDeviceCallback(callback, Handler(Looper.getMainLooper()))
        Handler(Looper.getMainLooper()).post { started = true }
    }

    fun stop() {
        started = false
        manager?.unregisterAudioDeviceCallback(callback)
        manager = null
    }
}
