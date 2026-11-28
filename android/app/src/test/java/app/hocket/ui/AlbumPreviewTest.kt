package app.hocket.ui

import androidx.activity.ComponentActivity
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.click
import androidx.compose.ui.test.swipeLeft
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.api.Track
import app.hocket.ui.screens.detail.AlbumDetailScreen
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * The album preview: a pull down from the top of an album page docks the cover and previews the
 * song on it (pausing what was playing here), a swipe moves the preview to the next song, and
 * putting it away stops the preview and gives playback back. A short pull only springs back. Play
 * from here (the button, or the card let go at the bottom) plays the album on from the preview.
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class AlbumPreviewTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private fun exists(tag: String) = compose.onAllNodesWithTag(tag).fetchSemanticsNodes().isNotEmpty()

    /** Shows the album page while [shown] is true. */
    private val shown = androidx.compose.runtime.mutableStateOf(true)

    private fun open(core: TestCore): List<Track> {
        val album = core.fake.library.albums.first { core.fake.library.albumTracks(it.id).size >= 3 }
        val tracks = core.fake.library.albumTracks(album.id)
        compose.setThemedContent(core) { if (shown.value) AlbumDetailScreen(androidx.navigation.compose.rememberNavController(), album.id) }
        core.start()
        compose.waitUntil(5_000) { compose.onAllNodesWithText(tracks[0].title).fetchSemanticsNodes().isNotEmpty() }
        compose.waitForIdle()
        return tracks
    }

    /** A slow pull (no flick): only the distance decides. */
    private fun pull(dp: Float, settle: Boolean = true) {
        compose.onRoot().performTouchInput {
            val from = Offset(width / 2f, height * 0.3f)
            down(from)
            val steps = 30
            for (i in 1..steps) moveTo(from + Offset(0f, dp * density * i / steps), 40)
            // Held still before letting go, so the release has no speed.
            repeat(8) { moveBy(Offset.Zero, 40) }
            up()
        }
        if (settle) {
            compose.mainClock.advanceTimeBy(1_000)
            compose.waitForIdle()
        }
    }

    private fun previewing(core: TestCore) = core.client.preview.value?.takeIf { it.playing }?.trackId

    @Test
    fun aPullPreviewsTheSongsAndPuttingItAwayGivesPlaybackBack() {
        val core = TestCore(startPlaying = true)
        val tracks = open(core)
        compose.waitUntil(5_000) { core.client.transport.value.position.isPlaying }
        val playingId = core.client.nowPlaying.value?.track?.id
        assertNull(core.client.preview.value)

        pull(200f)
        assertTrue(exists("albumPreview.pager"))
        compose.waitUntil(5_000) { core.client.preview.value != null }
        // Opens on the playing song when it is this album's, else the first.
        val opened = tracks.indexOfFirst { it.id == core.client.preview.value?.trackId }
        assertEquals(tracks.indexOfFirst { it.id == playingId }.coerceAtLeast(0), opened)
        assertTrue(opened < tracks.lastIndex)
        assertFalse("the preview pauses this device", core.client.transport.value.position.isPlaying)
        assertEquals("the preview plays", tracks[opened].id, previewing(core))

        compose.onNodeWithTag("albumPreview.pager").performTouchInput { swipeLeft(startX = width * 0.7f, endX = width * 0.3f, durationMillis = 250) }
        compose.mainClock.advanceTimeBy(1_000)
        compose.waitForIdle()
        compose.waitUntil(5_000) { previewing(core) == tracks[opened + 1].id }
        assertFalse(core.client.transport.value.position.isPlaying)

        compose.onNodeWithTag("albumPreview.close").performSemanticsAction(SemanticsActions.OnClick)
        compose.mainClock.advanceTimeBy(1_000)
        compose.waitForIdle()
        assertNull(core.client.preview.value)
        assertFalse(exists("albumPreview"))
        compose.waitUntil(5_000) { core.client.transport.value.position.isPlaying }
        assertEquals(playingId, core.client.nowPlaying.value?.track?.id)
    }

    @Test
    fun backPutsItAwayAndLeavingThePageStopsIt() {
        val core = TestCore(startPlaying = true)
        open(core)
        compose.waitUntil(5_000) { core.client.transport.value.position.isPlaying }
        pull(200f)
        compose.waitUntil(5_000) { previewing(core) != null }
        compose.activityRule.scenario.onActivity { it.onBackPressedDispatcher.onBackPressed() }
        compose.mainClock.advanceTimeBy(1_000)
        compose.waitForIdle()
        assertNull(core.client.preview.value)
        assertFalse(exists("albumPreview"))
        compose.waitUntil(5_000) { core.client.transport.value.position.isPlaying }

        // Open again, then the page goes away with it open: the preview stops, playback comes back.
        pull(200f)
        compose.waitUntil(5_000) { previewing(core) != null }
        assertFalse(core.client.transport.value.position.isPlaying)
        shown.value = false
        compose.waitForIdle()
        assertNull(core.client.preview.value)
        compose.waitUntil(5_000) { core.client.transport.value.position.isPlaying }
    }

    @Test
    fun aTapAsAShortPullSpringsBackStartsNothing() {
        val core = TestCore(startPlaying = false)
        open(core)
        compose.mainClock.autoAdvance = false
        pull(80f, settle = false)
        compose.mainClock.advanceTimeBy(48)
        // Where the docked card and its controls would be, while the cover is still on its way home.
        compose.onRoot().performTouchInput { click(Offset(width / 2f, height * 0.35f)); click(Offset(width / 2f, height * 0.75f)) }
        compose.mainClock.autoAdvance = true
        compose.mainClock.advanceTimeBy(1_000)
        compose.waitForIdle()
        assertFalse(exists("albumPreview"))
        assertNull(core.client.preview.value)
    }

    @Test
    fun aSwipeDownPutsItAwayAndAShortOneSpringsBack() {
        val core = TestCore(startPlaying = true)
        open(core)
        compose.waitUntil(5_000) { core.client.transport.value.position.isPlaying }
        pull(200f)
        compose.waitUntil(5_000) { previewing(core) != null }
        val previewed = previewing(core)
        // A little way down and let go: the card springs back and the preview carries on.
        swipeCard(48f)
        assertTrue(exists("albumPreview.pager"))
        assertEquals(previewed, previewing(core))
        // Far enough down: away, and playback comes back.
        swipeCard(200f)
        assertFalse(exists("albumPreview"))
        assertNull(core.client.preview.value)
        compose.waitUntil(5_000) { core.client.transport.value.position.isPlaying }
    }

    @Test
    fun grabbingTheCardAsALongPullDocksKeepsItOpen() {
        val core = TestCore(startPlaying = false)
        open(core)
        compose.mainClock.autoAdvance = false
        // Far past the threshold, then the card grabbed straight away and nudged up.
        pull(420f, settle = false)
        compose.mainClock.advanceTimeBy(48)
        compose.onNodeWithTag("albumPreview.pager").performTouchInput {
            down(center)
            // Quick, so the first deltas land while it's still settling; then held so the release isn't a flick.
            for (i in 1..3) moveTo(center - Offset(0f, 10f * density * i), 8)
            repeat(8) { moveBy(Offset.Zero, 40) }
            up()
        }
        compose.mainClock.autoAdvance = true
        compose.mainClock.advanceTimeBy(1_000)
        compose.waitForIdle()
        assertTrue("still open", exists("albumPreview.pager"))
        compose.waitUntil(5_000) { previewing(core) != null }
    }

    /** A slow drag down on the docked card. */
    private fun swipeCard(dp: Float) {
        compose.onNodeWithTag("albumPreview.pager").performTouchInput {
            val from = center
            down(from)
            val steps = 20
            for (i in 1..steps) moveTo(from + Offset(0f, dp * density * i / steps), 40)
            repeat(8) { moveBy(Offset.Zero, 40) }
            up()
        }
        compose.mainClock.advanceTimeBy(1_000)
        compose.waitForIdle()
    }

    @Test
    fun aShortPullSpringsBack() {
        val core = TestCore(startPlaying = false)
        open(core)
        pull(48f)
        assertFalse(exists("albumPreview"))
        assertNull(core.client.preview.value)
    }

    @Test
    fun playFromHerePlaysTheAlbumFromThatSong() {
        val core = TestCore(startPlaying = false)
        val tracks = open(core)
        pull(200f)
        compose.waitUntil(5_000) { core.client.preview.value != null }
        val previewed = core.client.preview.value!!.trackId
        compose.onNodeWithTag("albumPreview.playFrom").performSemanticsAction(SemanticsActions.OnClick)
        compose.mainClock.advanceTimeBy(1_000)
        compose.waitForIdle()
        assertNull(core.client.preview.value)
        compose.waitUntil(5_000) { core.client.nowPlaying.value?.track?.id == previewed }
        assertTrue(tracks.any { it.id == previewed })
        // On from the preview (which opened 30% in), not from the top.
        assertTrue(core.client.transport.value.position.positionMs > 0u)
    }

    @Test
    fun stashAndLoveBesideTheTitleToggleTheSongOnTheCard() {
        val core = TestCore(startPlaying = false)
        open(core)
        pull(200f)
        compose.waitUntil(5_000) { core.client.preview.value != null }
        val id = core.client.preview.value!!.trackId
        val wasLoved = core.fake.library.track(id)!!.loved
        fun tap(tag: String) {
            compose.onNodeWithTag(tag).performSemanticsAction(SemanticsActions.OnClick)
            compose.waitForIdle()
        }
        tap("albumPreview.stash")
        compose.waitUntil(5_000) { core.client.stash.value.any { it.track.id == id } }
        tap("albumPreview.stash")
        compose.waitUntil(5_000) { core.client.stash.value.none { it.track.id == id } }
        tap("albumPreview.love")
        compose.waitUntil(5_000) { core.fake.library.track(id)!!.loved != wasLoved }
        // Still up: neither puts the preview away.
        assertTrue(exists("albumPreview.info"))
        assertEquals(id, core.client.preview.value?.trackId)
    }

    @Test
    fun aTapBesideTheCardPutsItAway() {
        val core = TestCore(startPlaying = false)
        open(core)
        pull(200f)
        compose.waitUntil(5_000) { core.client.preview.value != null }
        // Above the card's left edge, clear of the card and the close button.
        compose.onRoot().performTouchInput { click(Offset(16f * density, 100f * density)) }
        compose.mainClock.advanceTimeBy(1_000)
        compose.waitForIdle()
        assertFalse(exists("albumPreview"))
        assertNull(core.client.preview.value)
    }

    @Test
    fun draggingTheCardToTheBottomPlaysFromIt() {
        val core = TestCore(startPlaying = false)
        open(core)
        pull(200f)
        compose.waitUntil(5_000) { core.client.preview.value != null }
        val previewed = core.client.preview.value!!.trackId
        // Past the dismissal, on to the bottom of the screen: the drop target lights up.
        swipeCard(540f)
        assertFalse(exists("albumPreview"))
        assertNull(core.client.preview.value)
        compose.waitUntil(5_000) { core.client.nowPlaying.value?.track?.id == previewed }
        assertTrue(core.client.transport.value.position.isPlaying)
        assertTrue(core.client.transport.value.position.positionMs > 0u)
    }
}
