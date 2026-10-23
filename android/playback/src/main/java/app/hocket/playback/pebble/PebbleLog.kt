package app.hocket.playback.pebble

import app.hocket.core.DevLog

/**
 * The watch link's log: into the core's capture ([DevLog], `kotlin/Pebble`), so the developer
 * diagnostics page has it next to the core's lines for the last hour. Not to logcat: the bundle
 * reads that too, and would get every line twice and the debug ones outside developer mode. Debug
 * lines (every message each way) are kept only in developer mode.
 */
internal object PebbleLog {
    private const val TAG = "kotlin/Pebble"

    fun d(message: String) = DevLog.append("debug", TAG, message)
    fun i(message: String) = DevLog.append("info", TAG, message)
    fun w(message: String) = DevLog.append("warn", TAG, message)

    /** A list path without what the user typed (`search:<text>` becomes `search:…`). */
    fun path(path: String): String = if (path.startsWith("search:")) "search:…" else path

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
