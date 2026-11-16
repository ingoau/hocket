package app.hocket.playback

import androidx.media3.exoplayer.audio.AudioSink
import org.junit.Assert.assertEquals
import org.junit.Test
import java.lang.reflect.Proxy

class MixClockSinkTest {
    /** The inner sink's clock (µs), and wall time (ms) the test moves along with it. */
    private var raw = 0L
    private var wallMs = 0L
    private var discontinuities = 0

    private val inner = Proxy.newProxyInstance(AudioSink::class.java.classLoader, arrayOf(AudioSink::class.java)) { _, method, _ ->
        when (method.name) {
            "getCurrentPositionUs" -> raw
            else -> null
        }
    } as AudioSink

    private val sink = MixClockSink(inner) { wallMs * 1_000_000 }.also {
        it.setListener(object : AudioSink.Listener {
            override fun onPositionDiscontinuity() {
                discontinuities++
            }
            override fun onUnderrun(bufferSize: Int, bufferSizeMs: Long, elapsedSinceLastFeedMs: Long) = Unit
            override fun onSkipSilenceEnabledChanged(skipSilenceEnabled: Boolean) = Unit
        })
    }

    /** Play [ms] of output: the raw clock and wall time move together. */
    private fun play(ms: Long): Long {
        raw += ms * 1_000
        wallMs += ms
        return sink.getCurrentPositionUs(false)
    }

    @Test
    fun aLeadIsTakenOutOnceAndDropsAtTheNextReBase() {
        raw = 100_000_000 // B's clock: 15 s ahead of B since the mix
        assertEquals(100_000_000L, sink.getCurrentPositionUs(false))
        sink.correct(15_000_000)
        assertEquals("the correction is a discontinuity", 85_010_000L, play(10))
        assertEquals(1, discontinuities)
        assertEquals(85_020_000L, play(10))
        // The next item's first frame: the sink re-bases, its raw clock steps back by the lead.
        raw -= 15_000_000
        assertEquals("continuous across the re-base", 85_030_000L, play(10))
        assertEquals(85_040_000L, play(10))
        assertEquals(1, discontinuities)
    }

    @Test
    fun aNegativeLeadDropsAtAForwardReBase() {
        raw = 50_000_000
        sink.getCurrentPositionUs(false)
        sink.correct(-2_000_000)
        assertEquals(52_010_000L, play(10))
        raw += 2_000_000 // the re-base: the clock that lagged catches up at once
        assertEquals(52_020_000L, play(10))
        assertEquals(52_030_000L, play(10))
    }

    @Test
    fun pausedTimeIsNotAReBase() {
        raw = 10_000_000
        sink.getCurrentPositionUs(false)
        sink.correct(-1_000_000)
        assertEquals(11_000_000L, sink.getCurrentPositionUs(false))
        wallMs += 60_000 // paused for a minute
        assertEquals(11_000_000L, sink.getCurrentPositionUs(false))
        assertEquals(11_010_000L, play(10))
    }

    @Test
    fun aFlushStartsOver() {
        raw = 30_000_000
        sink.getCurrentPositionUs(false)
        sink.correct(5_000_000)
        assertEquals(25_010_000L, play(10))
        sink.flush()
        raw = 1_000_000 // a seek
        assertEquals(1_000_000L, sink.getCurrentPositionUs(false))
        sink.correct(3_000_000)
        sink.flush()
        assertEquals("a pending correction goes with the flush", 1_000_000L, sink.getCurrentPositionUs(false))
    }

    @Test
    fun noPositionStaysNoPosition() {
        sink.correct(1_000_000)
        raw = AudioSink.CURRENT_POSITION_NOT_SET
        assertEquals(AudioSink.CURRENT_POSITION_NOT_SET, sink.getCurrentPositionUs(false))
    }
}
