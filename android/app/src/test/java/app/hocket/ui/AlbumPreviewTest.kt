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
import androidx.compose.ui.test.swipe
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
 * putting it away stops the preview and gives playback back. A short pull only springs back.
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class AlbumPreviewTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private fun exists(tag: String) = compose.onAllNodesWithTag(tag).fetchSemanticsNodes().isNotEmpty()

    private fun open(core: TestCore): List<Track> {
        val album = core.fake.library.albums.first { core.fake.library.albumTracks(it.id).size >= 3 }
        val tracks = core.fake.library.albumTracks(album.id)
        compose.setThemedContent(core) { AlbumDetailScreen(androidx.navigation.compose.rememberNavController(), album.id) }
        core.start()
        compose.waitUntil(5_000) { compose.onAllNodesWithText(tracks[0].title).fetchSemanticsNodes().isNotEmpty() }
        compose.waitForIdle()
        return tracks
    }

    private fun pull(dp: Float) {
        compose.onRoot().performTouchInput {
            val from = Offset(width / 2f, height * 0.3f)
            swipe(from, from + Offset(0f, dp * density), 400)
        }
        compose.mainClock.advanceTimeBy(1_000)
        compose.waitForIdle()
    }

    @Test
    fun aPullPreviewsTheSongsAndPuttingItAwayGivesPlaybackBack() {
        val core = TestCore(startPlaying = true)
        val tracks = open(core)
        compose.waitUntil(5_000) { core.client.transport.value.position.isPlaying }
        val playingId = core.client.nowPlaying.value?.track?.id
        assertNull(core.client.preview.value)

        pull(260f)
        assertTrue(exists("albumPreview.pager"))
        compose.waitUntil(5_000) { core.client.preview.value != null }
        // Opens on the playing song when it is this album's, else the first.
        val opened = tracks.indexOfFirst { it.id == core.client.preview.value?.trackId }
        assertEquals(tracks.indexOfFirst { it.id == playingId }.coerceAtLeast(0), opened)
        assertTrue(opened < tracks.lastIndex)
        assertFalse("the preview pauses this device", core.client.transport.value.position.isPlaying)

        compose.onNodeWithTag("albumPreview.pager").performTouchInput { swipeLeft(startX = width * 0.7f, endX = width * 0.3f, durationMillis = 250) }
        compose.mainClock.advanceTimeBy(1_000)
        compose.waitForIdle()
        compose.waitUntil(5_000) { core.client.preview.value?.trackId == tracks[opened + 1].id }

        compose.onNodeWithTag("albumPreview.close").performSemanticsAction(SemanticsActions.OnClick)
        compose.mainClock.advanceTimeBy(1_000)
        compose.waitForIdle()
        assertNull(core.client.preview.value)
        assertFalse(exists("albumPreview"))
        compose.waitUntil(5_000) { core.client.transport.value.position.isPlaying }
        assertEquals(playingId, core.client.nowPlaying.value?.track?.id)
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
        pull(260f)
        compose.waitUntil(5_000) { core.client.preview.value != null }
        val previewed = core.client.preview.value!!.trackId
        compose.onNodeWithTag("albumPreview.playFrom").performSemanticsAction(SemanticsActions.OnClick)
        compose.mainClock.advanceTimeBy(1_000)
        compose.waitForIdle()
        assertNull(core.client.preview.value)
        compose.waitUntil(5_000) { core.client.nowPlaying.value?.track?.id == previewed }
        assertTrue(tracks.any { it.id == previewed })
    }
}
