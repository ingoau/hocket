package app.hocket.playback

import android.content.Context
import android.content.Intent
import android.media.audiofx.AudioEffect
import androidx.media3.common.C

/**
 * Tells equalizer and effect apps (the system's, Wavelet, a phone maker's sound settings) about the
 * player's audio session, so they can attach their effects to it: the standard
 * `ACTION_OPEN_AUDIO_EFFECT_CONTROL_SESSION` when playback first starts on a session id, and
 * `ACTION_CLOSE_AUDIO_EFFECT_CONTROL_SESSION` when that session id goes (a new one, or the player is
 * released). Main thread.
 */
class AudioEffectSession(private val context: Context) {
    companion object {
        /** The session announced last in this process (for the settings' equalizer link); 0 (the output mix) when none. */
        @Volatile
        var current: Int = 0
            private set

        /**
         * The system's (or an installed app's) equalizer panel for the playing session, or for the
         * output mix when nothing has played yet. Null when nothing on the device offers one.
         */
        fun controlPanelIntent(context: Context): Intent? =
            Intent(AudioEffect.ACTION_DISPLAY_AUDIO_EFFECT_CONTROL_PANEL)
                .putExtra(AudioEffect.EXTRA_AUDIO_SESSION, current)
                .putExtra(AudioEffect.EXTRA_PACKAGE_NAME, context.packageName)
                .putExtra(AudioEffect.EXTRA_CONTENT_TYPE, AudioEffect.CONTENT_TYPE_MUSIC)
                .takeIf { it.resolveActivity(context.packageManager) != null }
    }

    /** The session id announced as open, or [C.AUDIO_SESSION_ID_UNSET]. */
    var openSessionId: Int = C.AUDIO_SESSION_ID_UNSET
        private set

    /** Playback started on [sessionId]: announce it, closing a different one first. */
    fun open(sessionId: Int) {
        if (sessionId == C.AUDIO_SESSION_ID_UNSET || sessionId == openSessionId) return
        close()
        broadcast(AudioEffect.ACTION_OPEN_AUDIO_EFFECT_CONTROL_SESSION, sessionId)
        openSessionId = sessionId
        current = sessionId
    }

    /** The player's session id changed: an announced one moves to [sessionId]. */
    fun onSessionIdChanged(sessionId: Int) {
        if (openSessionId != C.AUDIO_SESSION_ID_UNSET && sessionId != openSessionId) open(sessionId)
    }

    fun close() {
        if (openSessionId == C.AUDIO_SESSION_ID_UNSET) return
        broadcast(AudioEffect.ACTION_CLOSE_AUDIO_EFFECT_CONTROL_SESSION, openSessionId)
        openSessionId = C.AUDIO_SESSION_ID_UNSET
        current = 0
    }

    private fun broadcast(action: String, sessionId: Int) {
        context.sendBroadcast(
            Intent(action)
                .putExtra(AudioEffect.EXTRA_AUDIO_SESSION, sessionId)
                .putExtra(AudioEffect.EXTRA_PACKAGE_NAME, context.packageName)
                .putExtra(AudioEffect.EXTRA_CONTENT_TYPE, AudioEffect.CONTENT_TYPE_MUSIC),
        )
    }
}
