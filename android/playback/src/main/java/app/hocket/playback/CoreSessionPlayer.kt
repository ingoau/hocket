package app.hocket.playback

import android.net.Uri
import androidx.media3.common.C
import androidx.media3.common.DeviceInfo
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.Player
import androidx.media3.common.SimpleBasePlayer
import androidx.media3.common.util.Util
import app.hocket.core.Commands
import app.hocket.core.api.Command
import app.hocket.core.api.MediaSessionAction
import app.hocket.core.api.MediaSessionState
import app.hocket.core.api.QueueEntry
import app.hocket.core.api.QueueView
import app.hocket.core.api.TrackSummary
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
 *
 * The playlist is the core's queue ([applyQueue]): recent history, the current entry, playing next
 * and upcoming, keyed by queue key (media id `queue/<key>`), so controllers show it as the session
 * queue (Auto's queue view, legacy `MediaSession.setQueue`). Picking an entry is a
 * `JumpToQueueItem`, moving and removing upcoming entries are `MoveQueueItem` / `RemoveQueueItems`,
 * and items a controller sets or adds (library ids from [LibraryBrowser]) go to [MediaRequests].
 * The current entry's metadata is always the session's own; when the queue does not agree with it
 * (the view has not caught up yet) the playlist is that one item.
 *
 * While another device plays ([remote]), the player reports remote playback
 * ([DeviceInfo.PLAYBACK_TYPE_REMOTE], fixed volume) with [ConnectRoutes.SESSION_ID] as its routing
 * controller id, so the system pairs the session with [ConnectRouteProvider]'s routing session and
 * shows that device as the output. The controls keep working: the core forwards them.
 */
class CoreSessionPlayer(
    looper: Looper,
    private val dispatch: (Command) -> Unit,
    private val now: () -> Double = { System.currentTimeMillis().toDouble() },
    private val media: MediaRequests? = null,
    private val artwork: (TrackSummary) -> Uri? = { null },
) : SimpleBasePlayer(looper) {
    /** What the player does with media ids a controller sets or adds ([LibraryBrowser]). */
    interface MediaRequests {
        fun play(mediaIds: List<String>, startIndex: Int)
        fun enqueue(mediaIds: List<String>, next: Boolean)
    }

    companion object {
        /** History entries shown before the current one: enough to go back, small over binder. */
        const val HISTORY_SHOWN = 25

        private val REMOTE_DEVICE: DeviceInfo = DeviceInfo.Builder(DeviceInfo.PLAYBACK_TYPE_REMOTE)
            .setRoutingControllerId(ConnectRoutes.SESSION_ID)
            .build()

        /**
         * The timeline for [queue] around the playing track: the last [HISTORY_SHOWN] history
         * entries, the current one, playing next, upcoming. Empty when the queue's current entry is
         * not [currentTrackId] (or there is none).
         */
        fun timeline(queue: QueueView?, currentTrackId: String?): QueueTimeline {
            val current = queue?.current ?: return QueueTimeline.EMPTY
            if (currentTrackId != null && current.track.id != currentTrackId) return QueueTimeline.EMPTY
            val history = queue.history.takeLast(HISTORY_SHOWN)
            return QueueTimeline(history + current + queue.playingNext + queue.upcoming, history.size)
        }
    }

    /** The queue entries the playlist shows and where the current one is. */
    data class QueueTimeline(val entries: List<QueueEntry>, val currentIndex: Int) {
        /** Index into the core's combined playing-next + upcoming list (`MoveQueueItem.toIndex`). */
        fun upcomingIndex(index: Int): Int = index - currentIndex - 1

        companion object { val EMPTY = QueueTimeline(emptyList(), 0) }
    }

    private var current: MediaSessionState = MediaSessionState(null, false, app.hocket.core.api.PositionStamp(0u, 0.0, 1.0, false), false, RepeatMode.Off, 1.0, emptyList(), true)
    private var clockOffsetMs = 0.0
    private var remote = false
    private var queue: QueueView? = null
    private var timeline = QueueTimeline.EMPTY

    /** Apply a new queue view from the core. Main thread. */
    fun applyQueue(view: QueueView) {
        queue = view
        invalidateState()
    }

    /** Apply a new state from the core and notify controllers. Main thread. */
    fun apply(state: MediaSessionState, clockOffsetMs: Double = this.clockOffsetMs, remote: Boolean = this.remote) {
        current = state
        this.clockOffsetMs = clockOffsetMs
        this.remote = remote
        invalidateState()
    }

    /** Playing on another device (see the class docs). */
    val isRemote: Boolean get() = remote

    val state: MediaSessionState get() = current

    override fun getState(): State {
        val s = current
        val meta = s.metadata
        timeline = if (meta == null) QueueTimeline.EMPTY else timeline(queue, meta.trackId)
        val commands = Player.Commands.Builder().apply {
            add(Player.COMMAND_PLAY_PAUSE); add(Player.COMMAND_STOP); add(Player.COMMAND_PREPARE)
            add(Player.COMMAND_GET_CURRENT_MEDIA_ITEM); add(Player.COMMAND_GET_METADATA); add(Player.COMMAND_GET_TIMELINE)
            add(Player.COMMAND_GET_VOLUME); add(Player.COMMAND_SET_VOLUME); add(Player.COMMAND_GET_AUDIO_ATTRIBUTES)
            add(Player.COMMAND_SET_REPEAT_MODE); add(Player.COMMAND_SET_SHUFFLE_MODE)
            if (MediaSessionAction.Next in s.actions) { add(Player.COMMAND_SEEK_TO_NEXT); add(Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM) }
            if (MediaSessionAction.Previous in s.actions) { add(Player.COMMAND_SEEK_TO_PREVIOUS); add(Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM) }
            if (MediaSessionAction.Seek in s.actions) { add(Player.COMMAND_SEEK_IN_CURRENT_MEDIA_ITEM); add(Player.COMMAND_SEEK_BACK); add(Player.COMMAND_SEEK_FORWARD) }
            if (MediaSessionAction.Seek in s.actions || timeline.entries.size > 1) add(Player.COMMAND_SEEK_TO_MEDIA_ITEM)
            if (timeline.entries.size > 1) add(Player.COMMAND_CHANGE_MEDIA_ITEMS)
            if (media != null) { add(Player.COMMAND_SET_MEDIA_ITEM); add(Player.COMMAND_CHANGE_MEDIA_ITEMS) }
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
            .setDeviceInfo(if (remote) REMOTE_DEVICE else DeviceInfo.UNKNOWN)
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
            val seekable = MediaSessionAction.Seek in s.actions
            val durationUs = Util.msToUs(meta.durationMs.toLong())
            if (timeline.entries.isEmpty()) {
                val id = meta.trackId ?: "current"
                builder.setPlaylist(listOf(MediaItemData.Builder(id).setMediaItem(MediaItem.Builder().setMediaId(id).setMediaMetadata(mm).build()).setMediaMetadata(mm).setDurationUs(durationUs).setIsSeekable(seekable).build()))
                builder.setCurrentMediaItemIndex(0)
            } else {
                builder.setPlaylist(timeline.entries.mapIndexed { i, entry ->
                    val id = MediaId.QueueItem(entry.item.key).format()
                    if (i == timeline.currentIndex) {
                        MediaItemData.Builder(id).setMediaItem(MediaItem.Builder().setMediaId(id).setMediaMetadata(mm).build()).setMediaMetadata(mm).setDurationUs(durationUs).setIsSeekable(seekable).build()
                    } else {
                        val em = trackMetadata(entry.track, artwork(entry.track))
                        MediaItemData.Builder(id).setMediaItem(MediaItem.Builder().setMediaId(id).setMediaMetadata(em).build()).setMediaMetadata(em).setDurationUs(Util.msToUs(entry.track.durationMs.toLong())).build()
                    }
                })
                builder.setCurrentMediaItemIndex(timeline.currentIndex)
            }
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
        val t = timeline
        when {
            seekCommand == Player.COMMAND_SEEK_TO_NEXT || seekCommand == Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM -> dispatch(Commands.mediaSessionCommand(MediaSessionAction.Next))
            seekCommand == Player.COMMAND_SEEK_TO_PREVIOUS || seekCommand == Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM -> dispatch(Commands.mediaSessionCommand(MediaSessionAction.Previous))
            // Another queue entry (a controller's queue view): jump to it.
            mediaItemIndex != C.INDEX_UNSET && mediaItemIndex != t.currentIndex && mediaItemIndex in t.entries.indices ->
                dispatch(Commands.jumpToQueueItem(t.entries[mediaItemIndex].item.key))
            else -> dispatch(Commands.mediaSessionCommand(MediaSessionAction.Seek, (if (positionMs == C.TIME_UNSET) 0L else positionMs).coerceAtLeast(0).toDouble()))
        }
        return Futures.immediateVoidFuture()
    }

    override fun handleMoveMediaItems(fromIndex: Int, toIndex: Int, newIndex: Int): ListenableFuture<*> {
        // Only upcoming entries move, and only among upcoming entries: history and the current
        // entry are not the core's to reorder.
        val t = timeline
        val moved = t.entries.subList(fromIndex.coerceIn(0, t.entries.size), toIndex.coerceIn(0, t.entries.size))
        if (moved.isNotEmpty() && fromIndex > t.currentIndex && newIndex > t.currentIndex) {
            moved.forEachIndexed { i, entry -> dispatch(Commands.moveQueueItem(entry.item.key, t.upcomingIndex(newIndex) + i)) }
        }
        return Futures.immediateVoidFuture()
    }

    override fun handleRemoveMediaItems(fromIndex: Int, toIndex: Int): ListenableFuture<*> {
        val t = timeline
        val keys = t.entries.subList(fromIndex.coerceIn(0, t.entries.size), toIndex.coerceIn(0, t.entries.size))
            .filterIndexed { i, _ -> fromIndex + i != t.currentIndex }
            .map { it.item.key }
        if (keys.isNotEmpty()) dispatch(Commands.removeQueueItems(keys))
        return Futures.immediateVoidFuture()
    }

    override fun handleAddMediaItems(index: Int, mediaItems: List<MediaItem>): ListenableFuture<*> {
        // Right after the current entry is "play next"; anywhere later is "play later".
        media?.enqueue(mediaItems.map { it.mediaId }, next = timeline.entries.isEmpty() || index <= timeline.currentIndex + 1)
        return Futures.immediateVoidFuture()
    }

    override fun handleSetMediaItems(mediaItems: List<MediaItem>, startIndex: Int, startPositionMs: Long): ListenableFuture<*> {
        if (mediaItems.isNotEmpty()) media?.play(mediaItems.map { it.mediaId }, if (startIndex == C.INDEX_UNSET) 0 else startIndex)
        return Futures.immediateVoidFuture()
    }
}
