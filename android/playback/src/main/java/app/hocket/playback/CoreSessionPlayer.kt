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
import app.hocket.core.client.continuesTo
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import android.os.Looper
import android.util.Log

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
 * ([DeviceInfo.PLAYBACK_TYPE_REMOTE]) with [ConnectRoutes.SESSION_ID] as its routing controller id,
 * so the system pairs the session with [ConnectRouteProvider]'s routing session and shows that
 * device as the output. The controls keep working: the core forwards them. Once that device has
 * reported its volume ([remoteVolume]) the session's device volume is that device's, in
 * [ConnectRoutes.VOLUME_STEPS] steps: the phone's volume keys and the system volume panel drive it.
 * While this phone plays, its volume is the system music volume itself (the core mirrors it).
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

        /**
         * Playing-next and upcoming entries shown after the current one. The playlist is rebuilt,
         * diffed and sent over binder on every state change (each Next, each position update): a
         * long playlist in full made that slow on the main thread. Further entries come into view as
         * the queue moves.
         */
        const val UPCOMING_SHOWN = 100

        private val REMOTE_DEVICE: DeviceInfo = DeviceInfo.Builder(DeviceInfo.PLAYBACK_TYPE_REMOTE)
            .setRoutingControllerId(ConnectRoutes.SESSION_ID)
            .build()

        /** The playing device's volume can be set from here: the session offers device volume. */
        private val REMOTE_DEVICE_WITH_VOLUME: DeviceInfo = DeviceInfo.Builder(DeviceInfo.PLAYBACK_TYPE_REMOTE)
            .setRoutingControllerId(ConnectRoutes.SESSION_ID)
            .setMinVolume(0)
            .setMaxVolume(ConnectRoutes.VOLUME_STEPS)
            .build()

        /**
         * The timeline for [queue] around the playing track: the last [HISTORY_SHOWN] history
         * entries, the current one, then the first [UPCOMING_SHOWN] of playing next and upcoming. Empty when the queue's current entry is
         * not [currentTrackId] (or there is none).
         */
        fun timeline(queue: QueueView?, currentTrackId: String?): QueueTimeline {
            val current = queue?.current ?: return QueueTimeline.EMPTY
            if (currentTrackId != null && current.track.id != currentTrackId) return QueueTimeline.EMPTY
            // Media3 requires unique playlist UIDs and the UID is the queue key. A document from an
            // older core can repeat keys (#54); drop the repeats rather than crash, keeping current.
            val seen = hashSetOf(current.item.key)
            val shownHistory = queue.history.takeLast(HISTORY_SHOWN)
            val history = shownHistory.filter { seen.add(it.item.key) }
            var dropped = shownHistory.size - history.size
            val after = ArrayList<QueueEntry>(minOf(UPCOMING_SHOWN, queue.playingNext.size + queue.upcoming.size))
            for (entry in queue.playingNext.asSequence() + queue.upcoming.asSequence()) {
                if (after.size >= UPCOMING_SHOWN) break
                if (seen.add(entry.item.key)) after.add(entry) else dropped++
            }
            if (dropped > 0) Log.w(TAG, "dropped $dropped queue entries with duplicate keys")
            return QueueTimeline(history + current + after, history.size)
        }

        private const val TAG = "CoreSessionPlayer"
    }

    /** The queue entries the playlist shows and where the current one is. */
    data class QueueTimeline(val entries: List<QueueEntry>, val currentIndex: Int) {
        /** Index into the core's combined playing-next + upcoming list (`MoveQueueItem.toIndex`). */
        fun upcomingIndex(index: Int): Int = index - currentIndex - 1

        companion object { val EMPTY = QueueTimeline(emptyList(), 0) }
    }

    private var current: MediaSessionState = MediaSessionState(null, false, app.hocket.core.api.PositionStamp(0u, 0.0, 1.0, false), false, RepeatMode.Off, 1.0, false, emptyList(), true)
    private var clockOffsetMs = 0.0
    private var remote = false
    private var remoteVolume = false
    private var queue: QueueView? = null
    private var timeline = QueueTimeline.EMPTY

    /** Apply a new queue view from the core. Main thread. */
    fun applyQueue(view: QueueView) {
        val prev = queue
        val trackId = current.metadata?.trackId
        queue = view
        // Rating/love and other metadata-only queue views must not rebuild the
        // Media3 playlist; [apply] still refreshes the current item's metadata.
        if (prev != null && sameTimelineKeys(prev, view, trackId)) return
        invalidateState()
    }

    private fun sameTimelineKeys(a: QueueView, b: QueueView, trackId: String?): Boolean {
        val left = timeline(a, trackId)
        val right = timeline(b, trackId)
        if (left.currentIndex != right.currentIndex || left.entries.size != right.entries.size) return false
        for (i in left.entries.indices) {
            if (left.entries[i].item.key != right.entries[i].item.key) return false
        }
        return true
    }

    /** Apply a new state from the core and notify controllers. Main thread. */
    fun apply(state: MediaSessionState, clockOffsetMs: Double = this.clockOffsetMs, remote: Boolean = this.remote, remoteVolume: Boolean = this.remoteVolume) {
        val prev = current
        val remoteFlagsChanged = this.remote != remote || this.remoteVolume != (remote && remoteVolume)
        current = state
        this.clockOffsetMs = clockOffsetMs
        this.remote = remote
        this.remoteVolume = remote && remoteVolume
        // Position ticks only refresh the stamp; Media3 polls the live supplier below,
        // so rebuilding the playlist over binder is wasted. A seek is not a tick: it
        // must reach controllers as a discontinuity.
        if (!remoteFlagsChanged && prev.isPositionTick(state)) return
        if (prev.metadata?.trackId == state.metadata?.trackId && state.metadata != null && !prev.position.continuesTo(state.position)) {
            seekedTo = PositionClock.extrapolate(state.position, now(), clockOffsetMs, state.metadata?.durationMs?.toLong())
        }
        invalidateState()
    }

    /** A seek made elsewhere (the app, another device) for the next [getState] to report once. */
    private var seekedTo: Long? = null

    /** The track behind queue entry [key] in the current queue view; null when it is not there. */
    fun trackIdForQueueKey(key: String): String? = queue?.let { q ->
        (q.history.asSequence() + listOfNotNull(q.current) + q.playingNext + q.upcoming).firstOrNull { it.item.key == key }?.track?.id
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
            if (remoteVolume) {
                add(Player.COMMAND_GET_DEVICE_VOLUME)
                add(Player.COMMAND_SET_DEVICE_VOLUME_WITH_FLAGS); add(Player.COMMAND_ADJUST_DEVICE_VOLUME_WITH_FLAGS)
            }
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
            .setDeviceInfo(if (remoteVolume) REMOTE_DEVICE_WITH_VOLUME else if (remote) REMOTE_DEVICE else DeviceInfo.UNKNOWN)
            .apply { if (remoteVolume) setDeviceVolume(ConnectRoutes.steps(s.volume)).setIsDeviceMuted(s.muted == true) }
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
            val duration = meta.durationMs.toLong()
            builder.setContentPositionMs {
                val live = current
                PositionClock.extrapolate(live.position, now(), clockOffsetMs, live.metadata?.durationMs?.toLong() ?: duration)
            }
            builder.setContentBufferedPositionMs { duration }
            seekedTo?.let { builder.setPositionDiscontinuity(Player.DISCONTINUITY_REASON_SEEK, it) }
        }
        seekedTo = null
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

    // The playing device's volume (only offered while another device plays and reports it).
    override fun handleSetDeviceVolume(deviceVolume: Int, flags: Int): ListenableFuture<*> {
        dispatch(Commands.setVolume(ConnectRoutes.level(deviceVolume)))
        return Futures.immediateVoidFuture()
    }

    override fun handleIncreaseDeviceVolume(flags: Int): ListenableFuture<*> {
        val from = if (current.muted == true) 0 else ConnectRoutes.steps(current.volume)
        dispatch(Commands.setVolume(ConnectRoutes.level(from + 1)))
        return Futures.immediateVoidFuture()
    }

    override fun handleDecreaseDeviceVolume(flags: Int): ListenableFuture<*> {
        // Down while muted stays muted.
        if (current.muted == true) return Futures.immediateVoidFuture()
        dispatch(Commands.setVolume(ConnectRoutes.level(ConnectRoutes.steps(current.volume) - 1)))
        return Futures.immediateVoidFuture()
    }

    override fun handleSetDeviceMuted(muted: Boolean, flags: Int): ListenableFuture<*> {
        dispatch(Commands.setMuted(muted))
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
