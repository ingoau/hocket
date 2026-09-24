package app.hocket.playback

import android.content.Context
import android.net.Uri
import android.util.Log
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.exoplayer.DefaultLoadControl
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import androidx.media3.exoplayer.source.MediaSource
import app.hocket.core.Commands
import app.hocket.core.CoreStreams
import app.hocket.core.api.BackendCommand
import app.hocket.core.api.BackendReport
import app.hocket.core.api.BackendReportAudioFocusLostInner
import app.hocket.core.api.BackendReportBufferingInner
import app.hocket.core.api.BackendReportEndedInner
import app.hocket.core.api.BackendReportErrorInner
import app.hocket.core.api.BackendReportPausedInner
import app.hocket.core.api.BackendReportPlayingInner
import app.hocket.core.api.BackendReportPositionInner
import app.hocket.core.api.BackendReportPreBufferReadyInner
import app.hocket.core.api.BackendReportReadyInner
import app.hocket.core.api.BackendReportTransitionedToNextInner
import app.hocket.core.api.Command
import app.hocket.core.api.MediaSource as CoreMediaSource
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlin.math.pow

/**
 * The external `PlaybackBackend`: turns [BackendCommand]s from the core into ExoPlayer calls and
 * posts [BackendReport]s back through [dispatch]. Must be used from the main thread (ExoPlayer's
 * application thread).
 *
 * Mapping, as documented in `android/README.md`:
 * - `Load` builds a per-item media source through `DefaultMediaSourceFactory` over
 *   [CoreStreamDataSourceFactory] (so each item carries its own headers) and, when `next` is given, a
 *   second one behind it; ExoPlayer's playlist transition is the gapless join. `hocket-stream://`
 *   sources are read through the core ([HocketStreamDataSource], which the core caches); anything
 *   else (`file:`, a direct server URL) through `DefaultDataSource`.
 * - `SetNext` replaces everything after the current item.
 * - `TransitionedToNext` is detected from `onMediaItemTransition(reason = AUTO)`, preceded by the
 *   `Ended` of the played item (ExoPlayer reports no end for an item it advanced past, and the
 *   contract is `Ended` then `TransitionedToNext`, as the native backend does); the played item is
 *   then removed so the playlist is always "current (+ next)".
 * - `PreBuffer` uses a second, silent ExoPlayer prepared at the requested position; `DiscardPreBuffer`
 *   releases its media. A subsequent `Load` of the same key still goes through the main player: the
 *   OS/HTTP cache makes it start fast, which is the point of the pre-buffer.
 * - `gain_db` is applied as `player.volume = 10^(gain/20) * masterVolume`, clamped to 1.0. Media3 has
 *   no gain stage, so positive gain is an approximation (it cannot amplify) and ReplayGain-style
 *   attenuation is exact.
 * - Audio focus and becoming-noisy are handled by ExoPlayer; focus loss is reported as
 *   `AudioFocusLost` (transient when Media3 reports a suppression reason instead of a pause: ExoPlayer
 *   then resumes by itself when focus returns and reports `Playing`).
 * - The player holds a wake lock and a Wi-Fi lock while playing ([C.WAKE_MODE_NETWORK]): without
 *   them a stream stalls once the screen is off and the CPU or Wi-Fi radio sleeps.
 * - Network failures leave ExoPlayer idle with an error, so they are retried here with backoff
 *   ([recoveryDelayMs]; `prepare()` resumes at the same position) and reported as non-fatal; only when
 *   the retries run out is the error fatal and the core's own retry/skip takes over.
 */
