package app.hocket.cast

import android.content.Context
import android.net.Uri
import android.net.wifi.WifiManager
import android.os.PowerManager
import android.os.SystemClock
import app.hocket.core.Commands
import app.hocket.core.api.BackendCommand
import app.hocket.core.api.BackendReport
import app.hocket.core.api.BackendReportBufferingInner
import app.hocket.core.api.BackendReportEndedInner
import app.hocket.core.api.BackendReportErrorInner
import app.hocket.core.api.BackendReportPausedInner
import app.hocket.core.api.BackendReportPlayingInner
import app.hocket.core.api.BackendReportPositionInner
import app.hocket.core.api.BackendReportPreBufferReadyInner
import app.hocket.core.api.BackendReportReadyInner
import app.hocket.core.api.BackendReportSystemVolumeInner
import app.hocket.core.api.BackendReportTransitionedToNextInner
import app.hocket.core.api.Command
import app.hocket.core.api.MediaSource as CoreMediaSource
import app.hocket.playback.PlaybackLog
import app.hocket.playback.RendererBackend
import com.google.android.gms.cast.Cast
import com.google.android.gms.cast.MediaError
import com.google.android.gms.cast.MediaInfo
import com.google.android.gms.cast.MediaLoadRequestData
import com.google.android.gms.cast.MediaMetadata
import com.google.android.gms.cast.MediaQueueData
import com.google.android.gms.cast.MediaQueueItem
import com.google.android.gms.cast.MediaSeekOptions
import com.google.android.gms.cast.MediaStatus
import com.google.android.gms.cast.framework.CastSession
import com.google.android.gms.cast.framework.media.RemoteMediaClient
import com.google.android.gms.common.images.WebImage
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import org.json.JSONObject

/**
 * The core's backend commands on a Cast receiver (the Default Media Receiver), through the session's
 * [RemoteMediaClient]. Main thread.
 *
 * - `Load` loads a receiver queue of the item and, when given, its follow-up, so the receiver moves
 *   on by itself while the phone sleeps; `SetNext` replaces what follows the current item. The queue
 *   stays "current (+ next)": a played item is removed once the receiver has moved past it.
 * - Reports come from the receiver's status: the item it plays is found by the queue key in its
 *   media's custom data. Moving on to the follow-up reports `Ended` then `TransitionedToNext`, as
 *   the contract has it ([CastStatus]).
 * - A transcoded stream can't be seeked on the receiver (the server sends it chunked, without
 *   ranges): a seek in one, or a load part-way into one, reloads it from the server at that point
 *   (`timeOffset`), and positions are reported with that offset added.
 * - `SetSystemVolume` and the reports' `SystemVolume` are the receiver's volume. Per-track gain
 *   (`SetVolume`, ReplayGain) has nowhere to go on the Default Media Receiver and is ignored.
 * - While the receiver plays or buffers, a partial wake lock and a Wi-Fi lock keep this phone able
 *   to answer it (queue the next item, keep the Connect lease).
 * - A receiver that can't play the first thing it is given (it can't reach the server, or doesn't
 *   trust its certificate) ends the session through [onUnreachable], which says why; later failures
 *   are reported to the core, which skips the item.
 * - Some receivers claim a format and then crash on it (the Pixel Tablet's, on FLAC): the session
 *   just ends. [crashSuspect] names what it had just started, for the controller to act on.
 */
