package app.hocket.playback

import android.content.Context
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.os.Process
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.datasource.HttpDataSource
import androidx.media3.exoplayer.DefaultLoadControl
import androidx.media3.exoplayer.DefaultRenderersFactory
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.analytics.AnalyticsListener
import androidx.media3.exoplayer.audio.AudioSink
import androidx.media3.exoplayer.audio.DefaultAudioSink
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import androidx.media3.exoplayer.source.MediaSource
import app.hocket.core.Commands
import app.hocket.core.CoreStreamException
import app.hocket.core.CoreStreams
import app.hocket.core.HocketJson
import app.hocket.core.api.AudioSettings
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
import app.hocket.core.api.BackendReportTransitionStartedInner
import app.hocket.core.api.BackendReportTransitionedToNextInner
import app.hocket.core.api.Command
import app.hocket.core.api.MediaSource as CoreMediaSource
import app.hocket.core.api.Query
import app.hocket.core.api.QueryResult
import app.hocket.core.api.QueueView
import app.hocket.core.api.TransitionPlan
import app.hocket.core.upcomingEntries
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.util.concurrent.Executors
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
 * - `gain_db` (ReplayGain, boost included), the in-app EQ and the limiter run in the core's DSP chain
 *   inside the player's audio sink ([MixProcessor], JNI), switching gain at the exact sample of a
 *   gapless join. `player.volume` is only `masterVolume` (`SetVolume`, an amplitude gain: the sleep
 *   fade once the core has the system volume below, else the user volume on a dB scale too).
 *   Without the native library (JVM tests) the old approximation applies: `player.volume =
 *   10^(gain/20) * masterVolume`, clamped to 1.0.
 * - AutoMix (`docs/automix.md` → Android): an item with `mix_out_ms` is clipped there. When the next
 *   source carries `mix_in`, A's tail is decoded ([PcmDecoder]) once A is within a minute of the
 *   tail start, rendered at B's format in Rust and armed in [MixControl]; the processor sums it with
 *   B at B's first frame. `TransitionStarted` is reported when ExoPlayer moves on to B (the tail is
 *   heard), the handover (`Ended(A)`, `TransitionedToNext(B)`, `Position(B, p)`) when B's position
 *   reaches `handover_ms`. Until then reports name A at its tail position; after it B's position is
 *   mapped back by `incoming_from_ms − incoming_at_ms` (the processor plays the tail's lead-in ahead
 *   of B) until the next seek. A tail that isn't ready at A's clip point means B starts unmixed.
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
    /** Core queries (AutoMix: the mode, stored records, sources); null when no core answers. */
    private val query: suspend (Query) -> QueryResult? = { q -> CoreHost.current?.let { runCatching { it.query(q) }.getOrNull() } },
    /** The per-buffer DSP; null (JVM tests) keeps the volume-based gain and never mixes. */
    private val dsp: DspEngine? = NativeDsp.orNull,
    private val ffi: AutomixFfi = AutomixFfi.Native,
) {
    /** What a player that stopped playing while it still has an item is reporting. */
    internal enum class Stall { Paused, Buffering, Nothing }

    internal companion object {
        /** The native desktop engine's cadence (`audio/native/engine.rs`): receivers extrapolate between reports. */
        private const val POSITION_INTERVAL_MS = 1_000L
        /** How often the handover is checked against B's position during an overlap. */
        private const val HANDOVER_POLL_MS = 40L
        /** A's tail is decoded and rendered once A is this close to its tail start. */
        private const val PREPARE_AHEAD_MS = 60_000L
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

    private val main = Handler(Looper.getMainLooper())
    private val mixControl = MixControl()

    /** Which transition the playback thread started mixing; consumed at ExoPlayer's item transition. */
    private var mixHeard: MixControl.Armed? = null
    private val mixListener = object : MixListener {
        override fun onMixStarted(armed: MixControl.Armed) {
            main.post { mixHeard = armed }
        }

        override fun onMixSkipped(armed: MixControl.Armed, reason: String) {
            PlaybackLog.i("AutoMix: ${armed.from} -> ${armed.to} plays unmixed: $reason")
        }
    }
    private val processor: MixProcessor? = dsp?.let { MixProcessor(it, mixControl, mixListener) }

    private fun renderersFactory() = object : DefaultRenderersFactory(context) {
        override fun buildAudioSink(context: Context, enableFloatOutput: Boolean, enableAudioOutputPlaybackParams: Boolean): AudioSink? {
            val p = processor ?: return super.buildAudioSink(context, enableFloatOutput, enableAudioOutputPlaybackParams)
            // PCM path only: no float output, no offload (both are off by default), so every buffer
            // reaches the processor.
            return DefaultAudioSink.Builder(context).setAudioProcessors(arrayOf(p)).build()
        }
    }

    val player: ExoPlayer = ExoPlayer.Builder(context, renderersFactory())
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
    /** The sources in the playlist by key (AutoMix needs A's URL, gain and B's format). */
    private val sources = HashMap<String, CoreMediaSource>()

    /** A transition that is sounding (the tail started), until its handover. */
    private class Running(val armed: MixControl.Armed) {
        var handedOver = false
        /** A `SetNext` that came during the overlap, applied at the handover (`next` may be null). */
        var held: Held? = null
    }

    private class Held(val next: CoreMediaSource?)

    private var running: Running? = null
    private var handoverJob: Job? = null
    private var tailJob: Job? = null
    /** After a handover, B's true position is ExoPlayer's plus this, until the next seek. */
    private var offsetKey: String? = null
    private var offsetMs = 0L
    private var queueView: QueueView? = null
    private val tailDispatcher = Executors.newSingleThreadExecutor { r -> Thread(r, "automix-tail") }.asCoroutineDispatcher()
    private val analysisDispatcher = Executors.newSingleThreadExecutor { r ->
        Thread({ Process.setThreadPriority(Process.THREAD_PRIORITY_LOWEST); r.run() }, "automix-analysis")
    }.asCoroutineDispatcher()
    private val loudness = LoudnessRecorder(scope, query, dispatch, streams, io, analysisDispatcher, ffi)

    /**
     * Keys by media id, so reports name the queue key the core gave us. During a transition's
     * overlap, before the handover, that is still A, whatever ExoPlayer is on.
     */
    private fun currentKey(): String? = running?.takeIf { !it.handedOver }?.armed?.from ?: player.currentMediaItem?.mediaId

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
        settleTransitionForRestart()
        player.prepare()
    }

    /**
     * A mixed transition can't survive a restart of B's stream (`prepare()` after an error): the
     * sink's queued tail is gone, and ExoPlayer's position for B runs `incoming_at − incoming_from`
     * ahead of B's real one. Hand over if that hasn't happened and resume B where it really is.
     */
    private fun settleTransitionForRestart() {
        if (running == null && offsetKey == null) return
        if (running?.handedOver == false) handOver(reportPosition = false)
        val p = position().toLong()
        running = null
        offsetKey = null
        mixControl.cancelMix()
        player.seekTo(player.currentMediaItemIndex, p)
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
                resetTransition()
                sources.clear()
                val items = listOfNotNull(mediaSource(d.source), d.next?.let(::mediaSource))
                lastLoadedKey = d.source.key
                cancelRecovery()
                mixControl.items.keys.retainAll(sources.keys)
                player.setMediaSources(items, 0, d.position_ms.toLong())
                arm(d.source, d.next)
                refreshLocality()
                applyGain(d.source.gainDb)
                player.playWhenReady = d.play
                player.prepare()
                analyseWindow()
            }
            is BackendCommand.SetNext -> {
                // With no current item (after Stop) a follow-up would become the current item and
                // later play under the wrong key; the next `Load` carries its own `next`.
                if (player.mediaItemCount == 0) return
                // During the overlap the newest follow-up waits for the handover; the transition
                // that is sounding keeps its B.
                running?.takeIf { !it.handedOver }?.let { r ->
                    if (command.data.next?.key != r.armed.to) r.held = Held(command.data.next)
                    return
                }
                applySetNext(command.data.next)
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
                resetTransition()
                player.stop()
                player.clearMediaItems()
            }
            is BackendCommand.Seek -> seek(command.data.position_ms.toLong())
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
                if (!command.data.enabled && running == null) {
                    while (player.mediaItemCount > player.currentMediaItemIndex + 1) player.removeMediaItem(player.mediaItemCount - 1)
                    disarm()
                }
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
            BackendCommand.CompleteTransition -> completeTransition()
        }
    }

    /**
     * A seek cancels a transition. Before the handover A is "now playing": ExoPlayer goes back to
     * A, B stays next and its tail stays armed (it mixes again from the tail start) unless the
     * target is already past it. After it, B seeks and ExoPlayer's positions are true again.
     */
    private fun seek(positionMs: Long) {
        val r = running
        offsetKey = null
        mixControl.cancelMix()
        if (r != null && !r.handedOver) {
            running = null
            handoverJob?.cancel()
            val index = (0 until player.mediaItemCount).firstOrNull { player.getMediaItemAt(it).mediaId == r.armed.from }
            if (positionMs >= r.armed.tailStartMs && mixControl.armed === r.armed) disarm()
            if (index != null) player.seekTo(index, positionMs) else player.seekTo(positionMs)
            r.held?.let { applySetNext(it.next) }
            applyGain(currentGainDb)
            return
        }
        player.seekTo(positionMs)
    }

    private fun resetTransition() {
        running = null
        handoverJob?.cancel()
        offsetKey = null
        mixHeard = null
        disarm()
        mixControl.cancelMix()
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

    /**
     * The reported position: before a transition's handover A's (its tail start plus the tail
     * played, at the tail's average speed); after it B's true position ([offsetMs]).
     */
    fun position(): UInt {
        val exo = player.currentPosition.coerceAtLeast(0)
        running?.takeIf { !it.handedOver }?.let { r ->
            val a = r.armed
            val played = exo.coerceAtMost(a.tailLenMs)
            val source = if (a.tailLenMs > 0) played * a.tailSourceMs / a.tailLenMs else played
            return (a.tailStartMs + source).toUInt()
        }
        if (offsetKey != null && offsetKey == player.currentMediaItem?.mediaId) return (exo + offsetMs).coerceAtLeast(0).toUInt()
        return exo.toUInt()
    }

    /**
     * ExoPlayer moved on to the preloaded follow-up by itself. Reports the played item's `Ended`
     * first, then the transition, and drops the played item so the playlist stays current(+next).
     */
    internal fun onAutoTransition(mediaItem: MediaItem) {
        val played = if (player.currentMediaItemIndex > 0) player.getMediaItemAt(0).mediaId else null
        val heard = mixHeard
        mixHeard = null
        offsetKey = null
        // The processor's post can land after a reset (a `Load`, a seek): only the transition still
        // armed counts.
        if (heard != null && heard === mixControl.armed && heard.to == mediaItem.mediaId && heard.from == played) {
            startTransition(heard)
            return
        }
        if (player.currentMediaItemIndex > 0) player.removeMediaItem(0)
        applyGain(gainFor(mediaItem))
        // The follow-up of the item now playing arrives with the core's next `SetNext`.
        disarm()
        analyseWindow()
        if (played != null && played != mediaItem.mediaId) report(BackendReport.Ended(BackendReportEndedInner(played)))
        report(BackendReport.TransitionedToNext(BackendReportTransitionedToNextInner(mediaItem.mediaId)))
        report(BackendReport.Position(BackendReportPositionInner(mediaItem.mediaId, 0u)))
    }

    // -- AutoMix ------------------------------------------------------------------------------

    /** ExoPlayer moved on to B and the processor is mixing: the tail is heard. */
    private fun startTransition(armed: MixControl.Armed) {
        val r = Running(armed)
        running = r
        offsetKey = armed.to
        offsetMs = armed.incomingFromMs - armed.incomingAtMs
        applyGain(currentGainDb)
        PlaybackLog.i("AutoMix: transition ${armed.from} -> ${armed.to} started")
        report(
            BackendReport.TransitionStarted(
                BackendReportTransitionStartedInner(armed.from, armed.to, armed.tailLenMs.toUInt(), armed.incomingAtMs.toUInt(), armed.handoverMs.toUInt()),
            ),
        )
        handoverJob?.cancel()
        handoverJob = scope.launch {
            while (isActive && running === r && !r.handedOver) {
                if (player.currentMediaItem?.mediaId == armed.to && player.currentPosition >= armed.handoverMs) {
                    handOver(reportPosition = true)
                    break
                }
                delay(HANDOVER_POLL_MS)
            }
        }
    }

    /** The handover: `Ended(A)`, `TransitionedToNext(B)` (and B's position); A leaves the playlist. */
    private fun handOver(reportPosition: Boolean) {
        val r = running?.takeIf { !it.handedOver } ?: return
        r.handedOver = true
        running = null
        handoverJob?.cancel()
        val a = r.armed
        val index = player.currentMediaItemIndex
        (0 until index).firstOrNull { player.getMediaItemAt(it).mediaId == a.from }?.let(player::removeMediaItem)
        if (mixControl.armed === a) mixControl.armed = null
        report(BackendReport.Ended(BackendReportEndedInner(a.from)))
        report(BackendReport.TransitionedToNext(BackendReportTransitionedToNextInner(a.to)))
        if (reportPosition) report(BackendReport.Position(BackendReportPositionInner(a.to, position())))
        r.held?.let { applySetNext(it.next) }
        refreshLocality()
        analyseWindow()
    }

    /**
     * `complete_transition`: hand over now if that hasn't happened and play B alone from where it
     * is (a seek drops the tail still queued in the sink).
     */
    private fun completeTransition() {
        val r = running
        if (r == null && offsetKey == null) {
            mixControl.cancelMix()
            return
        }
        if (r != null) handOver(reportPosition = false)
        val p = position().toLong()
        offsetKey = null
        mixControl.cancelMix()
        player.seekTo(player.currentMediaItemIndex, p)
    }

    /** `SetNext`: replace what follows the current item and arm its transition. */
    private fun applySetNext(next: CoreMediaSource?) {
        if (player.mediaItemCount == 0) return
        while (player.mediaItemCount > player.currentMediaItemIndex + 1) player.removeMediaItem(player.mediaItemCount - 1)
        next?.let { player.addMediaSource(mediaSource(it)) }
        arm(player.currentMediaItem?.mediaId?.let(sources::get), next)
        refreshLocality()
        analyseWindow()
    }

    /**
     * Arm the transition from [current] into [next] (its `mix_in`), keeping a tail already rendered
     * for the same pair and plan; anything else disarms. The tail is rendered once A is within
     * [PREPARE_AHEAD_MS] of the tail start.
     */
    private fun arm(current: CoreMediaSource?, next: CoreMediaSource?) {
        val plan = next?.mixIn
        if (current == null || next == null || plan == null || processor == null) {
            disarm()
            return
        }
        val json = HocketJson.json.encodeToString(TransitionPlan.serializer(), plan)
        mixControl.armed?.let { if (it.from == current.key && it.to == next.key && it.planJson == json) return }
        disarm()
        val armed = MixControl.Armed(
            from = current.key, to = next.key, planJson = json,
            incomingAtMs = plan.incomingAtMs.toLong(), incomingFromMs = plan.incomingFromMs.toLong(),
            tailLenMs = plan.tailLenMs.toLong(), handoverMs = plan.handoverMs.toLong(),
            tailStartMs = plan.tailStartMs.toLong(), tailSourceMs = plan.tailSourceMs.toLong(),
        )
        mixControl.armed = armed
        tailJob = scope.launch {
            while (isActive) {
                val onA = player.currentMediaItem?.mediaId == current.key
                if (onA && player.currentPosition >= armed.tailStartMs - PREPARE_AHEAD_MS) break
                delay(1_000)
            }
            val job = coroutineContext[Job]
            // The settings as they are now: an EQ change since arming applies to the tail too.
            val tail = withContext(tailDispatcher) { renderTail(current, next, armed, mixControl.audioSettingsJson) { job?.isActive != true } }
            if (tail != null && mixControl.armed === armed) armed.tail = tail
        }
    }

    private fun disarm() {
        tailJob?.cancel()
        tailJob = null
        mixControl.armed = null
    }

    /** Decode A's tail, then DSP, resample to B's format and render it in Rust. Null on failure (logged). */
    private fun renderTail(a: CoreMediaSource, b: CoreMediaSource, armed: MixControl.Armed, settings: String?, cancelled: () -> Boolean): MixControl.Tail? = try {
        val started = System.nanoTime()
        val decoder = PcmDecoder(streams)
        val pcm = decoder.decode(a.url, a.headers, armed.tailStartMs, armed.tailSourceMs + 50, cancelled)
        val (rate, channels) = decoder.probe(b.url, b.headers)
        val samples = ffi.prepareTail(armed.planJson, pcm, channels, rate, settings, a.gainDb)
        PlaybackLog.i("AutoMix: tail ${a.key} -> ${b.key} ready (${pcm.durationMs} ms of A at ${pcm.rate} Hz -> ${armed.tailLenMs} ms at $rate Hz) in ${(System.nanoTime() - started) / 1_000_000} ms")
        MixControl.Tail(samples, rate, channels)
    } catch (e: InterruptedException) {
        null
    } catch (e: Throwable) {
        PlaybackLog.w("AutoMix: tail ${a.key} -> ${b.key} failed: ${e.javaClass.simpleName}: ${e.message}")
        null
    }

    /** The core's audio settings: the in-app EQ (and normalisation) for the DSP chain, live. */
    fun onAudioSettings(settings: AudioSettings) {
        mixControl.audioSettingsJson = HocketJson.json.encodeToString(AudioSettings.serializer(), settings)
    }

    /** Loudness-only records for the window (current, next, next-next) when AutoMix is on. */
    private fun analyseWindow() {
        if (dsp == null) return
        val current = player.currentMediaItem?.mediaId?.let(sources::get)
        val next = (player.currentMediaItemIndex + 1).takeIf { it < player.mediaItemCount }?.let { sources[player.getMediaItemAt(it).mediaId] }
        val candidates = ArrayList<LoudnessRecorder.Candidate>()
        listOfNotNull(current, next).forEach { candidates += LoudnessRecorder.Candidate(it.track.id, it.track.durationMs.toLong(), it) }
        queueView?.upcomingEntries?.getOrNull(1)?.let { candidates += LoudnessRecorder.Candidate(it.track.id, it.track.durationMs.toLong(), null) }
        loudness.onWindow(candidates.distinctBy { it.trackId })
    }

    private fun cancelRecovery() {
        recoveryJob?.cancel()
        recoveryJob = null
        recoveryKey = null
        recoveryAttempts = 0
    }

    fun release() {
        resetTransition()
        loudness.cancel()
        tailDispatcher.close()
        analysisDispatcher.close()
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

    /**
     * Reports reach the core only while this is the backend playing: while casting the core's
     * commands go to the Cast renderer, and this player, stopped, must not say anything about the
     * items it played (a late `Paused` would pause the cast). Previews play here either way, so
     * theirs always go through.
     */
    var active = true
        set(value) {
            if (field == value) return
            field = value
            if (value) reportSystemVolume()
        }

    private fun report(report: BackendReport) {
        if (active || isPreview(report)) dispatch(Commands.backendReport(report))
    }

    private fun isPreview(report: BackendReport): Boolean = when (report) {
        is BackendReport.PreviewReady, is BackendReport.PreviewProgress, BackendReport.PreviewEnded,
        BackendReport.PreviewFailed, BackendReport.PreviewInterrupted -> true
        else -> false
    }

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
    fun onQueueChanged(queue: QueueView? = null) {
        if (queue != null) queueView = queue
        refreshLocality()
        analyseWindow()
    }

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
        // The DSP chain applies the track gain (boost included) at the right sample.
        val linear = if (processor != null) 1.0 else 10.0.pow(gainDb / 20.0)
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
        sources[source.key] = source
        mixControl.items[source.key] = MixControl.Item(source.gainDb, source.mixOutMs?.toLong())
        val builder = MediaItem.Builder()
            .setMediaId(source.key)
            .setUri(Uri.parse(source.url))
            .setMimeType(source.mimeType)
            .setRequestMetadata(MediaItem.RequestMetadata.Builder().setExtras(extras).build())
        // AutoMix commits early: the tail replaces everything from the mix-out point on.
        if (processor != null) {
            source.mixOutMs?.let { builder.setClippingConfiguration(MediaItem.ClippingConfiguration.Builder().setEndPositionMs(it.toLong()).build()) }
        }
        val item = builder.build()
        return mediaSourceFactory(source.headers).createMediaSource(item)
    }
}
