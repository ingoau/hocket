package app.hocket.ui

import androidx.activity.ComponentActivity
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performSemanticsAction
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.Commands
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * The picker on its own sheet — the add to playlist the swipe gesture, the queue's swipe and the
 * selection toolbar's overflow still open: the playlists load ticked, a tap moves a pending tick to
 * the summary line, and Done applies it and closes. (The song menu's picker is covered by
 * [SongMenuTest], on the menu's own sub-page.)
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], qualifiers = "w411dp-h891dp-xxhdpi")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class PlaylistPickerSheetTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    @Test
    fun theSheetPicksAndDoneApplies() {
        val core = TestCore(startPlaying = false)
        val lib = core.fake.library
        val track = lib.playlistTracks.getValue("pl0").first()
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        var closed = false
        compose.setThemedContent(core) {
            CompositionLocalProvider(app.hocket.ui.LocalDetailNavigator provides DetailNavigator({}, {})) {
                app.hocket.ui.components.PlaylistPicker(Commands.tracks(listOf(track)), onDismiss = { closed = true })
            }
        }
        compose.waitUntil(5_000) { compose.onAllNodesWithTag("picker.row.pl1", useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty() }
        val inPl1 = lib.playlistTracks.getValue("pl1").count { it == track }
        compose.onNodeWithTag("picker.row.pl1", useUnmergedTree = true).performSemanticsAction(SemanticsActions.OnClick)
        compose.waitForIdle()
        val summary = compose.onNodeWithTag("picker.summary").fetchSemanticsNode().config[SemanticsProperties.Text].joinToString { it.text }.orEmpty()
        assertTrue("summary says the add is pending: $summary", summary.contains("adding", ignoreCase = true))
        compose.onNodeWithTag("picker.done").performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { closed }
        assertEquals(inPl1 + 1, lib.playlistTracks.getValue("pl1").count { it == track })
    }
}
