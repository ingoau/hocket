package app.hocket.playback

import app.hocket.core.api.AnalysisRecord
import app.hocket.core.api.Query
import app.hocket.core.api.QueryResult
import app.hocket.core.api.TransitionMode
import app.hocket.core.api.TransitionSettings
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Test

/** [LoudnessRecorder]'s window handling (no decoding: no candidate's file is on the device). */
@OptIn(ExperimentalCoroutinesApi::class)
class LoudnessRecorderTest {
    private object NoFfi : AutomixFfi {
        override fun usable(record: AnalysisRecord, durationMs: Long) = false
        override fun loudnessRecord(head: PcmDecoder.Pcm, tail: PcmDecoder.Pcm, tailStartMs: Long, durationMs: Long, transcoded: Boolean): AnalysisRecord? = null
        override fun prepareTail(planJson: String, pcm: PcmDecoder.Pcm, contextMs: Int, outChannels: Int, outRate: Int, audioSettingsJson: String?, gainDb: Double) =
            RenderedTail(FloatArray(0), FloatArray(0), FloatArray(0))
    }

    @Test
    fun aWindowThatChangesDuringAPassRunsAfterIt() = runTest {
        val dispatcher = StandardTestDispatcher(testScheduler)
        val gate = CompletableDeferred<Unit>()
        val looked = mutableListOf<String>()
        val recorder = LoudnessRecorder(
            this,
            { q ->
                when (q) {
                    is Query.Transitions -> {
                        gate.await()
                        QueryResult.Transitions(TransitionSettings(TransitionMode.AutoMix, 6.0))
                    }
                    is Query.TrackAnalysis -> {
                        looked += q.data.track_id
                        null
                    }
                    else -> null
                }
            },
            {}, { null }, dispatcher, dispatcher, NoFfi,
        )
        recorder.onWindow(listOf(LoudnessRecorder.Candidate("t1", 1_000, null)))
        advanceUntilIdle()
        // Two changes while the first pass waits: the newest one runs after it, the middle one never.
        recorder.onWindow(listOf(LoudnessRecorder.Candidate("t2", 1_000, null)))
        recorder.onWindow(listOf(LoudnessRecorder.Candidate("t3", 1_000, null)))
        gate.complete(Unit)
        advanceUntilIdle()
        assertEquals(listOf("t1", "t3"), looked)
        recorder.cancel()
    }
}
