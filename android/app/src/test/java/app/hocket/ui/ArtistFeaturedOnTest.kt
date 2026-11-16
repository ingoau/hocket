package app.hocket.ui

import androidx.activity.ComponentActivity
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.ui.screens.detail.ArtistDetailScreen
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * An artist's page lists the others' albums they're credited on as "Featured on"; an artist the
 * core knows nothing of says so rather than loading forever. A tall screen, so every section is
 * composed.
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h3000dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class ArtistFeaturedOnTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private fun count(text: String) = compose.onAllNodesWithText(text, useUnmergedTree = true).fetchSemanticsNodes().size

    @Test
    fun aGuestsPageFeaturesTheAlbumTheyAreOn() {
        val core = TestCore(startPlaying = false)
        // The fake's fifth song features the next artist: "A feat. B" on A's album.
        val featured = core.fake.library.tracks.first { it.artist?.contains(" feat. ") == true }
        val guest = featured.artists!!.last()
        val album = core.fake.library.albums.first { it.id == featured.albumId }
        core.start()
        compose.setThemedContent(core) { ArtistDetailScreen(androidx.navigation.compose.rememberNavController(), guest.id!!) }
        compose.waitUntil(5_000) { count("Featured on") > 0 }
        val onShelf = hasText(album.name) and hasAnyAncestor(hasTestTag("artist.featured"))
        compose.waitUntil(5_000) { compose.onAllNodes(onShelf, useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty() }
        // Whose album it is, under its name.
        assertTrue(compose.onAllNodes(hasText(album.artist!!, substring = true) and hasAnyAncestor(hasTestTag("artist.featured")), useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty())
    }

    @Test
    fun anArtistNoSongCreditsIsNotLeftLoading() {
        val core = TestCore(startPlaying = false)
        core.start()
        compose.setThemedContent(core) { ArtistDetailScreen(androidx.navigation.compose.rememberNavController(), "ar-nobody") }
        compose.waitUntil(5_000) { count("Not in your library") > 0 }
    }
}
