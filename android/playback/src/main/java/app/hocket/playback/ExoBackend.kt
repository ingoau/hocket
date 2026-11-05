package app.hocket.playback

import android.content.Context
import android.net.Uri
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.datasource.HttpDataSource
import androidx.media3.exoplayer.DefaultLoadControl
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.analytics.AnalyticsListener
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import androidx.media3.exoplayer.source.MediaSource
import app.hocket.core.Commands
import app.hocket.core.CoreStreamException
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
import app.hocket.core.api.BackendReportPreviewProgressInner
import app.hocket.core.api.BackendReportPreviewReadyInner
import app.hocket.core.api.BackendReportReadyInner
import app.hocket.core.api.BackendReportSystemVolumeInner
import app.hocket.core.api.BackendReportTransitionedToNextInner
import app.hocket.core.api.Command
import app.hocket.core.api.MediaSource as CoreMediaSource
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlin.math.pow
import kotlin.math.roundToInt

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
 *   attenuation is exact. `masterVolume` (`SetVolume`) is an amplitude gain: only the sleep fade
 *   once the core has the system volume below, else the user volume on a dB scale too.
 * - The phone's volume is the system music volume, with no second volume in the app: the player's
 *   device volume (the music stream) is reported as `SystemVolume` at start and on every change
 *   (keys, the system panel, another app, a headset), and `SetSystemVolume` sets it. A device whose
 *   volume is fixed reports nothing, and the core keeps its own volume instead.
 * - Audio focus and becoming-noisy are handled by ExoPlayer; focus loss is reported as
 *   `AudioFocusLost` (transient when Media3 reports a suppression reason instead of a pause: ExoPlayer
 *   then resumes by itself when focus returns and reports `Playing`).
 * - The main player's audio session is announced to equalizer apps once it first plays, and closed
 *   on release ([AudioEffectSession]).
 * - The player holds a wake lock while playing, and a Wi-Fi lock too ([C.WAKE_MODE_NETWORK]) while
 *   the current or the next item is streamed: without them a stream stalls once the screen is off and
 *   the CPU or Wi-Fi radio sleeps. When both are on the device (a download, a `file:` URL, or a
 *   core stream the core has whole at the profile it serves: `MediaSource.local` when resolved, then
 *   the core's own answer, [CoreStreams.streamLocal], asked again on every load, transition and queue
 *   change, which is how the core's burst fetch finishing a track, or an eviction, shows up; a "no"
 *   wins over the resolve-time flag) the wake lock alone does ([C.WAKE_MODE_LOCAL], [wakeModeFor]):
 *   the high-performance Wi-Fi lock keeps the Wi-Fi radio out of power save, which costs battery for
 *   nothing when no byte comes over the network.
 * - `Paused` is reported only when the player no longer means to play (`playWhenReady` false: a
 *   pause, becoming noisy, a lasting focus loss). A stall with `playWhenReady` still set (an empty
 *   buffer, a load being retried, an error being recovered from) is `Buffering`, never `Paused`: the
 *   core would otherwise show the item paused, and the service would count the player as idle and
 *   could stop itself under a playback that was only rebuffering.
 * - Errors a later `prepare()` can fix ([isRecoverable]: the connection, a timeout, a 5xx from the
 *   server, a stream handle the core closed, a stuck player) leave ExoPlayer idle with an error, so
 *   they are retried here with backoff ([recoveryDelayMs]; `prepare()` resumes at the same position)
 *   and reported as non-fatal. While the device is offline ([onConnectivityChanged]) no attempt is
 *   spent: the retry waits, and the network coming back retries at once with a fresh schedule. Only
 *   when the attempts run out is the error fatal and the core's own retry/skip takes over.
 */
