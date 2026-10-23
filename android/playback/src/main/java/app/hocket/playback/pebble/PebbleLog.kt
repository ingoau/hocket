package app.hocket.playback.pebble

import android.util.Log
import app.hocket.core.DevLog

/**
 * The watch link's log: into the core's capture ([DevLog], `kotlin/Pebble`), so the developer
 * diagnostics page has it next to the core's lines for the last hour, and to logcat for a live
 * `adb logcat`. Debug lines (every message each way) are kept only in developer mode.
 */
internal object PebbleLog {
    private const val TAG = "Pebble"
    private const val DEV_TAG = "kotlin/Pebble"

    fun d(message: String) = log(Log.DEBUG, "debug", message)
    fun i(message: String) = log(Log.INFO, "info", message)
    fun w(message: String) = log(Log.WARN, "warn", message)

    private fun log(priority: Int, level: String, message: String) {
        runCatching { Log.println(priority, TAG, message) }
        DevLog.append(level, DEV_TAG, message)
    }

    /** A message type's name ([PebbleProtocol.Type]). */
    fun type(type: Int): String = when (type) {
        PebbleProtocol.Type.NOW -> "now"
        PebbleProtocol.Type.LIST -> "list"
        PebbleProtocol.Type.ART -> "art"
        PebbleProtocol.Type.SETTINGS -> "settings"
        PebbleProtocol.Type.HELLO_REPLY -> "hello-reply"
        PebbleProtocol.Type.NOTICE -> "notice"
        PebbleProtocol.Type.HELLO -> "hello"
        PebbleProtocol.Type.CMD -> "cmd"
        PebbleProtocol.Type.LIST_REQ -> "list-req"
        PebbleProtocol.Type.ART_REQ -> "art-req"
        else -> "type $type"
    }

    /** A command's name ([PebbleProtocol.Cmd]). */
    fun cmd(cmd: Int): String = when (cmd) {
        PebbleProtocol.Cmd.PLAY_PAUSE -> "play-pause"
        PebbleProtocol.Cmd.PLAY -> "play"
        PebbleProtocol.Cmd.PAUSE -> "pause"
        PebbleProtocol.Cmd.NEXT -> "next"
        PebbleProtocol.Cmd.PREVIOUS -> "previous"
        PebbleProtocol.Cmd.VOLUME_UP -> "volume-up"
        PebbleProtocol.Cmd.VOLUME_DOWN -> "volume-down"
        PebbleProtocol.Cmd.TOGGLE_LOVE -> "toggle-love"
        PebbleProtocol.Cmd.SET_RATING -> "set-rating"
        PebbleProtocol.Cmd.TOGGLE_SHUFFLE -> "toggle-shuffle"
        PebbleProtocol.Cmd.CYCLE_REPEAT -> "cycle-repeat"
        PebbleProtocol.Cmd.RESUME -> "resume"
        PebbleProtocol.Cmd.ACTIVATE -> "activate"
        PebbleProtocol.Cmd.PLAY_NEXT -> "play-next"
        PebbleProtocol.Cmd.PLAY_LATER -> "play-later"
        PebbleProtocol.Cmd.SEEK -> "seek"
        else -> "cmd $cmd"
    }
}
