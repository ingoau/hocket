package app.hocket.diagnostics

import android.hardware.SensorManager
import org.junit.Assert.assertEquals
import org.junit.Test

/** Three jolts in a short window are a shake; one long swing, slow wobbles and an encore during the cooldown are not. */
class ShakeDetectorTest {
    private val g = SensorManager.GRAVITY_EARTH
    private var shakes = 0
    private val detector = ShakeDetector { shakes++ }

    private fun jolt(atMs: Long, strength: Float = 3f) = detector.onAcceleration(strength * g, 0f, 0f, atMs)

    @Test
    fun threeJoltsInAWindowAreOneShake() {
        jolt(0); jolt(300); jolt(600)
        assertEquals(1, shakes)
    }

    @Test
    fun restingAndGentleMovementIsNothing() {
        repeat(50) { detector.onAcceleration(0f, 0f, g, it * 20L) }
        repeat(50) { jolt(1_000L + it * 150, strength = 2f) }
        assertEquals(0, shakes)
    }

    @Test
    fun aSingleSwingReadAboveTheThresholdForManySamplesIsOneJolt() {
        // Samples every 20 ms for 100 ms: one jolt, then two more make the shake.
        for (t in 0L..100L step 20) jolt(t)
        assertEquals(0, shakes)
        jolt(400); jolt(700)
        assertEquals(1, shakes)
    }

    @Test
    fun joltsTooFarApartDoNotAddUp() {
        jolt(0); jolt(1_000); jolt(2_500); jolt(4_000)
        assertEquals(0, shakes)
    }

    @Test
    fun aLongShakeOpensOnceThenCoolsDown() {
        for (t in 0L..1_900L step 300) jolt(t)
        assertEquals(1, shakes)
        // After the cooldown a fresh shake counts again.
        jolt(5_000); jolt(5_300); jolt(5_600)
        assertEquals(2, shakes)
    }
}
