package app.hocket.ui.components

import androidx.activity.ComponentActivity
import androidx.compose.foundation.layout.size
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.unit.dp
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.ArtworkSizes
import app.hocket.ui.TestCore
import app.hocket.ui.setThemedContent
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import java.util.concurrent.atomic.AtomicInteger

/**
 * The full player's artwork stayed the id's gradient for a whole session when its one fetch failed
 * (a network drop answers "no file"), while the mini player's smaller size had loaded: a size with
 * no file is asked for again, and a smaller known file stands in meanwhile.
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35])
class ArtworkRetryTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    @Test
    fun aFailedFetchIsAskedForAgain() {
        val id = "retry-${System.nanoTime()}"
        val core = TestCore(startPlaying = false)
        val asks = AtomicInteger()
        core.fake.artworkFile = { _, _ -> asks.incrementAndGet(); null }
        compose.mainClock.autoAdvance = false
        compose.setThemedContent(core) { Artwork(id, ArtworkSizes.FULL, null, Modifier.size(100.dp)) }
        compose.mainClock.advanceTimeByFrame()
        compose.waitForIdle()
        assertEquals(1, asks.get())
        compose.mainClock.advanceTimeBy(2_100)
        compose.waitForIdle()
        assertEquals(2, asks.get())
        compose.mainClock.advanceTimeBy(4_100)
        compose.waitForIdle()
        assertEquals(3, asks.get())
    }

    @Test
    fun aSmallerKnownFileStandsIn() {
        val id = "stand-${System.nanoTime()}"
        assertNull(ArtworkPathCache.smaller(id, ArtworkSizes.FULL))
        ArtworkPathCache.put(ArtworkPathCache.key(id, ArtworkSizes.THUMB), "/t")
        ArtworkPathCache.put(ArtworkPathCache.key(id, ArtworkSizes.LIST), "/l")
        ArtworkPathCache.put(ArtworkPathCache.key(id, ArtworkSizes.HERO), "/h")
        assertEquals("/l", ArtworkPathCache.smaller(id, ArtworkSizes.FULL)?.path)
        assertEquals("/t", ArtworkPathCache.smaller(id, ArtworkSizes.LIST)?.path)
        assertNull(ArtworkPathCache.smaller(id, ArtworkSizes.THUMB))
    }
}
