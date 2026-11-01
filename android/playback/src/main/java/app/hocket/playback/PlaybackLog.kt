package app.hocket.playback

import android.content.Intent
import android.media.AudioDeviceInfo
import android.view.KeyEvent
import androidx.media3.common.Player
import app.hocket.core.DevLog

/**
 * The playback service's log: into the core's capture ([DevLog], `kotlin/Playback`), so the
 * diagnostics bundle has the service's life, who asked it to play or pause, and why the platform
 * player stopped, next to the core's lines for the last hour. Logcat on a phone keeps only minutes
 * of a busy device, so lines left there are usually gone by the time a bundle is taken. Not also to
 * logcat: the bundle reads that too and would get every line twice. Debug lines are kept only in
 * developer mode.
 */
internal object PlaybackLog {
    private const val TAG = "kotlin/Playback"

    fun d(message: String) = DevLog.append("debug", TAG, message)
    fun i(message: String) = DevLog.append("info", TAG, message)
    fun w(message: String) = DevLog.append("warn", TAG, message)

    /** A start intent: its action, and the key for a media button (`media button PLAY down`). */
    fun intent(intent: Intent?): String {
        if (intent == null) return "no intent (restarted by the system)"
        if (intent.action == Intent.ACTION_MEDIA_BUTTON) {
            @Suppress("DEPRECATION")
            val key = intent.getParcelableExtra<KeyEvent>(Intent.EXTRA_KEY_EVENT)
            return "media button ${key?.let(::key) ?: "(no key)"}"
        }
        return intent.action ?: "no action"
    }

    /** `PLAY down` / `MEDIA_PAUSE up` (repeat count when held). */
    fun key(event: KeyEvent): String {
        val name = KeyEvent.keyCodeToString(event.keyCode).removePrefix("KEYCODE_")
        val action = when (event.action) {
            KeyEvent.ACTION_DOWN -> "down"
            KeyEvent.ACTION_UP -> "up"
            else -> "action ${event.action}"
        }
        return if (event.repeatCount > 0) "$name $action (repeat ${event.repeatCount})" else "$name $action"
    }

    /** Why `playWhenReady` changed ([Player.PlayWhenReadyChangeReason]). */
    fun playWhenReadyReason(reason: Int): String = when (reason) {
        Player.PLAY_WHEN_READY_CHANGE_REASON_USER_REQUEST -> "requested"
        Player.PLAY_WHEN_READY_CHANGE_REASON_AUDIO_FOCUS_LOSS -> "audio focus lost"
        Player.PLAY_WHEN_READY_CHANGE_REASON_AUDIO_BECOMING_NOISY -> "audio becoming noisy (output left headphones/Bluetooth)"
        Player.PLAY_WHEN_READY_CHANGE_REASON_REMOTE -> "remote"
        Player.PLAY_WHEN_READY_CHANGE_REASON_END_OF_MEDIA_ITEM -> "end of item"
        Player.PLAY_WHEN_READY_CHANGE_REASON_SUPPRESSED_TOO_LONG -> "suppressed too long"
        else -> "reason $reason"
    }

    /** Why playback is suppressed ([Player.PlaybackSuppressionReason]). */
    fun suppressionReason(reason: Int): String = when (reason) {
        Player.PLAYBACK_SUPPRESSION_REASON_NONE -> "none"
        Player.PLAYBACK_SUPPRESSION_REASON_TRANSIENT_AUDIO_FOCUS_LOSS -> "transient audio focus loss"
        Player.PLAYBACK_SUPPRESSION_REASON_UNSUITABLE_AUDIO_OUTPUT -> "unsuitable audio output"
        else -> "reason $reason"
    }

    /** A player command's name ([Player.Command]), for the ones a controller sends. */
    fun command(command: Int): String = when (command) {
        Player.COMMAND_PLAY_PAUSE -> "play/pause"
        Player.COMMAND_PREPARE -> "prepare"
        Player.COMMAND_STOP -> "stop"
        Player.COMMAND_SEEK_TO_DEFAULT_POSITION -> "seek to default"
        Player.COMMAND_SEEK_IN_CURRENT_MEDIA_ITEM -> "seek"
        Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM -> "previous item"
        Player.COMMAND_SEEK_TO_PREVIOUS -> "previous"
        Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM -> "next item"
        Player.COMMAND_SEEK_TO_NEXT -> "next"
        Player.COMMAND_SEEK_TO_MEDIA_ITEM -> "seek to item"
        Player.COMMAND_SEEK_BACK -> "seek back"
        Player.COMMAND_SEEK_FORWARD -> "seek forward"
        Player.COMMAND_SET_SPEED_AND_PITCH -> "set speed"
        Player.COMMAND_SET_SHUFFLE_MODE -> "set shuffle"
        Player.COMMAND_SET_REPEAT_MODE -> "set repeat"
        Player.COMMAND_SET_MEDIA_ITEM -> "set item"
        Player.COMMAND_CHANGE_MEDIA_ITEMS -> "change items"
        Player.COMMAND_SET_VOLUME -> "set volume"
        Player.COMMAND_SET_DEVICE_VOLUME_WITH_FLAGS -> "set device volume"
        Player.COMMAND_ADJUST_DEVICE_VOLUME_WITH_FLAGS -> "adjust device volume"
        else -> "command $command"
    }

    /** An output device: `Bluetooth A2DP "Pixel Buds Pro"`. */
    fun device(info: AudioDeviceInfo): String = "${deviceType(info.type)} \"${info.productName}\""

    private fun deviceType(type: Int): String = when (type) {
        AudioDeviceInfo.TYPE_BUILTIN_SPEAKER -> "speaker"
        AudioDeviceInfo.TYPE_BUILTIN_EARPIECE -> "earpiece"
        AudioDeviceInfo.TYPE_WIRED_HEADSET -> "wired headset"
        AudioDeviceInfo.TYPE_WIRED_HEADPHONES -> "wired headphones"
        AudioDeviceInfo.TYPE_BLUETOOTH_A2DP -> "Bluetooth A2DP"
        AudioDeviceInfo.TYPE_BLUETOOTH_SCO -> "Bluetooth SCO"
        AudioDeviceInfo.TYPE_BLE_HEADSET -> "BLE headset"
        AudioDeviceInfo.TYPE_BLE_SPEAKER -> "BLE speaker"
        AudioDeviceInfo.TYPE_BLE_BROADCAST -> "BLE broadcast"
        AudioDeviceInfo.TYPE_USB_HEADSET -> "USB headset"
        AudioDeviceInfo.TYPE_USB_DEVICE -> "USB device"
        AudioDeviceInfo.TYPE_HDMI -> "HDMI"
        AudioDeviceInfo.TYPE_HEARING_AID -> "hearing aid"
        AudioDeviceInfo.TYPE_REMOTE_SUBMIX -> "remote submix"
        AudioDeviceInfo.TYPE_TELEPHONY -> "telephony"
        else -> "type $type"
    }
}