class ExoBackend(
    private val context: Context,
    private val scope: CoroutineScope,
    private val dispatch: (Command) -> Unit,
    /** The running core's stream reader, read on each open (a restarted service has a new core). */
    private val streams: () -> CoreStreams? = { CoreHost.current as? CoreStreams },
) {
    internal companion object {
        private const val TAG = "ExoBackend"
        private const val POSITION_INTERVAL_MS = 750L
        private val RECOVERY_DELAYS_MS = longArrayOf(1_000, 2_000, 4_000, 8_000, 15_000, 30_000)

        /** Backoff before recovery attempt [attempt] (0-based) of a network error, or null when out of attempts. */
        internal fun recoveryDelayMs(attempt: Int): Long? = RECOVERY_DELAYS_MS.getOrNull(attempt)

        /** Errors a later `prepare()` can fix: the connection, not the media. */
        internal fun isRecoverable(errorCode: Int): Boolean =
            errorCode == PlaybackException.ERROR_CODE_IO_NETWORK_CONNECTION_FAILED ||
                errorCode == PlaybackException.ERROR_CODE_IO_NETWORK_CONNECTION_TIMEOUT
    }

    val player: ExoPlayer = ExoPlayer.Builder(context)
        .setAudioAttributes(AudioAttributes.Builder().setUsage(C.USAGE_MEDIA).setContentType(C.AUDIO_CONTENT_TYPE_MUSIC).build(), true)
        .setHandleAudioBecomingNoisy(true)
        .setWakeMode(C.WAKE_MODE_NETWORK)
        .setLoadControl(DefaultLoadControl.Builder().setBufferDurationsMs(30_000, 120_000, 2_500, 5_000).build())
        .setMediaSourceFactory(mediaSourceFactory())
        .build()

    private var preBufferPlayer: ExoPlayer? = null
    private var preBufferKey: String? = null
    private var masterVolume = 1.0
    private var currentGainDb = 0.0
    private var positionJob: Job? = null
    private var recoveryJob: Job? = null
    /** Recovery attempts for the current error streak; reset once the player is ready again. */
    private var recoveryAttempts = 0
    /** The key of the last `Load`, so an error raised after the playlist emptied is still attributable. */
    private var lastLoadedKey: String? = null
    /** Keys by media id, so reports name the queue key the core gave us. */
    private fun currentKey(): String? = player.currentMediaItem?.mediaId

    /** The key an error report is attributed to; null when there is nothing to attribute it to (after `Stop`). */
    internal fun errorKey(): String? = currentKey() ?: lastLoadedKey

    init {
        player.addListener(object : Player.Listener {
            override fun onPlaybackStateChanged(playbackState: Int) {
                val key = currentKey() ?: return
                when (playbackState) {
                    Player.STATE_READY -> {
                        recoveryAttempts = 0
                        val duration = player.duration.takeIf { it != C.TIME_UNSET }?.toUInt()
                        report(BackendReport.Ready(BackendReportReadyInner(key, duration)))
                        report(BackendReport.Buffering(BackendReportBufferingInner(key, false)))
                        if (player.playWhenReady) report(BackendReport.Playing(BackendReportPlayingInner(key, position())))
                    }
                    Player.STATE_BUFFERING -> report(BackendReport.Buffering(BackendReportBufferingInner(key, true)))
                    Player.STATE_ENDED -> {
                        stopPositionLoop()
                        report(BackendReport.Position(BackendReportPositionInner(key, position())))
                        report(BackendReport.Ended(BackendReportEndedInner(key)))
                    }
                    Player.STATE_IDLE -> Unit
                }
            }

            override fun onIsPlayingChanged(isPlaying: Boolean) {
                val key = currentKey() ?: return
                if (isPlaying) {
                    report(BackendReport.Playing(BackendReportPlayingInner(key, position())))
                    startPositionLoop()
                } else {
                    stopPositionLoop()
                    if (player.playbackState != Player.STATE_ENDED && !player.isLoading) {
                        report(BackendReport.Paused(BackendReportPausedInner(key, position())))
                    }
                }
            }

            override fun onPlayWhenReadyChanged(playWhenReady: Boolean, reason: Int) {
                if (!playWhenReady && reason == Player.PLAY_WHEN_READY_CHANGE_REASON_AUDIO_FOCUS_LOSS) {
                    report(BackendReport.AudioFocusLost(BackendReportAudioFocusLostInner(transient = false)))
                }
                if (!playWhenReady && reason == Player.PLAY_WHEN_READY_CHANGE_REASON_AUDIO_BECOMING_NOISY) {
                    currentKey()?.let { report(BackendReport.Paused(BackendReportPausedInner(it, position()))) }
                }
            }

            override fun onPlaybackSuppressionReasonChanged(playbackSuppressionReason: Int) {
                if (playbackSuppressionReason == Player.PLAYBACK_SUPPRESSION_REASON_TRANSIENT_AUDIO_FOCUS_LOSS) {
                    report(BackendReport.AudioFocusLost(BackendReportAudioFocusLostInner(transient = true)))
                }
            }

            override fun onMediaItemTransition(mediaItem: MediaItem?, reason: Int) {
                if (reason == Player.MEDIA_ITEM_TRANSITION_REASON_AUTO && mediaItem != null) onAutoTransition(mediaItem)
            }

            override fun onPositionDiscontinuity(oldPosition: Player.PositionInfo, newPosition: Player.PositionInfo, reason: Int) {
                if (reason == Player.DISCONTINUITY_REASON_SEEK || reason == Player.DISCONTINUITY_REASON_SEEK_ADJUSTMENT) {
                    currentKey()?.let { report(BackendReport.Position(BackendReportPositionInner(it, newPosition.positionMs.coerceAtLeast(0).toUInt()))) }
                }
            }

            override fun onPlayerError(error: PlaybackException) {
                // No key at all (error after Stop): the core could not correlate the report, skip it.
                val key = errorKey() ?: return
                val message = error.errorCodeName + ": " + (error.message ?: "")
                // Any player error leaves ExoPlayer idle: "non-fatal" only holds if we bring it back.
                val delayMs = if (isRecoverable(error.errorCode)) recoveryDelayMs(recoveryAttempts) else null
                if (delayMs == null) {
                    recoveryAttempts = 0
                    report(BackendReport.Error(BackendReportErrorInner(key, message, true)))
                    return
                }
                recoveryAttempts++
                report(BackendReport.Error(BackendReportErrorInner(key, message, false)))
                report(BackendReport.Buffering(BackendReportBufferingInner(key, true)))
                recoveryJob?.cancel()
                recoveryJob = scope.launch {
                    delay(delayMs)
                    if (player.playerError != null && currentKey() == key) {
                        Log.i(TAG, "retrying $key after ${error.errorCodeName} (attempt $recoveryAttempts)")
                        player.prepare()
                    }
                }
            }
        })
    }

    fun handle(command: BackendCommand) {
        when (command) {
            is BackendCommand.Load -> {
                val d = command.data
                val sources = listOfNotNull(mediaSource(d.source), d.next?.let(::mediaSource))
                lastLoadedKey = d.source.key
                cancelRecovery()
                player.setMediaSources(sources, 0, d.position_ms.toLong())
                applyGain(d.source.gainDb)
                player.playWhenReady = d.play
                player.prepare()
            }
            is BackendCommand.SetNext -> {
                // With no current item (after Stop) a follow-up would become the current item and
                // later play under the wrong key; the next `Load` carries its own `next`.
                if (player.mediaItemCount == 0) return
                while (player.mediaItemCount > 1) player.removeMediaItem(player.mediaItemCount - 1)
                command.data.next?.let { player.addMediaSource(mediaSource(it)) }
            }
            BackendCommand.Play -> {
                if (player.playbackState == Player.STATE_IDLE) player.prepare()
                if (player.playbackState == Player.STATE_ENDED) player.seekTo(0)
                player.play()
            }
            BackendCommand.Pause -> player.pause()
            BackendCommand.Stop -> {
                stopPositionLoop()
                cancelRecovery()
                lastLoadedKey = null
                player.stop()
                player.clearMediaItems()
            }
            is BackendCommand.Seek -> player.seekTo(command.data.position_ms.toLong())
            is BackendCommand.SetVolume -> {
                masterVolume = command.data.volume.coerceIn(0.0, 1.0)
                applyGain(currentGainDb)
            }
            is BackendCommand.PreBuffer -> {
                val p = preBufferPlayer ?: ExoPlayer.Builder(context)
                    .setAudioAttributes(AudioAttributes.Builder().setUsage(C.USAGE_MEDIA).setContentType(C.AUDIO_CONTENT_TYPE_MUSIC).build(), false)
                    .build().also { pb ->
                        pb.addListener(object : Player.Listener {
                            override fun onPlaybackStateChanged(playbackState: Int) {
                                if (playbackState == Player.STATE_READY) preBufferKey?.let { report(BackendReport.PreBufferReady(BackendReportPreBufferReadyInner(it))) }
                            }
                        })
                        preBufferPlayer = pb
                    }
                preBufferKey = command.data.source.key
                p.setMediaSource(mediaSource(command.data.source), command.data.position_ms.toLong())
                p.playWhenReady = false
                p.prepare()
            }
            BackendCommand.DiscardPreBuffer -> {
                preBufferKey = null
                preBufferPlayer?.let { it.stop(); it.clearMediaItems() }
            }
            is BackendCommand.SetGapless -> {
                // ExoPlayer joins playlist items gaplessly whenever encoder delay/padding metadata allows;
                // "off" is modelled by not preloading a follow-up (the core stops sending `next`).
                if (!command.data.enabled) while (player.mediaItemCount > 1) player.removeMediaItem(player.mediaItemCount - 1)
            }
        }
    }

    /** Current position, or the pre-buffered one when the main player has nothing. */
    fun position(): UInt = player.currentPosition.coerceAtLeast(0).toUInt()

    /**
     * ExoPlayer moved on to the preloaded follow-up by itself. Reports the played item's `Ended`
     * first, then the transition, and drops the played item so the playlist stays current(+next).
     */
    internal fun onAutoTransition(mediaItem: MediaItem) {
        val played = if (player.currentMediaItemIndex > 0) player.getMediaItemAt(0).mediaId else null
        if (player.currentMediaItemIndex > 0) player.removeMediaItem(0)
        applyGain(gainFor(mediaItem))
        if (played != null && played != mediaItem.mediaId) report(BackendReport.Ended(BackendReportEndedInner(played)))
        report(BackendReport.TransitionedToNext(BackendReportTransitionedToNextInner(mediaItem.mediaId)))
        report(BackendReport.Position(BackendReportPositionInner(mediaItem.mediaId, 0u)))
    }

    private fun cancelRecovery() {
        recoveryJob?.cancel()
        recoveryJob = null
        recoveryAttempts = 0
    }

    fun release() {
        stopPositionLoop()
        cancelRecovery()
        preBufferPlayer?.release()
        preBufferPlayer = null
        player.release()
    }

    private fun report(report: BackendReport) = dispatch(Commands.backendReport(report))

    private fun startPositionLoop() {
        if (positionJob?.isActive == true) return
        positionJob = scope.launch {
            while (isActive && player.isPlaying) {
                currentKey()?.let { report(BackendReport.Position(BackendReportPositionInner(it, position()))) }
                delay(POSITION_INTERVAL_MS)
            }
        }
    }

    private fun stopPositionLoop() {
        positionJob?.cancel()
        positionJob = null
    }

    private fun gainFor(item: MediaItem): Double = item.requestMetadata.extras?.getDouble("gainDb", 0.0) ?: 0.0

    private fun applyGain(gainDb: Double) {
        currentGainDb = gainDb
        val linear = 10.0.pow(gainDb / 20.0)
        player.volume = (linear * masterVolume).coerceIn(0.0, 1.0).toFloat()
    }

    private fun mediaSourceFactory(headers: Map<String, String> = emptyMap()): DefaultMediaSourceFactory =
        DefaultMediaSourceFactory(CoreStreamDataSourceFactory(context, streams, headers))
            .setLoadErrorHandlingPolicy(CoreStreamLoadErrorPolicy())

    private fun mediaSource(source: CoreMediaSource): MediaSource {
        val extras = android.os.Bundle().apply { putDouble("gainDb", source.gainDb) }
        val item = MediaItem.Builder()
            .setMediaId(source.key)
            .setUri(Uri.parse(source.url))
            .setMimeType(source.mimeType)
            .setRequestMetadata(MediaItem.RequestMetadata.Builder().setExtras(extras).build())
            .build()
        return mediaSourceFactory(source.headers).createMediaSource(item)
    }
}
