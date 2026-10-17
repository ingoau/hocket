package app.hocket.ui

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performSemanticsAction
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.ui.screens.library.LibraryList
import app.hocket.ui.screens.library.ListSort
import app.hocket.ui.screens.library.SortChip
import app.hocket.ui.screens.library.SortKey
import app.hocket.ui.screens.library.sortKeys
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
 * What the sort chip and sheet say to accessibility services: the chip names the order and its
 * direction (an order without one, Random, only its name); the orders and the two directions are
 * radio buttons, each "selected" or not, with no toggle state besides; a shuffle's directions are
 * disabled and neither is chosen.
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class SortSheetSemanticsTest {
    @get:Rule
    val compose = createComposeRule()

    private fun node(tag: String) = compose.onNodeWithTag(tag).fetchSemanticsNode().config

    private fun click(tag: String) {
        compose.onNodeWithTag(tag).performSemanticsAction(SemanticsActions.OnClick)
        compose.waitForIdle()
    }

    private fun chip(): String = node("library.sort")[SemanticsProperties.ContentDescription].single()

    /** A radio button: its role, whether it is selected, and no checked / unchecked state of its own. */
    private fun assertRadio(tag: String, selected: Boolean) {
        val config = node(tag)
        assertEquals(tag, Role.RadioButton, config.getOrNull(SemanticsProperties.Role))
        assertEquals(tag, selected, config.getOrNull(SemanticsProperties.Selected))
        assertNull(tag, config.getOrNull(SemanticsProperties.ToggleableState))
    }

    private fun start(initial: ListSort) {
        val core = TestCore(startPlaying = false)
        compose.setThemedContent(core) {
            var sort by remember { mutableStateOf(initial) }
            SortChip(sort, LibraryList.Songs.sortKeys, { sort = it })
        }
        compose.waitForIdle()
    }

    @Test
    fun theSheetsRowsAndDirectionsAreRadioButtons() {
        start(ListSort(SortKey.Bpm, false))
        assertEquals("Sort: BPM, Slowest", chip())
        click("library.sort")
        compose.waitUntil(5_000) { compose.onAllNodesWithTag("library.sortSheet").fetchSemanticsNodes().isNotEmpty() }
        LibraryList.Songs.sortKeys.forEach { assertRadio("library.sort.option.${it.id}", it == SortKey.Bpm) }
        assertRadio("library.sort.ascending", true)
        assertRadio("library.sort.descending", false)

        click("library.sort.descending")
        assertRadio("library.sort.ascending", false)
        assertRadio("library.sort.descending", true)
        assertEquals("Sort: BPM, Fastest", chip())

        // An order with its usual direction: most played first.
        click("library.sort.option.playCount")
        assertRadio("library.sort.option.playCount", true)
        assertRadio("library.sort.option.bpm", false)
        assertEquals("Sort: Most played, Most", chip())
    }

    @Test
    fun randomHasNoDirection() {
        start(ListSort(SortKey.Title, false))
        assertEquals("Sort: Title, A–Z", chip())
        click("library.sort")
        compose.waitUntil(5_000) { compose.onAllNodesWithTag("library.sortSheet").fetchSemanticsNodes().isNotEmpty() }
        click("library.sort.option.random")
        assertEquals("Sort: Random", chip())
        listOf("library.sort.ascending", "library.sort.descending").forEach { tag ->
            assertRadio(tag, false)
            assertTrue(tag, node(tag).contains(SemanticsProperties.Disabled))
        }
        assertFalse(node("library.sort.option.random").contains(SemanticsProperties.Disabled))
    }
}