internal class CastRenderer(
    private val context: Context,
    private val scope: CoroutineScope,
    private val dispatch: (Command) -> Unit,
    private val onUnreachable: (String) -> Unit,
) : RendererBackend {
    private var session: CastSession? = null
    private val client: RemoteMediaClient? get() = session?.remoteMediaClient
    private var active = false
    private var current: CoreMediaSource? = null
    private var next: CoreMediaSource? = null
    /** Where the current item's stream starts (a transcode loaded at `timeOffset`). */
    private var offsetMs = 0L
    /** The receiver's queue item id for [current], once a status names it. */
    private var currentItemId: Int? = null
    /** The key the receiver has queued after [current] (what [next] was when last sent). */
    private var queuedNextKey: String? = null
    /** The receiver's item ids from before the last load: their statuses are about the old queue. */
    private var staleIds: Set<Int> = emptySet()
    /** Each load's generation; a load reports at most one error. */
    private var loadGen = 0
    private var failedGen = -1
    private val status = CastStatus()
    /** Something has played on the receiver this session: a failure now is the item's, not the server's reach. */
    private var played = false
    /** When [current] was loaded, or (later) first played (elapsed realtime); 0 once it ended. */
    private var startedAt = 0L
    /** The item [startedAt] last marked playing. */
    private var startedKey: String? = null
    private var positionJob: Job? = null
    private val wakeLock = (context.getSystemService(Context.POWER_SERVICE) as PowerManager)
        .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "hocket:cast").apply { setReferenceCounted(false) }
    private val wifiLock = (context.applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager)
        .let {
            @Suppress("DEPRECATION")
            it.createWifiLock(WifiManager.WIFI_MODE_FULL_HIGH_PERF, "hocket:cast")
        }.apply { setReferenceCounted(false) }

    private val callback = object : RemoteMediaClient.Callback() {
        override fun onStatusUpdated() = onStatus()
        override fun onQueueStatusUpdated() = onStatus()
        override fun onMediaError(error: MediaError) {
            PlaybackLog.w("cast media error: ${error.type} ${error.reason} ${error.detailedErrorCode}")
            fail(error.reason ?: error.type ?: "media error")
        }
    }

    private val castListener = object : Cast.Listener() {
        override fun onVolumeChanged() = reportVolume()
    }

    fun attach(session: CastSession) {
        if (this.session === session) return
        detach()
        this.session = session
        session.remoteMediaClient?.registerCallback(callback)
        session.addCastListener(castListener)
        played = false
        reportVolume()
    }

    fun detach() {
        session?.let {
            it.remoteMediaClient?.unregisterCallback(callback)
            it.removeCastListener(castListener)
        }
        session = null
        reset()
    }

    override fun setActive(active: Boolean) {
        this.active = active
        if (active) reportVolume() else reset()
    }

    private fun reset() {
        stopPositionLoop()
        releaseLocks()
        current = null
        next = null
        startedAt = 0
        startedKey = null
        offsetMs = 0
        currentItemId = null
        queuedNextKey = null
        staleIds = emptySet()
        status.reset()
    }

    override fun isBusy(): Boolean = when (client?.mediaStatus?.playerState) {
        MediaStatus.PLAYER_STATE_PLAYING, MediaStatus.PLAYER_STATE_BUFFERING, MediaStatus.PLAYER_STATE_LOADING -> true
        else -> false
    }

    override fun handle(command: BackendCommand) {
        val client = client
        if (client == null) {
            PlaybackLog.w("cast: no session for ${command::class.simpleName}")
            return
        }
        when (command) {
            is BackendCommand.Load -> load(command.data.source, command.data.next, command.data.position_ms.toLong(), command.data.play)
            is BackendCommand.SetNext -> setNext(command.data.next)
            BackendCommand.Play -> {
                val st = client.mediaStatus
                val finished = st?.playerState == MediaStatus.PLAYER_STATE_IDLE && st.idleReason == MediaStatus.IDLE_REASON_FINISHED
                val c = current
                // Played to the end: the receiver has nothing to resume; start it again.
                if (finished && c != null) load(c, next, 0, true) else client.play()
            }
            BackendCommand.Pause -> client.pause()
            BackendCommand.Stop -> {
                reset()
                client.stop()
            }
            is BackendCommand.Seek -> seek(command.data.position_ms.toLong())
            // Per-track gain has nowhere to go on the Default Media Receiver.
            is BackendCommand.SetVolume -> Unit
            is BackendCommand.SetSystemVolume -> {
                val s = session ?: return
                val volume = command.data.volume.coerceIn(0.0, 1.0)
                runCatching {
                    s.volume = volume
                    if (s.isMute != command.data.muted) s.isMute = command.data.muted
                }.onFailure { PlaybackLog.w("cast volume: ${it.message}") }
                // The session still holds the receiver's last level until it answers
                // (onVolumeChanged): answer with what was asked, so a dragged slider doesn't jump.
                report(BackendReport.SystemVolume(BackendReportSystemVolumeInner(volume, command.data.muted)))
            }
            // Nothing to warm up on this phone: the receiver loads it when it is told to.
            is BackendCommand.PreBuffer -> report(BackendReport.PreBufferReady(BackendReportPreBufferReadyInner(command.data.source.key)))
            BackendCommand.DiscardPreBuffer -> Unit
            // The follow-up is always queued on the receiver (the core keeps sending it).
            is BackendCommand.SetGapless -> Unit
            // Previews play on the phone (PlaybackService routes them to ExoBackend).
            is BackendCommand.PreviewLoad, BackendCommand.PreviewPlay, BackendCommand.PreviewPause,
            is BackendCommand.PreviewSeek, BackendCommand.PreviewStop -> Unit
        }
    }

    private fun load(source: CoreMediaSource, follow: CoreMediaSource?, positionMs: Long, play: Boolean) {
        val client = client ?: return
        // The old item's position must not be reported as this one's.
        stopPositionLoop()
        staleIds = client.mediaStatus?.queueItems?.map { it.itemId }?.toSet().orEmpty()
        current = source
        next = follow
        currentItemId = null
        queuedNextKey = follow?.key
        val gen = ++loadGen
        startedAt = SystemClock.elapsedRealtime()
        startedKey = null
        // `timeOffset` is in whole seconds; the rest is a seek within the stream.
        offsetMs = if (source.transcoded) positionMs / 1000 * 1000 else 0
        status.loading(source.key)
        report(BackendReport.Buffering(BackendReportBufferingInner(source.key, true)))
        val items = listOfNotNull(queueItem(source, offsetMs), follow?.let { queueItem(it, 0) })
        val request = MediaLoadRequestData.Builder()
            .setQueueData(
                MediaQueueData.Builder()
                    .setItems(items)
                    .setStartIndex(0)
                    .setRepeatMode(MediaStatus.REPEAT_MODE_REPEAT_OFF)
                    .build(),
            )
            .setAutoplay(play)
            .setCurrentTime(positionMs - offsetMs)
            .build()
        client.load(request).setResultCallback { result ->
            if (!result.status.isSuccess && gen == loadGen) {
                PlaybackLog.w("cast load failed: ${result.status.statusCode} ${result.status.statusMessage}")
                fail(result.status.statusMessage ?: "load failed (${result.status.statusCode})")
            }
        }
    }

    private fun setNext(follow: CoreMediaSource?) {
        next = follow
        syncNext()
    }

    /**
     * Make the receiver's queue after the current item hold [next]. Until a status names the current
     * item's id (just after a load) there is nothing to put it after: [onStatus] calls this again.
     */
    private fun syncNext() {
        val client = client ?: return
        val id = currentItemId ?: return
        val want = next
        if (queuedNextKey == want?.key) return
        val items = client.mediaStatus?.queueItems ?: return
        val at = items.indexOfFirst { it.itemId == id }
        if (at < 0) return
        val after = items.drop(at + 1).map { it.itemId }.toIntArray()
        if (after.isNotEmpty()) client.queueRemoveItems(after, null)
        want?.let { client.queueAppendItem(queueItem(it, 0), null) }
        queuedNextKey = want?.key
    }

    private fun seek(positionMs: Long) {
        val client = client ?: return
        val c = current ?: return
        if (c.transcoded) {
            val play = client.mediaStatus?.playerState != MediaStatus.PLAYER_STATE_PAUSED
            load(c, next, positionMs, play)
        } else {
            client.seek(MediaSeekOptions.Builder().setPosition(positionMs).build())
        }
    }

    private fun onStatus() {
        if (!active) return
        val st = client?.mediaStatus ?: return
        val itemId = st.currentItemId
        val playing = keyOf(st.getQueueItemById(itemId))
        val c = current ?: return
        // Still the queue from before the last load: nothing to say about it.
        if (currentItemId == null && itemId in staleIds) return
        if (currentItemId == null && playing == c.key) currentItemId = itemId
        // The receiver moved on to the follow-up by itself: told by the item id, since the
        // follow-up may be the same key (Repeat All over one item).
        val n = next
        val was = currentItemId
        if (was != null && itemId != 0 && itemId != was && n != null && playing == n.key) {
            report(BackendReport.Ended(BackendReportEndedInner(c.key)))
            current = n
            next = null
            offsetMs = 0
            currentItemId = itemId
            queuedNextKey = null
            startedAt = SystemClock.elapsedRealtime()
            startedKey = n.key
            status.transitioned(n.key)
            report(BackendReport.TransitionedToNext(BackendReportTransitionedToNextInner(n.key)))
            report(BackendReport.Position(BackendReportPositionInner(n.key, 0u)))
            client?.queueRemoveItems(intArrayOf(was), null)
        }
        syncNext()
        val key = current?.key ?: return
        if (playing != null && playing != key) return
        val duration = st.mediaInfo?.streamDuration?.takeIf { it > 0 }?.let { (it + offsetMs).toUInt() }
        for (event in status.update(key, st.playerState, st.idleReason)) {
            when (event) {
                CastStatus.Event.Ready -> {
                    report(BackendReport.Ready(BackendReportReadyInner(key, duration)))
                    report(BackendReport.Buffering(BackendReportBufferingInner(key, false)))
                }
                CastStatus.Event.Playing -> {
                    if (startedKey != key) {
                        startedAt = SystemClock.elapsedRealtime()
                        startedKey = key
                    }
                    played = true
                    report(BackendReport.Playing(BackendReportPlayingInner(key, position())))
                    startPositionLoop()
                }
                CastStatus.Event.Paused -> {
                    stopPositionLoop()
                    report(BackendReport.Paused(BackendReportPausedInner(key, position())))
                }
                CastStatus.Event.Buffering -> report(BackendReport.Buffering(BackendReportBufferingInner(key, true)))
                CastStatus.Event.Ended -> {
                    startedAt = 0
                    stopPositionLoop()
                    report(BackendReport.Ended(BackendReportEndedInner(key)))
                }
                CastStatus.Event.Failed -> fail("the receiver stopped with an error")
            }
        }
        if (isBusy()) acquireLocks() else releaseLocks()
    }

    /**
     * What the receiver had just started untranscoded, if anything: a session dying now likely died
     * of it. Read before [detach], which forgets it.
     */
    fun crashSuspect(): CoreMediaSource? {
        val c = current ?: return null
        if (c.transcoded || startedAt == 0L) return null
        return c.takeIf { SystemClock.elapsedRealtime() - startedAt <= CRASH_WINDOW_MS }
    }

    /** The receiver is gone for a moment (reconnecting): the core holds the position where it is, still playing. */
    fun hold() {
        stopPositionLoop()
        current?.key?.let { report(BackendReport.Buffering(BackendReportBufferingInner(it, true))) }
    }

    /** The receiver couldn't play [current]: before anything has played, the server is out of its reach. */
    private fun fail(message: String) {
        val key = current?.key ?: return
        // One load, one error (a media error, the load's result and an error status can all say it).
        if (failedGen == loadGen) return
        failedGen = loadGen
        stopPositionLoop()
        if (!played) {
            val name = session?.castDevice?.friendlyName ?: context.getString(R.string.cast_receiver)
            reset()
            onUnreachable(context.getString(R.string.cast_unreachable, name))
            return
        }
        status.failed()
        report(BackendReport.Error(BackendReportErrorInner(key, "Cast: $message", true)))
    }

    private fun position(): UInt = ((client?.approximateStreamPosition ?: 0L) + offsetMs).coerceAtLeast(0).toUInt()

    private fun startPositionLoop() {
        if (positionJob?.isActive == true) return
        positionJob = scope.launch {
            while (isActive && active) {
                current?.key?.let { report(BackendReport.Position(BackendReportPositionInner(it, position()))) }
                // Renewed while it plays: a long track brings no status to renew it.
                if (isBusy()) acquireLocks()
                delay(POSITION_INTERVAL_MS)
            }
        }
    }

    private fun stopPositionLoop() {
        positionJob?.cancel()
        positionJob = null
    }

    private fun reportVolume() {
        if (!active) return
        val s = session ?: return
        val (volume, muted) = runCatching { s.volume to s.isMute }.getOrNull() ?: return
        report(BackendReport.SystemVolume(BackendReportSystemVolumeInner(volume.coerceIn(0.0, 1.0), muted)))
    }

    private fun report(report: BackendReport) {
        if (active) dispatch(Commands.backendReport(report))
    }

    private fun acquireLocks() {
        wakeLock.acquire(WAKE_LOCK_TIMEOUT_MS)
        if (!wifiLock.isHeld) wifiLock.acquire()
    }

    private fun releaseLocks() {
        if (wakeLock.isHeld) wakeLock.release()
        if (wifiLock.isHeld) wifiLock.release()
    }

    private fun queueItem(source: CoreMediaSource, offsetMs: Long): MediaQueueItem {
        val t = source.track
        val metadata = MediaMetadata(MediaMetadata.MEDIA_TYPE_MUSIC_TRACK).apply {
            putString(MediaMetadata.KEY_TITLE, t.title)
            t.artist?.let { putString(MediaMetadata.KEY_ARTIST, it) }
            t.album?.let { putString(MediaMetadata.KEY_ALBUM_TITLE, it) }
            source.artworkUrl?.let { addImage(WebImage(Uri.parse(it))) }
        }
        val info = MediaInfo.Builder(source.key)
            .setContentUrl(streamUrl(source.url, if (source.transcoded) offsetMs else 0))
            .setContentType(source.mimeType ?: "audio/mpeg")
            .setStreamType(MediaInfo.STREAM_TYPE_BUFFERED)
            .setMetadata(metadata)
            .apply { if (t.durationMs.toLong() > offsetMs) setStreamDuration(t.durationMs.toLong() - offsetMs) }
            .setCustomData(JSONObject().put(KEY, source.key))
            .build()
        return MediaQueueItem.Builder(info)
            .setAutoplay(true)
            .setPreloadTime(PRELOAD_S)
            .build()
    }

    companion object {
        const val POSITION_INTERVAL_MS = 1_000L
        const val PRELOAD_S = 20.0
        /** A session ending this soon after an item started is taken as the receiver crashing on it. */
        const val CRASH_WINDOW_MS = 10_000L
        /** Renewed by statuses and the position loop while it plays; a lost session can't hold the phone awake for long. */
        const val WAKE_LOCK_TIMEOUT_MS = 10 * 60_000L
        private const val KEY = "hocketKey"

        fun keyOf(item: MediaQueueItem?): String? = item?.media?.customData?.optString(KEY)?.takeIf { it.isNotEmpty() }

        /** The server's stream URL starting [offsetMs] in (Subsonic's `timeOffset`, whole seconds). */
        fun streamUrl(url: String, offsetMs: Long): String {
            if (offsetMs <= 0) return url
            val sep = if (url.contains('?')) '&' else '?'
            return "$url${sep}timeOffset=${offsetMs / 1000}"
        }
    }
}
