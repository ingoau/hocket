package app.hocket.ui

import androidx.activity.ComponentActivity
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.Commands
import app.hocket.core.SettingKeys
import app.hocket.ui.screens.detail.sameArtist
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * An album's tracks name their artist only where it isn't the album's (a feature), and on every
 * track with `display.albumTrackArtists` on. A tall screen, so every row of the album is composed.
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h3000dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class AlbumTrackArtistsTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private fun count(text: String) = compose.onAllNodesWithText(text, useUnmergedTree = true).fetchSemanticsNodes().size

    @Test
    fun onlyAnotherArtistIsNamedUntilTheSettingNamesThemAll() {
        val core = TestCore(startPlaying = false)
        // The fake's fifth song features the next artist: "A feat. B" on A's album.
        val featured = core.fake.library.tracks.first { it.artist?.contains(" feat. ") == true }
        val album = core.fake.library.albums.first { it.id == featured.albumId }
        val tracks = core.fake.library.albumTracks(album.id)
        assertTrue(tracks.any { it.artist == album.artist })
        val features = tracks.count { it.artist == featured.artist }
        core.start()
        compose.setThemedContent(core) {
            app.hocket.ui.screens.detail.AlbumDetailScreen(androidx.navigation.compose.rememberNavController(), album.id)
        }
        compose.waitUntil(5_000) { count(featured.title) > 0 }
        assertEquals(features, count(featured.artist!!))
        // The album's own artist: in the header only.
        assertEquals(1, count(album.artist!!))

        core.client.dispatch(Commands.setSetting(SettingKeys.DISPLAY_ALBUM_TRACK_ARTISTS, "true"))
        compose.waitUntil(5_000) { count(album.artist!!) > 1 }
        assertEquals(features, count(featured.artist!!))
    }

    @Test
    fun theSameArtistIgnoresCaseAndSpacing() {
        assertTrue(sameArtist("Ninja Tune ", "ninja tune"))
        assertTrue(sameArtist(null, "Anyone"))
        assertFalse(sameArtist("A feat. B", "A"))
    }
}
