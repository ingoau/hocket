package app.hocket.playback

import android.net.Uri
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.Player
import androidx.media3.common.SimpleBasePlayer
import androidx.media3.common.util.Util
import app.hocket.core.Commands
import app.hocket.core.api.Command
import app.hocket.core.api.MediaSessionAction
import app.hocket.core.api.MediaSessionState
import app.hocket.core.api.RepeatMode
import app.hocket.core.client.PositionClock
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import android.os.Looper

/**
 * The `MediaSessionAdapter` seam on Android: a Media3 [Player] whose state is exactly the core's
 * [MediaSessionState] and whose controls dispatch [Command.MediaSessionCommand]s.
 *
 * It is deliberately not the ExoPlayer instance. The core owns transport (which may be on another
 * device), so the OS notification, lockscreen, Bluetooth and headset controls must reflect the
 * session, not the local decoder. Position is extrapolated from the stamp the same way the UI does.
 */
class CoreSessionPlayer(
    looper: Looper,
    private val dispatch: (Command) -> Unit,
    private val now: () -> Double = { System.currentTimeMillis().toDouble() },
) : SimpleBasePlayer(looper) {
    private var current: MediaSessionState = MediaSessionState(null, false, app.hocket.core.api.PositionStamp(0u, 0.0, 1.0, false), false, RepeatMode.Off, 1.0, emptyList(), true)
    private var clockOffsetMs = 0.0

    /** Apply a new state from the core and notify controllers. Main thread. */
    fun apply(state: MediaSessionState, clockOffsetMs: Double = this.clockOffsetMs) {
        current = state
        this.clockOffsetMs = clockOffsetMs
        invalidateState()
    }

    val state: MediaSessionState get() = current

    override fun getState(): State {
        val s = current
        val meta = s.metadata
        val commands = Player.Commands.Builder().apply {
            add(Player.COMMAND_PLAY_PAUSE); add(Player.COMMAND_STOP); add(Player.COMMAND_PREPARE)
            add(Player.COMMAND_GET_CURRENT_MEDIA_ITEM); add(Player.COMMAND_GET_METADATA); add(Player.COMMAND_GET_TIMELINE)
            add(Player.COMMAND_GET_VOLUME); add(Player.COMMAND_SET_VOLUME); add(Player.COMMAND_GET_AUDIO_ATTRIBUTES)
            add(Player.COMMAND_SET_REPEAT_MODE); add(Player.COMMAND_SET_SHUFFLE_MODE)
            if (MediaSessionAction.Next in s.actions) { add(Player.COMMAND_SEEK_TO_NEXT); add(Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM) }
            if (MediaSessionAction.Previous in s.actions) { add(Player.COMMAND_SEEK_TO_PREVIOUS); add(Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM) }
            if (MediaSessionAction.Seek in s.actions) { add(Player.COMMAND_SEEK_IN_CURRENT_MEDIA_ITEM); add(Player.COMMAND_SEEK_BACK); add(Player.COMMAND_SEEK_FORWARD); add(Player.COMMAND_SEEK_TO_MEDIA_ITEM) }
        }.build()
        val builder = State.Builder()
            .setAvailableCommands(commands)
            .setPlayWhenReady(s.isPlaying, Player.PLAY_WHEN_READY_CHANGE_REASON_REMOTE)
            .setPlaybackState(if (meta == null) Player.STATE_IDLE else Player.STATE_READY)
            .setRepeatMode(when (s.repeat) { RepeatMode.Off -> Player.REPEAT_MODE_OFF; RepeatMode.All -> Player.REPEAT_MODE_ALL; RepeatMode.One -> Player.REPEAT_MODE_ONE })
            .setShuffleModeEnabled(s.shuffle)
            .setVolume(s.volume.toFloat())
            .setSeekBackIncrementMs(10_000)
            .setSeekForwardIncrementMs(30_000)
        if (meta != null) {
            val mm = MediaMetadata.Builder()
                .setTitle(meta.title)
                .setArtist(meta.artist)
                .setAlbumTitle(meta.album)
                .setDurationMs(meta.durationMs.toLong())
                .setIsPlayable(true)
                .setIsBrowsable(false)
                .setMediaType(MediaMetadata.MEDIA_TYPE_MUSIC)
                .setUserRating(androidx.media3.common.HeartRating(meta.loved))
                .apply { meta.artworkPath?.let { setArtworkUri(if (it.startsWith("file:") || it.startsWith("content:")) Uri.parse(it) else Uri.fromFile(java.io.File(it))) } }
                .build()
            val item = MediaItem.Builder().setMediaId(meta.trackId ?: "current").setMediaMetadata(mm).build()
            builder.setPlaylist(listOf(
                MediaItemData.Builder(meta.trackId ?: "current")
                    .setMediaItem(item)
                    .setMediaMetadata(mm)
                    .setDurationUs(Util.msToUs(meta.durationMs.toLong()))
                    .setIsSeekable(MediaSessionAction.Seek in s.actions)
                    .build(),
            ))
            builder.setCurrentMediaItemIndex(0)
            val stamp = s.position
            val duration = meta.durationMs.toLong()
            builder.setContentPositionMs { PositionClock.extrapolate(stamp, now(), clockOffsetMs, duration) }
            builder.setContentBufferedPositionMs { duration }
        }
        return builder.build()
    }

    override fun handleSetPlayWhenReady(playWhenReady: Boolean): ListenableFuture<*> {
        dispatch(Commands.mediaSessionCommand(if (playWhenReady) MediaSessionAction.Play else MediaSessionAction.Pause))
        return Futures.immediateVoidFuture()
    }

    override fun handlePrepare(): ListenableFuture<*> = Futures.immediateVoidFuture()

    override fun handleStop(): ListenableFuture<*> {
        dispatch(Commands.mediaSessionCommand(MediaSessionAction.Stop))
        return Futures.immediateVoidFuture()
    }

    override fun handleRelease(): ListenableFuture<*> = Futures.immediateVoidFuture()

    override fun handleSetRepeatMode(repeatMode: Int): ListenableFuture<*> {
        // The core cycles its own repeat mode; a controller asking for a specific mode is honoured by
        // cycling until it matches (at most two steps).
        val target = when (repeatMode) { Player.REPEAT_MODE_ALL -> RepeatMode.All; Player.REPEAT_MODE_ONE -> RepeatMode.One; else -> RepeatMode.Off }
        var mode = current.repeat
        var steps = 0
        while (mode != target && steps < 3) {
            dispatch(Commands.mediaSessionCommand(MediaSessionAction.Repeat))
            mode = when (mode) { RepeatMode.Off -> RepeatMode.All; RepeatMode.All -> RepeatMode.One; RepeatMode.One -> RepeatMode.Off }
            steps++
        }
        return Futures.immediateVoidFuture()
    }

    override fun handleSetShuffleModeEnabled(shuffleModeEnabled: Boolean): ListenableFuture<*> {
        if (shuffleModeEnabled != current.shuffle) dispatch(Commands.mediaSessionCommand(MediaSessionAction.Shuffle))
        return Futures.immediateVoidFuture()
    }

    override fun handleSetVolume(volume: Float, volumeOperationType: Int): ListenableFuture<*> {
        dispatch(Commands.setVolume(volume.toDouble()))
        return Futures.immediateVoidFuture()
    }

    override fun handleSeek(mediaItemIndex: Int, positionMs: Long, seekCommand: Int): ListenableFuture<*> {
        when (seekCommand) {
            Player.COMMAND_SEEK_TO_NEXT, Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM -> dispatch(Commands.mediaSessionCommand(MediaSessionAction.Next))
            Player.COMMAND_SEEK_TO_PREVIOUS, Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM -> dispatch(Commands.mediaSessionCommand(MediaSessionAction.Previous))
            else -> dispatch(Commands.mediaSessionCommand(MediaSessionAction.Seek, positionMs.coerceAtLeast(0).toDouble()))
        }
        return Futures.immediateVoidFuture()
    }
}
