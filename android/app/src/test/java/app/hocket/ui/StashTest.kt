package app.hocket.ui

import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertTextEquals
import androidx.compose.ui.test.getBoundsInRoot
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
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
        val ids = (listOfNotNull(q.current) + q.playingNext + q.upcoming).map { it.track.id }.distinct().take(n)
        core.client.dispatch(Command.StashAdd(CommandStashAddInner(ids)))
        compose.waitUntil(5_000) { core.client.stash.value.size == ids.size }
        compose.waitForIdle()
        return ids
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
    fun sheetActsOnTheStashAndKeepsItUntilCleared() {
        val core = start()
        stashSome(core, 3)
        compose.onNodeWithTag("stash.chip").performClick()
        compose.waitForIdle()
        compose.onNodeWithTag("stash.sheet").assertIsDisplayed()
        compose.onAllNodesWithTag("stash.row").assertCountEquals(3)
        compose.onNodeWithTag("stash.playLater").performClick()
        compose.waitForIdle()
        assertTrue(core.client.stash.value.size == 3)
        // Remove one with ×.
        compose.onAllNodesWithTag("stash.remove")[0].performClick()
        compose.waitUntil(5_000) { core.client.stash.value.size == 2 }
        compose.waitForIdle()
        compose.onAllNodesWithTag("stash.row").assertCountEquals(2)
        // Clear: the sheet and the chip go.
        compose.onNodeWithTag("stash.clear").performClick()
        compose.waitUntil(5_000) { core.client.stash.value.isEmpty() }
        compose.waitForIdle()
        compose.onAllNodesWithTag("stash.chip").assertCountEquals(0)
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
