package app.hocket.ui

import app.hocket.core.groupEntries
import app.hocket.core.historyEntries
import app.hocket.core.restEntries
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsNotDisplayed
import androidx.compose.ui.unit.dp
import androidx.compose.ui.test.assertTextEquals
import androidx.compose.ui.test.getBoundsInRoot
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.Commands
import app.hocket.core.SettingKeys
import app.hocket.core.api.Command
import app.hocket.core.api.CommandStashAddInner
import app.hocket.ui.nav.AppRoot
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * The song stash (#150) on the phone: the chip appears beside the floating bar (or above the mini
 * player, end-aligned, with the standard bar) once something is stashed, shows the count, and opens
 * the sheet, whose actions keep the stash and whose Clear empties it.
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class StashTest {
    @get:Rule
    val compose = createComposeRule()

    private fun start(): TestCore {
        val core = TestCore()
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        compose.waitForIdle()
        return core
    }

    private fun stashSome(core: TestCore, n: Int): List<String> {
        val q = core.client.queue.value
        val ids = (listOfNotNull(q.current) + q.groupEntries + q.restEntries).map { it.item.track }.distinct().take(n)
        core.client.dispatch(Command.StashAdd(CommandStashAddInner(ids)))
        compose.waitUntil(5_000) { core.client.stash.value.size == ids.size }
        compose.waitForIdle()
        return ids.map { it.id }
    }

    @Test
    fun chipSitsBesideTheFloatingBar() {
        val core = start()
        compose.onAllNodesWithTag("stash.chip").assertCountEquals(0)
        stashSome(core, 2)
        compose.onNodeWithTag("stash.chip").assertIsDisplayed()
        compose.onNodeWithTag("stash.count", useUnmergedTree = true).assertTextEquals("2")
        val chip = compose.onNodeWithTag("stash.chip").getBoundsInRoot()
        val lastPlace = compose.onNodeWithTag("navBar.library").getBoundsInRoot()
        val mini = compose.onNodeWithTag("miniPlayer").getBoundsInRoot()
        // To the right of the pill, on the same line, below the mini player.
        assertTrue("chip left ${chip.left} vs pill end ${lastPlace.right}", chip.left >= lastPlace.right)
        assertTrue("chip ${chip.top}..${chip.bottom} vs mini bottom ${mini.bottom}", chip.top >= mini.bottom)
        val width = compose.onNodeWithTag("navBar").getBoundsInRoot().right
        assertTrue("chip ends inside the screen", chip.right <= width)
    }

    @Test
    fun withTheStandardBarTheChipFloatsAboveTheMiniPlayerAtTheEnd() {
        val core = start()
        core.client.dispatch(Commands.setSetting(SettingKeys.DISPLAY_FLOATING_NAV_BAR, "false"))
        compose.waitForIdle()
        stashSome(core, 1)
        compose.onNodeWithTag("stash.chip").assertIsDisplayed()
        val chip = compose.onNodeWithTag("stash.chip").getBoundsInRoot()
        val mini = compose.onNodeWithTag("miniPlayer").getBoundsInRoot()
        assertTrue("chip bottom ${chip.bottom} above mini top ${mini.top}", chip.bottom <= mini.top)
        assertTrue("chip right ${chip.right} lines up with mini right ${mini.right}", kotlin.math.abs((chip.right - mini.right).value) < 2f)
    }

    @Test
    fun sheetActsOnTheWholeStashUntilSongsArePickedAndKeepsItUntilCleared() {
        val core = start()
        stashSome(core, 3)
        compose.onNodeWithTag("stash.chip").performClick()
        compose.waitForIdle()
        compose.onNodeWithTag("stash.sheet").assertIsDisplayed()
        compose.onAllNodesWithTag("stash.row").assertCountEquals(3)
        // Nothing picked: the menu (no header of its own) acts on all three, and keeps them.
        compose.onAllNodesWithTag("stash.menu.header").assertCountEquals(0)
        compose.onNodeWithTag("stash.acting").assertTextEquals("For all 3 songs")
        compose.onAllNodesWithTag("stash.menu.remove").assertCountEquals(0)
        val queued = core.client.queue.value.restEntries.size
        compose.onNodeWithTag("stash.menu.action.playLater").performScrollTo().performClick()
        compose.waitUntil(5_000) { core.client.queue.value.restEntries.size >= queued }
        compose.waitForIdle()
        assertTrue(core.client.stash.value.size == 3)
        // Tapping a song picks it: the menu acts on that one, and can take it out of the stash.
        compose.onAllNodesWithTag("stash.row")[0].performScrollTo().performClick()
        compose.waitForIdle()
        compose.onNodeWithTag("stash.summary").assertTextEquals("1 of 3 selected")
        compose.onNodeWithTag("stash.acting").assertTextEquals("For the 1 selected song")
        val first = core.client.stash.value.first().entryId
        // (Clicked through its semantics: Play later's snackbar may cover the bottom of the sheet.)
        compose.onNodeWithTag("stash.menu.remove").performScrollTo().performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { core.client.stash.value.size == 2 }
        compose.waitForIdle()
        assertTrue(core.client.stash.value.none { it.entryId == first })
        compose.onAllNodesWithTag("stash.row").assertCountEquals(2)
        compose.onNodeWithTag("stash.acting").assertTextEquals("For all 2 songs")
        // Clear: the sheet and the chip go.
        compose.onNodeWithTag("stash.clear").performClick()
        compose.waitUntil(5_000) { core.client.stash.value.isEmpty() }
        compose.waitForIdle()
        compose.onAllNodesWithTag("stash.chip").assertCountEquals(0)
    }

    @Test
    fun pickingOneSongShowsItsOwnMenuWhichGoesToItsAlbum() {
        val core = start()
        stashSome(core, 2)
        compose.onNodeWithTag("stash.chip").performClick()
        compose.waitForIdle()
        // Rows have no menu of their own: the menu under them is the one.
        compose.onAllNodesWithContentDescription("More options for", substring = true, useUnmergedTree = true).assertCountEquals(0)
        // Several songs (their menu loaded): no go-to row.
        compose.waitUntil(5_000) { compose.onAllNodesWithTag("stash.menu.action.play", useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty() }
        compose.onAllNodesWithTag("stash.menu.action.goToAlbum", useUnmergedTree = true).assertCountEquals(0)
        compose.onAllNodesWithTag("stash.row")[0].performScrollTo().performClick()
        compose.waitForIdle()
        compose.onNodeWithTag("stash.menu.remove").assertTextEquals("Remove from stash")
        compose.onNodeWithTag("stash.menu.action.goToAlbum", useUnmergedTree = true).performScrollTo().performClick()
        compose.waitForIdle()
        compose.onAllNodesWithTag("stash.sheet").assertCountEquals(0)
        assertTrue(core.client.stash.value.size == 2)
    }

    @Test
    @Config(qualifiers = "w411dp-h960dp")
    fun aShortStashRestsWithItsButtonsAtTheFootAndOpensToTheRest() {
        val core = start()
        stashSome(core, 2)
        compose.onNodeWithTag("stash.chip").performClick()
        compose.waitUntil(5_000) { compose.onAllNodesWithTag("stash.menu.action.play", useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty() }
        compose.waitForIdle()
        val screen = compose.onAllNodes(androidx.compose.ui.test.isRoot())[0].getBoundsInRoot()
        // At rest: the songs, then the play buttons at the screen's foot; the rest of the menu below it.
        compose.onAllNodesWithTag("stash.row").assertCountEquals(2)
        compose.onNodeWithTag("stash.summary").assertIsDisplayed()
        val playGroup = compose.onNodeWithTag("stash.menu.action.playLater", useUnmergedTree = true).getBoundsInRoot()
        assertTrue("buttons end ${playGroup.bottom} on a ${screen.bottom} screen", playGroup.bottom <= screen.bottom - 8.dp && playGroup.bottom >= screen.bottom - 40.dp)
        val sheetTop = compose.onNodeWithTag("stash.sheet").getBoundsInRoot().top
        assertTrue("sheet top $sheetTop", sheetTop > 80.dp)
        compose.onNodeWithTag("stash.menu.action.addToPlaylist", useUnmergedTree = true).assertIsNotDisplayed()
        // The handle opens it the rest of the way: as tall as it all is, the menu's end in view.
        compose.onNodeWithTag("stash.sheetHandle").performClick()
        compose.waitForIdle()
        compose.onNodeWithTag("stash.menu.action.addToPlaylist", useUnmergedTree = true).assertIsDisplayed()
        val openTop = compose.onNodeWithTag("stash.sheet").getBoundsInRoot().top
        assertTrue("open top $openTop above rest top $sheetTop", openTop < sheetTop)
    }

    @Test
    @Config(qualifiers = "w411dp-h640dp")
    fun aLongStashOpensAtItsTopAndTheHeaderScrollsAway() {
        val core = start()
        stashSome(core, 9)
        compose.onNodeWithTag("stash.chip").performClick()
        compose.waitUntil(5_000) { compose.onAllNodesWithTag("stash.row").fetchSemanticsNodes().isNotEmpty() }
        compose.waitForIdle()
        // Opened at the top: the header, then the first song.
        compose.onNodeWithTag("stash.summary").assertIsDisplayed()
        val first = core.client.stash.value.first().track.title
        compose.onAllNodes(androidx.compose.ui.test.hasTestTag("stash.row") and androidx.compose.ui.test.hasText(first, substring = true))[0].assertIsDisplayed()
        // The header is no fixed bar: scrolled down to the menu, it's gone with the songs.
        compose.onNodeWithTag("stash.list").performScrollToNode(androidx.compose.ui.test.hasTestTag("stash.menu"))
        compose.waitForIdle()
        compose.onNodeWithTag("stash.summary").assertIsNotDisplayed()
    }

    @Test
    @Config(qualifiers = "w320dp-h700dp")
    fun onANarrowPhoneTheChipFloatsAboveTheMiniPlayerInstead() {
        val core = start()
        stashSome(core, 12)
        compose.onNodeWithTag("stash.chip").assertIsDisplayed()
        val chip = compose.onNodeWithTag("stash.chip").getBoundsInRoot()
        val mini = compose.onNodeWithTag("miniPlayer").getBoundsInRoot()
        assertTrue("chip bottom ${chip.bottom} above mini top ${mini.top}", chip.bottom <= mini.top)
    }
}
