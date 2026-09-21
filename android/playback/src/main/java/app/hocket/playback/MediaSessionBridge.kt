package app.hocket.playback

import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.os.Bundle
import androidx.media3.session.CommandButton
import androidx.media3.session.MediaSession
import androidx.media3.session.SessionCommand
import androidx.media3.session.SessionResult
import app.hocket.core.Commands
import app.hocket.core.api.Command
import app.hocket.core.api.MediaSessionAction
import app.hocket.core.api.MediaSessionState
import app.hocket.core.api.RepeatMode
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture

/**
 * Builds the Media3 [MediaSession] over a [CoreSessionPlayer] and keeps its custom button layout in
 * step with the core's customised action list (`MediaSessionState.actions`): love, shuffle, repeat
 * and rate as media button preferences, so they appear on the notification and on Wear/Auto surfaces
 * that honour them. Transport buttons come from the player's available commands.
 */
class MediaSessionBridge(
    context: Context,
    val player: CoreSessionPlayer,
    private val dispatch: (Command) -> Unit,
    launchIntent: Intent?,
) {
    companion object {
        const val CMD_LOVE = "app.hocket.LOVE"
        const val CMD_SHUFFLE = "app.hocket.SHUFFLE"
        const val CMD_REPEAT = "app.hocket.REPEAT"
        const val CMD_RATE = "app.hocket.RATE"
        const val EXTRA_RATING = "rating"
    }

    private val customCommands = listOf(CMD_LOVE, CMD_SHUFFLE, CMD_REPEAT, CMD_RATE).map { SessionCommand(it, Bundle.EMPTY) }

    val session: MediaSession = MediaSession.Builder(context, player)
        .setId("hocket")
        .setCallback(object : MediaSession.Callback {
            override fun onConnect(session: MediaSession, controller: MediaSession.ControllerInfo): MediaSession.ConnectionResult {
                val commands = MediaSession.ConnectionResult.DEFAULT_SESSION_COMMANDS.buildUpon().apply { customCommands.forEach { add(it) } }.build()
                return MediaSession.ConnectionResult.AcceptedResultBuilder(session)
                    .setAvailableSessionCommands(commands)
                    .setMediaButtonPreferences(buttons(player.state))
                    .build()
            }

            override fun onCustomCommand(session: MediaSession, controller: MediaSession.ControllerInfo, customCommand: SessionCommand, args: Bundle): ListenableFuture<SessionResult> {
                when (customCommand.customAction) {
                    CMD_LOVE -> dispatch(Commands.mediaSessionCommand(MediaSessionAction.Love))
                    CMD_SHUFFLE -> dispatch(Commands.mediaSessionCommand(MediaSessionAction.Shuffle))
                    CMD_REPEAT -> dispatch(Commands.mediaSessionCommand(MediaSessionAction.Repeat))
                    CMD_RATE -> dispatch(Commands.mediaSessionCommand(MediaSessionAction.Rate, args.getInt(EXTRA_RATING, 0).toDouble()))
                    else -> return Futures.immediateFuture(SessionResult(SessionResult.RESULT_ERROR_NOT_SUPPORTED))
                }
                return Futures.immediateFuture(SessionResult(SessionResult.RESULT_SUCCESS))
            }

            override fun onPlaybackResumption(session: MediaSession, controller: MediaSession.ControllerInfo): ListenableFuture<MediaSession.MediaItemsWithStartPosition> {
                // Never auto-resume from the system (design: resuming is always explicit). Report what we
                // have so the notification can stay; the core decides whether anything plays.
                return Futures.immediateFuture(MediaSession.MediaItemsWithStartPosition(emptyList(), 0, 0L))
            }
        })
        .apply {
            launchIntent?.let {
                setSessionActivity(PendingIntent.getActivity(context, 0, it, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT))
            }
        }
        .build()

    /** Push a new state to the player and refresh the custom layout. Main thread. */
    fun apply(state: MediaSessionState, clockOffsetMs: Double) {
        player.apply(state, clockOffsetMs)
        session.setMediaButtonPreferences(buttons(state))
    }

    private fun buttons(state: MediaSessionState): List<CommandButton> {
        val meta = state.metadata
        return state.actions.mapNotNull { action ->
            when (action) {
                MediaSessionAction.Love -> CommandButton.Builder(if (meta?.loved == true) CommandButton.ICON_HEART_FILLED else CommandButton.ICON_HEART_UNFILLED)
                    .setDisplayName(if (meta?.loved == true) "Unlove" else "Love")
                    .setSessionCommand(SessionCommand(CMD_LOVE, Bundle.EMPTY)).setEnabled(meta != null).build()
                MediaSessionAction.Shuffle -> CommandButton.Builder(if (state.shuffle) CommandButton.ICON_SHUFFLE_ON else CommandButton.ICON_SHUFFLE_OFF)
                    .setDisplayName("Shuffle").setSessionCommand(SessionCommand(CMD_SHUFFLE, Bundle.EMPTY)).build()
                MediaSessionAction.Repeat -> CommandButton.Builder(when (state.repeat) { RepeatMode.Off -> CommandButton.ICON_REPEAT_OFF; RepeatMode.All -> CommandButton.ICON_REPEAT_ALL; RepeatMode.One -> CommandButton.ICON_REPEAT_ONE })
                    .setDisplayName("Repeat").setSessionCommand(SessionCommand(CMD_REPEAT, Bundle.EMPTY)).build()
                MediaSessionAction.Rate -> CommandButton.Builder(if ((meta?.rating ?: 0u) > 0u) CommandButton.ICON_STAR_FILLED else CommandButton.ICON_STAR_UNFILLED)
                    .setDisplayName("Rate").setSessionCommand(SessionCommand(CMD_RATE, Bundle().apply { putInt(EXTRA_RATING, if ((meta?.rating ?: 0u) >= 5u) 0 else 5) })).setEnabled(meta != null).build()
                else -> null // transport actions are player commands
            }
        }
    }

    fun release() {
        session.release()
        player.release()
    }
}
