package app.hocket.playback

import app.hocket.core.Queries
import app.hocket.core.CoreStreams
import app.hocket.core.HocketJson
import app.hocket.core.api.AnalysisRecord
import app.hocket.core.api.Command
import app.hocket.core.api.CommandStoreAnalysisRecordInner
import app.hocket.core.api.MediaSource as CoreMediaSource
import app.hocket.core.api.Query
import app.hocket.core.api.QueryResult
import app.hocket.core.api.QueryTrackAnalysisInner
import app.hocket.core.api.TransitionMode
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * Loudness-only AutoMix records from the phone (`docs/automix.md` → Android). With the mode on
 * AutoMix, each window change (current, next, next-next) looks for a track the core has no usable
 * record for (`Query.TrackAnalysis`) whose file is on the device (a download or a complete cache
 * entry: nothing is fetched for this), decodes its first and last minute with `MediaCodec` on one
 * low-priority thread, builds the record in Rust and hands it to the core
 * (`Command.StoreAnalysisRecord`, which stores, replans and uploads it). One track at a time; each
 * tried once per session. A window that changes while a pass runs is kept (the newest) and runs next.
 */
internal class LoudnessRecorder(
    private val scope: CoroutineScope,
    private val query: suspend (Query) -> QueryResult?,
    private val dispatch: (Command) -> Unit,
    private val streams: () -> CoreStreams?,
    private val io: CoroutineDispatcher,
    /** The one thread decoding runs on (low priority). */
    private val worker: CoroutineDispatcher,
    private val ffi: AutomixFfi,
) {
    /** A window track: its source when the backend has it, else resolved through the core. */
    class Candidate(val trackId: String, val durationMs: Long, val source: CoreMediaSource?)

    private val tried = HashSet<String>()
    private var job: Job? = null
    /** The newest window that came while [job] ran; it runs next. */
    private var pending: List<Candidate>? = null

    fun onWindow(candidates: List<Candidate>) {
        if (candidates.isEmpty()) return
        if (job?.isActive == true) {
            pending = candidates
            return
        }
        job = scope.launch {
            var window: List<Candidate>? = candidates
            while (window != null && isActive) {
                record(window)
                window = pending
                pending = null
            }
        }
    }

    fun cancel() {
        job?.cancel()
        job = null
        pending = null
    }

    private suspend fun record(candidates: List<Candidate>) {
        val mode = (query(Query.Transitions) as? QueryResult.Transitions)?.data?.mode
        if (mode != TransitionMode.AutoMix) return
        for (c in candidates) {
            if (!currentCoroutineContext().isActive) return
            if (c.trackId in tried || c.durationMs <= 0) continue
            val stored = (query(Query.TrackAnalysis(QueryTrackAnalysisInner(c.trackId))) as? QueryResult.Analysis)?.data
            if (stored != null && ffi.usable(stored, c.durationMs)) continue
            val source = c.source ?: (query(Queries.mediaSource(c.trackId)) as? QueryResult.Source)?.data ?: continue
            if (!withContext(io) { isLocal(source) }) continue
            tried += c.trackId
            val record = withContext(worker) { analyse(source, c.durationMs) } ?: continue
            PlaybackLog.i("AutoMix: loudness record for ${c.trackId} (audible ${record.audibleStartMs}–${record.audibleEndMs} ms)")
            dispatch(Command.StoreAnalysisRecord(CommandStoreAnalysisRecordInner(c.trackId, record)))
        }
    }

    private fun isLocal(source: CoreMediaSource): Boolean = when {
        source.url.startsWith("file:") -> true
        source.url.startsWith(CoreStreams.SCHEME + ":") -> runCatching { streams()?.streamLocal(source.url) == true }.getOrDefault(false)
        else -> false
    }

    private fun analyse(source: CoreMediaSource, durationMs: Long): AnalysisRecord? = try {
        val decoder = PcmDecoder(streams)
        val head = decoder.decode(source.url, source.headers, 0, HEAD_TAIL_MS)
        val tailStart = (durationMs - HEAD_TAIL_MS).coerceAtLeast(0)
        val tail = decoder.decode(source.url, source.headers, tailStart, HEAD_TAIL_MS + 1_000)
        ffi.loudnessRecord(head, tail, tailStart, durationMs, source.transcoded)
    } catch (e: Throwable) {
        if (e is InterruptedException) throw e
        PlaybackLog.w("AutoMix: loudness analysis failed: ${e.javaClass.simpleName}: ${e.message}")
        null
    }

    private companion object {
        const val HEAD_TAIL_MS = 60_000L
    }
}

/** The one-shot UniFFI calls of AutoMix, behind a seam (no native library in JVM tests). */
interface AutomixFfi {
    fun usable(record: AnalysisRecord, durationMs: Long): Boolean
    fun loudnessRecord(head: PcmDecoder.Pcm, tail: PcmDecoder.Pcm, tailStartMs: Long, durationMs: Long, transcoded: Boolean): AnalysisRecord?
    /** The rendered tail as interleaved floats at [outRate] × [outChannels]. */
    fun prepareTail(planJson: String, pcm: PcmDecoder.Pcm, outChannels: Int, outRate: Int, audioSettingsJson: String?, gainDb: Double): FloatArray

    object Native : AutomixFfi {
        override fun usable(record: AnalysisRecord, durationMs: Long): Boolean =
            app.hocket.core.ffi.automixRecordUsable(HocketJson.json.encodeToString(AnalysisRecord.serializer(), record), durationMs.toUInt())

        override fun loudnessRecord(head: PcmDecoder.Pcm, tail: PcmDecoder.Pcm, tailStartMs: Long, durationMs: Long, transcoded: Boolean): AnalysisRecord? {
            if (head.rate != tail.rate || head.channels != tail.channels || head.rate <= 0) return null
            val json = app.hocket.core.ffi.automixLoudnessRecord(
                head.leBytes(), tail.leBytes(), tailStartMs.toUInt(), head.channels.toUInt(), head.rate.toUInt(),
                durationMs.toUInt(), transcoded, System.currentTimeMillis().toDouble(),
            )
            return HocketJson.json.decodeFromString(AnalysisRecord.serializer(), json)
        }

        override fun prepareTail(planJson: String, pcm: PcmDecoder.Pcm, outChannels: Int, outRate: Int, audioSettingsJson: String?, gainDb: Double): FloatArray {
            val bytes = app.hocket.core.ffi.automixPrepareTail(
                planJson, pcm.leBytes(), pcm.channels.toUInt(), pcm.rate.toUInt(), outChannels.toUInt(), outRate.toUInt(), audioSettingsJson, gainDb,
            )
            val floats = java.nio.ByteBuffer.wrap(bytes).order(java.nio.ByteOrder.LITTLE_ENDIAN).asFloatBuffer()
            return FloatArray(floats.remaining()).also { floats.get(it) }
        }
    }
}
