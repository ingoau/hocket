package app.hocket.ui

import androidx.activity.ComponentActivity
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipeRight
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.ui.screens.detail.ArtistDetailScreen
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * The artist page's top songs page sideways in columns, so their rows don't swipe: a horizontal
 * drag scrolls the shelf instead of queueing a song (report R-20261119-chbw).
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h3000dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class ArtistTopSongsSwipeTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    @Test
    fun aSwipeOnATopSongDoesNotQueueIt() {
        val core = TestCore(startPlaying = false)
        val lib = core.fake.library
        val artist = lib.albums.groupBy { it.artistId }.maxBy { (_, albums) -> albums.sumOf { lib.albumTracks(it.id).size } }.key!!
        core.start()
        compose.setThemedContent(core) { ArtistDetailScreen(androidx.navigation.compose.rememberNavController(), artist) }
        compose.waitUntil(5_000) { compose.onAllNodesWithTag("artist.top").fetchSemanticsNodes().isNotEmpty() }
        val first = lib.artistAlbums(artist).flatMap { lib.albumTracks(it.id) }.maxBy { it.playCount }
        val row = hasText(first.title) and hasAnyAncestor(hasTestTag("artist.top"))
        compose.waitUntil(5_000) { compose.onAllNodes(row).fetchSemanticsNodes().isNotEmpty() }
        val before = core.client.queue.value.entries.map { it.track.id }
        // Swipe right (play next by default) on the first column, where the shelf can't scroll further.
        compose.onAllNodes(row)[0].performTouchInput { swipeRight() }
        compose.waitForIdle()
        Thread.sleep(300)
        compose.waitForIdle()
        assertEquals(before, core.client.queue.value.entries.map { it.track.id })
    }
}