class ExoBackend(
    private val context: Context,
    private val scope: CoroutineScope,
    private val dispatch: (Command) -> Unit,
    /** The running core's stream reader, read on each open (a restarted service has a new core). */
    private val streams: () -> CoreStreams? = { CoreHost.current as? CoreStreams },
    /** Where [CoreStreams.streamLocal] (a blocking index lookup) runs. */
    private val io: CoroutineDispatcher = Dispatchers.IO,
) {
    /** What a player that stopped playing while it still has an item is reporting. */
    internal enum class Stall { Paused, Buffering, Nothing }

    internal companion object {
        /** The native desktop engine's cadence (`audio/native/engine.rs`): receivers extrapolate between reports. */
        private const val POSITION_INTERVAL_MS = 1_000L
        private val RECOVERY_DELAYS_MS = longArrayOf(1_000, 2_000, 4_000, 8_000, 15_000, 30_000)
        /** Outputs that count as headphones for headphone handoff. */
        private val HEADPHONE_KINDS = setOf(AudioOutputs.Kind.Bluetooth, AudioOutputs.Kind.Wired, AudioOutputs.Kind.Usb)

        /** Backoff before recovery attempt [attempt] (0-based) of a network error, or null when out of attempts. */
        internal fun recoveryDelayMs(attempt: Int): Long? = RECOVERY_DELAYS_MS.getOrNull(attempt)

        /** Errors a later `prepare()` can fix by their code alone: the connection, not the media. */
        internal fun isRecoverable(errorCode: Int): Boolean =
            errorCode == PlaybackException.ERROR_CODE_IO_NETWORK_CONNECTION_FAILED ||
                errorCode == PlaybackException.ERROR_CODE_IO_NETWORK_CONNECTION_TIMEOUT ||
                errorCode == PlaybackException.ERROR_CODE_TIMEOUT

        /**
         * Whether a later `prepare()` can fix [error]: a network failure or timeout, a stuck player
         * (`ERROR_CODE_TIMEOUT`), a 5xx from the server (the core's stream status or a direct HTTP
         * source), or a core stream handle that went away under the player (closed as idle, too many
         * open, unknown): a re-open serves it again. A 4xx, an unknown token (the core re-resolves), a
         * core that shut down, and anything about the media itself are not.
         */
        internal fun isRecoverable(error: PlaybackException): Boolean {
            if (isRecoverable(error.errorCode)) return true
            var t: Throwable? = error
            while (t != null) {
                when (t) {
                    is CoreStreamException -> return when (t.kind) {
                        CoreStreamException.Kind.Network, CoreStreamException.Kind.Closed, CoreStreamException.Kind.UnknownHandle,
                        CoreStreamException.Kind.TooManyHandles, CoreStreamException.Kind.NoServer -> true
                        CoreStreamException.Kind.Status -> (t.httpStatus ?: 0) >= 500
                        else -> false
                    }
                    is HttpDataSource.InvalidResponseCodeException -> return t.responseCode >= 500
                    is HttpDataSource.CleartextNotPermittedException -> return false
                    is HttpDataSource.HttpDataSourceException -> return true
                }
                t = t.cause
            }
            return false
        }

        /**
         * [Player.Listener.onIsPlayingChanged] `false`: [Stall.Paused] when the player no longer
         * means to play, [Stall.Buffering] while it does but has nothing to play yet; idle (an error,
         * a stop) and ended are reported by their own callbacks.
         */
        /**
         * The wake mode for the current item and the one after it (null: none) by whether each is on
         * the device: [C.WAKE_MODE_LOCAL] when every one is, else [C.WAKE_MODE_NETWORK].
         */
        internal fun wakeModeFor(current: Boolean, next: Boolean?): Int =
            if (current && next != false) C.WAKE_MODE_LOCAL else C.WAKE_MODE_NETWORK

        internal fun stall(playWhenReady: Boolean, playbackState: Int): Stall = when {
            !playWhenReady -> if (playbackState == Player.STATE_IDLE || playbackState == Player.STATE_ENDED) Stall.Nothing else Stall.Paused
            playbackState == Player.STATE_BUFFERING -> Stall.Buffering
            else -> Stall.Nothing
        }
    }

    val player: ExoPlayer = ExoPlayer.Builder(context)
        .setAudioAttributes(AudioAttributes.Builder().setUsage(C.USAGE_MEDIA).setContentType(C.AUDIO_CONTENT_TYPE_MUSIC).build(), true)
        .setHandleAudioBecomingNoisy(true)
        .setWakeMode(C.WAKE_MODE_NETWORK)
        .setLoadControl(DefaultLoadControl.Builder().setBufferDurationsMs(30_000, 120_000, 2_500, 5_000).build())
        .setMediaSourceFactory(mediaSourceFactory())
        .setDeviceVolumeControlEnabled(true)
        .build()

    /** The preview player. It asks for audio focus; the main player does not treat that as another app. */
    private var previewPlayer: ExoPlayer? = null
    private var preferredDevice: android.media.AudioDeviceInfo? = null

    init {
        // The output picked in the Connect sheet (speaker, earphones, ...) for this app's playback only.
        AudioOutputs.start(context)
        AudioOutputs.attach { device ->
            preferredDevice = device
            player.setPreferredAudioDevice(device)
            previewPlayer?.setPreferredAudioDevice(device)
        }
        watchHeadphones()
    }

    /**
     * Headphone handoff (beta): tells the core when headphones (Bluetooth, wired, USB) connect, so
     * it can pull playback here from a device that plays. The core checks the setting. The first
     * list is the baseline: headphones already connected when the app starts don't count.
     */
    private fun watchHeadphones() {
        scope.launch {
            var known: Set<String>? = null
            AudioOutputs.outputs.collect { state ->
                // Hearing aids are worn all day: connecting them says nothing about wanting the music here.
                val headphones = state.outputs.filter { it.kind in HEADPHONE_KINDS && it.form != AudioOutputs.Form.HearingAid }
                val keys = headphones.map { "${it.kind}:${it.name}" }.toSet()
                val before = known
                known = keys
                if (before == null) return@collect
                headphones.firstOrNull { "${it.kind}:${it.name}" !in before }?.let {
                    PlaybackLog.i("headphones connected: ${it.kind} \"${it.name}\"")
                    dispatch(Commands.headphonesConnected(it.name.ifEmpty { null }))
                }
            }
        }
    }

    private var preBufferPlayer: ExoPlayer? = null
    private var previewJob: Job? = null
    private var previewActive = false
    /** The main player's audio session as effect apps see it (the pre-buffer player stays silent). */
    private val effects = AudioEffectSession(context)
    private var preBufferKey: String? = null
    private var masterVolume = 1.0
    private var currentGainDb = 0.0
    private var positionJob: Job? = null
    private var recoveryJob: Job? = null
    /** Recovery attempts for the current error streak; reset once the player is ready again. */
    private var recoveryAttempts = 0
    /** The key a pending recovery is for. */
    private var recoveryKey: String? = null
    /** Connectivity as the service last reported it; a retry waits while this is false. */
    @Volatile
    private var online = true
    /** The wake mode set on the main player ([updateWakeMode]). */
    internal var wakeMode = C.WAKE_MODE_NETWORK
        private set
    /** What the core last said of core-stream URLs in the playlist: on the device or not ([refreshLocality]). */
    private val streamLocal = HashMap<String, Boolean>()
    private var localityJob: Job? = null
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
                        cancelRecovery()
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
                    effects.open(player.audioSessionId)
                    report(BackendReport.Playing(BackendReportPlayingInner(key, position())))
                    startPositionLoop()
                } else {
                    stopPositionLoop()
                    when (stall(player.playWhenReady, player.playbackState)) {
                        Stall.Paused -> report(BackendReport.Paused(BackendReportPausedInner(key, position())))
                        Stall.Buffering -> report(BackendReport.Buffering(BackendReportBufferingInner(key, true)))
                        Stall.Nothing -> Unit
                    }
                }
            }

            override fun onPlayWhenReadyChanged(playWhenReady: Boolean, reason: Int) {
                // Who the change came from: the core's own play and pause are "requested"; a pause
                // from the platform (focus, a headset leaving) is the one that explains itself here.
                val why = PlaybackLog.playWhenReadyReason(reason)
                if (reason == Player.PLAY_WHEN_READY_CHANGE_REASON_USER_REQUEST) {
                    PlaybackLog.d("player ${if (playWhenReady) "playing" else "pausing"}: $why")
                } else {
                    PlaybackLog.i("player ${if (playWhenReady) "playing" else "pausing"}: $why")
                }
                if (playWhenReady) return
                if (reason == Player.PLAY_WHEN_READY_CHANGE_REASON_AUDIO_FOCUS_LOSS && !previewActive) {
                    report(BackendReport.AudioFocusLost(BackendReportAudioFocusLostInner(transient = false)))
                }
                // The player stopped meaning to play, whoever asked (a pause while buffering never
                // reaches onIsPlayingChanged, so this is the only place that sees it).
                if (player.playbackState == Player.STATE_BUFFERING || player.playbackState == Player.STATE_READY) {
                    currentKey()?.let { report(BackendReport.Paused(BackendReportPausedInner(it, position()))) }
                }
            }

            override fun onPlaybackSuppressionReasonChanged(playbackSuppressionReason: Int) {
                PlaybackLog.i("playback suppression: ${PlaybackLog.suppressionReason(playbackSuppressionReason)}")
                if (playbackSuppressionReason == Player.PLAYBACK_SUPPRESSION_REASON_TRANSIENT_AUDIO_FOCUS_LOSS && !previewActive) {
                    report(BackendReport.AudioFocusLost(BackendReportAudioFocusLostInner(transient = true)))
                }
            }

            override fun onAudioSessionIdChanged(audioSessionId: Int) = effects.onSessionIdChanged(audioSessionId)

            override fun onDeviceVolumeChanged(volume: Int, muted: Boolean) = reportSystemVolume()

            // The stream's range arrives here once the player has read it.
            override fun onDeviceInfoChanged(deviceInfo: androidx.media3.common.DeviceInfo) = reportSystemVolume()

            override fun onMediaItemTransition(mediaItem: MediaItem?, reason: Int) {
                if (reason == Player.MEDIA_ITEM_TRANSITION_REASON_AUTO && mediaItem != null) onAutoTransition(mediaItem)
                refreshLocality()
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
                val delayMs = if (isRecoverable(error)) recoveryDelayMs(recoveryAttempts) else null
                if (delayMs == null) {
                    cancelRecovery()
                    report(BackendReport.Error(BackendReportErrorInner(key, message, true)))
                    return
                }
                recoveryAttempts++
                PlaybackLog.w("playback error on $key: ${error.errorCodeName}; retrying in ${delayMs}ms (attempt $recoveryAttempts)")
                report(BackendReport.Error(BackendReportErrorInner(key, message, false)))
                report(BackendReport.Buffering(BackendReportBufferingInner(key, true)))
                scheduleRecovery(key, delayMs)
            }
        })
        player.addAnalyticsListener(object : AnalyticsListener {
            override fun onAudioPositionAdvancing(eventTime: AnalyticsListener.EventTime, playoutStartSystemTimeMs: Long) = onAudioHeard()
        })
        // The core mirrors the music volume from here on.
        reportSystemVolume()
    }

    private fun scheduleRecovery(key: String, delayMs: Long) {
        recoveryJob?.cancel()
        recoveryKey = key
        recoveryJob = scope.launch {
            delay(delayMs)
            attemptRecovery(key)
        }
    }

    /**
     * Re-prepare after a recoverable error, if the error still stands and the item is still the one
     * that failed. Offline, the attempt is not spent: it waits for [onConnectivityChanged], with the
     * longest delay as a safety net should that never come.
     */
    private fun attemptRecovery(key: String) {
        if (player.playerError == null || currentKey() != key) {
            recoveryKey = null
            return
        }
        if (!online) {
            PlaybackLog.i("offline; waiting for connectivity before retrying $key")
            scheduleRecovery(key, RECOVERY_DELAYS_MS.last())
            return
        }
        PlaybackLog.i("retrying $key (attempt $recoveryAttempts)")
        player.prepare()
    }

    /**
     * Connectivity as the service sees it. Coming back online while a recovery is pending retries at
     * once, with the attempts reset (the failures so far were the old network's).
     */
    fun onConnectivityChanged(online: Boolean) {
        this.online = online
        if (!online) return
        val key = recoveryKey ?: return
        if (recoveryJob?.isActive != true) return
        recoveryJob?.cancel()
        recoveryAttempts = 0
        attemptRecovery(key)
    }

    /**
     * True while the player still means to play something: playing, buffering, or recovering from
     * an error. The service must not stop itself (releasing the player) on such a player, whatever
     * the media session says about it.
     */
    fun isBusy(): Boolean =
        previewActive ||
            recoveryJob?.isActive == true ||
            (player.playWhenReady && (player.playbackState == Player.STATE_BUFFERING || player.playbackState == Player.STATE_READY))

    fun handle(command: BackendCommand) {
        when (command) {
            is BackendCommand.Load -> {
                val d = command.data
                val sources = listOfNotNull(mediaSource(d.source), d.next?.let(::mediaSource))
                lastLoadedKey = d.source.key
                cancelRecovery()
                player.setMediaSources(sources, 0, d.position_ms.toLong())
                refreshLocality()
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
                refreshLocality()
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
            is BackendCommand.SetSystemVolume -> {
                val info = player.deviceInfo
                if (info.maxVolume > info.minVolume) {
                    val steps = info.minVolume + (command.data.volume.coerceIn(0.0, 1.0) * (info.maxVolume - info.minVolume)).roundToInt()
                    if (steps != player.deviceVolume) player.setDeviceVolume(steps, 0)
                    if (command.data.muted != player.isDeviceMuted) player.setDeviceMuted(command.data.muted, 0)
                }
                // Always answer: a level that rounds to the step it is already at changes nothing,
                // and the core shows what the stream is, not what was asked.
                reportSystemVolume()
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
            is BackendCommand.PreviewLoad -> {
                // Set before the preview player takes focus, so the main player's focus loss is
                // ours and is not reported as another app interrupting.
                previewActive = true
                val p = ensurePreviewPlayer()
                val source = command.data.source
                p.setMediaSource(mediaSource(source), command.data.position_ms.toLong())
                val linear = 10.0.pow(source.gainDb / 20.0)
                p.volume = (linear * masterVolume).coerceIn(0.0, 1.0).toFloat()
                p.playWhenReady = true
                p.prepare()
            }
            BackendCommand.PreviewPlay -> previewPlayer?.play()
            BackendCommand.PreviewPause -> previewPlayer?.pause()
            is BackendCommand.PreviewSeek -> previewPlayer?.seekTo(command.data.position_ms.toLong())
            BackendCommand.PreviewStop -> stopPreviewPlayer()
        }
    }

    private fun ensurePreviewPlayer(): ExoPlayer = previewPlayer ?: ExoPlayer.Builder(context)
        .setAudioAttributes(AudioAttributes.Builder().setUsage(C.USAGE_MEDIA).setContentType(C.AUDIO_CONTENT_TYPE_MUSIC).build(), true)
        .setHandleAudioBecomingNoisy(true)
        .setMediaSourceFactory(mediaSourceFactory())
        .build()
        .also { p ->
            p.addListener(object : Player.Listener {
                override fun onPlaybackStateChanged(playbackState: Int) {
                    when (playbackState) {
                        Player.STATE_READY -> {
                            val duration = p.duration.takeIf { it != C.TIME_UNSET }?.toUInt()
                            report(BackendReport.PreviewReady(BackendReportPreviewReadyInner(duration)))
                        }
                        Player.STATE_ENDED -> {
                            stopPreviewLoop()
                            report(BackendReport.PreviewEnded)
                        }
                    }
                }

                override fun onPlayerError(error: PlaybackException) {
                    stopPreviewLoop()
                    report(BackendReport.PreviewFailed)
                }

                override fun onPlayWhenReadyChanged(playWhenReady: Boolean, reason: Int) {
                    // Another app taking focus, or headphones unplugged: the
                    // core must not resume main playback into that.
                    val interrupted = reason == Player.PLAY_WHEN_READY_CHANGE_REASON_AUDIO_FOCUS_LOSS ||
                        reason == Player.PLAY_WHEN_READY_CHANGE_REASON_AUDIO_BECOMING_NOISY
                    if (!playWhenReady && interrupted) {
                        report(BackendReport.PreviewInterrupted)
                    }
                }

                override fun onIsPlayingChanged(isPlaying: Boolean) {
                    if (isPlaying) startPreviewLoop() else stopPreviewLoop()
                    if (p.playbackState == Player.STATE_READY) {
                        report(BackendReport.PreviewProgress(BackendReportPreviewProgressInner(p.currentPosition.coerceAtLeast(0).toUInt(), isPlaying)))
                    }
                }
            })
            p.setPreferredAudioDevice(preferredDevice)
            previewPlayer = p
        }

    private fun stopPreviewPlayer() {
        stopPreviewLoop()
        previewPlayer?.let { it.stop(); it.clearMediaItems() }
        previewActive = false
    }

    private fun startPreviewLoop() {
        if (previewJob?.isActive == true) return
        val p = previewPlayer ?: return
        previewJob = scope.launch {
            while (isActive && p.isPlaying) {
                report(BackendReport.PreviewProgress(BackendReportPreviewProgressInner(p.currentPosition.coerceAtLeast(0).toUInt(), true)))
                delay(500)
            }
        }
    }

    private fun stopPreviewLoop() {
        previewJob?.cancel()
        previewJob = null
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
        recoveryKey = null
        recoveryAttempts = 0
    }

    fun release() {
        stopPositionLoop()
        cancelRecovery()
        preBufferPlayer?.release()
        preBufferPlayer = null
        stopPreviewLoop()
        previewPlayer?.release()
        previewPlayer = null
        previewActive = false
        effects.close()
        AudioOutputs.detach()
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

    /** The music stream's volume to the core; nothing on a device whose volume is fixed. */
    private fun reportSystemVolume() {
        val info = player.deviceInfo
        val range = info.maxVolume - info.minVolume
        if (range <= 0) return
        val volume = ((player.deviceVolume - info.minVolume).toDouble() / range).coerceIn(0.0, 1.0)
        report(BackendReport.SystemVolume(BackendReportSystemVolumeInner(volume, player.isDeviceMuted)))
    }

    /**
     * The sound is heard: ExoPlayer's `onAudioPositionAdvancing`, its first advance since a play or a
     * seek. ExoPlayer reports playing at once but holds its position until the first sample leaves the
     * output, which over Bluetooth is 150–300 ms later; reporting now re-stamps the core from when
     * sound began, so lyrics extrapolating from the stamp don't run ahead by the output latency.
     */
    internal fun onAudioHeard() {
        if (!player.isPlaying) return
        currentKey()?.let { report(BackendReport.Position(BackendReportPositionInner(it, position()))) }
    }

    private fun stopPositionLoop() {
        positionJob?.cancel()
        positionJob = null
    }

    private fun gainFor(item: MediaItem): Double = item.requestMetadata.extras?.getDouble("gainDb", 0.0) ?: 0.0

    /**
     * The core's queue changed, which it does whenever a queued track's cache state changes (the
     * burst fetch completed it, the budget evicted it): ask the core again what is local.
     */
    fun onQueueChanged() = refreshLocality()

    /** Whether playing [item] needs no network: a core stream by the core's latest answer, else the resolve-time flag. */
    private fun isLocal(item: MediaItem): Boolean {
        val url = item.localConfiguration?.uri?.toString()
        if (url != null && url.startsWith(CoreStreams.SCHEME + ":")) streamLocal[url]?.let { return it }
        return item.requestMetadata.extras?.getBoolean("local", false) == true
    }

    /** The current and the next item, when there are. */
    private fun playing(): List<MediaItem> {
        val index = player.currentMediaItemIndex
        if (index !in 0 until player.mediaItemCount) return emptyList()
        return listOfNotNull(player.getMediaItemAt(index), (index + 1).takeIf { it < player.mediaItemCount }?.let(player::getMediaItemAt))
    }

    /** Ask the core whether the core streams playing now and next are local, then set the wake mode. */
    private fun refreshLocality() {
        updateWakeMode()
        val urls = playing().mapNotNull { it.localConfiguration?.uri?.toString() }.filter { it.startsWith(CoreStreams.SCHEME + ":") }
        streamLocal.keys.retainAll(urls.toSet())
        val core = streams() ?: return
        if (urls.isEmpty()) return
        localityJob?.cancel()
        localityJob = scope.launch {
            val answers = withContext(io) { urls.associateWith { runCatching { core.streamLocal(it) }.getOrDefault(false) } }
            streamLocal.putAll(answers)
            updateWakeMode()
        }
    }

    /** The wake mode for what is playing and what follows ([wakeModeFor]); nothing loaded keeps the last one. */
    private fun updateWakeMode() {
        val index = player.currentMediaItemIndex
        if (player.mediaItemCount == 0 || index !in 0 until player.mediaItemCount) return
        val next = (index + 1).takeIf { it < player.mediaItemCount }?.let { isLocal(player.getMediaItemAt(it)) }
        val mode = wakeModeFor(isLocal(player.getMediaItemAt(index)), next)
        if (mode == wakeMode) return
        PlaybackLog.i("wake mode ${if (mode == C.WAKE_MODE_LOCAL) "local (no Wi-Fi lock)" else "network"}")
        wakeMode = mode
        player.setWakeMode(mode)
    }

    private fun applyGain(gainDb: Double) {
        currentGainDb = gainDb
        val linear = 10.0.pow(gainDb / 20.0)
        player.volume = (linear * masterVolume).coerceIn(0.0, 1.0).toFloat()
    }

    private fun mediaSourceFactory(headers: Map<String, String> = emptyMap()): DefaultMediaSourceFactory =
        DefaultMediaSourceFactory(CoreStreamDataSourceFactory(context, streams, headers))
            .setLoadErrorHandlingPolicy(CoreStreamLoadErrorPolicy())

    private fun mediaSource(source: CoreMediaSource): MediaSource {
        val local = source.local == true || source.url.startsWith("file:")
        val extras = android.os.Bundle().apply {
            putDouble("gainDb", source.gainDb)
            putBoolean("local", local)
        }
        val item = MediaItem.Builder()
            .setMediaId(source.key)
            .setUri(Uri.parse(source.url))
            .setMimeType(source.mimeType)
            .setRequestMetadata(MediaItem.RequestMetadata.Builder().setExtras(extras).build())
            .build()
        return mediaSourceFactory(source.headers).createMediaSource(item)
    }
}
