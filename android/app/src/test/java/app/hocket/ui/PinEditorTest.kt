package app.hocket.ui

import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.click
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.performTouchInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.Commands
import app.hocket.core.SettingKeys
import app.hocket.ui.a11y.A11yChecks
import app.hocket.ui.a11y.performCustomAction
import app.hocket.ui.nav.AppRoot
import app.hocket.ui.screens.library.LibraryPins
import app.hocket.ui.screens.library.PinEntry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * The Library's pin editor on the fake core: Edit opens it; the chip, unpin (and Undo) and the
 * accessibility moves change a draft only; Done saves the draft to `library.pins`, and × or back
 * throws it away.
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class PinEditorTest {
    @get:Rule
    val compose = createComposeRule()

    private lateinit var core: TestCore

    private fun exists(tag: String) = compose.onAllNodesWithTag(tag).fetchSemanticsNodes().isNotEmpty()

    private fun await(tag: String) {
        compose.waitUntil(5_000) { exists(tag) }
        compose.waitForIdle()
    }

    private fun click(tag: String, index: Int = 0) {
        await(tag)
        compose.onAllNodesWithTag(tag)[index].performSemanticsAction(SemanticsActions.OnClick)
        compose.waitForIdle()
    }

    private fun saved(): List<PinEntry> = LibraryPins.current(core.client)

    /** The editor's tiles' labels, in order (each tile's merged text: title, then kind line). */
    private fun tiles(): List<String> = compose.onAllNodesWithTag("library.pins.editor.pin").fetchSemanticsNodes()
        .map { n -> n.config[SemanticsProperties.Text].first().text }

    private fun chips(): List<String> = compose.onAllNodesWithTag("library.pins.editor.chip").fetchSemanticsNodes()
        .map { n -> n.config[SemanticsProperties.ContentDescription].first() }

    /** Five pins: an album (opens), a playlist (plays), an artist, a song and a genre. */
    private fun start(): List<PinEntry> {
        core = TestCore(startPlaying = false)
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        await("navBar")
        val lib = core.fake.library
        val pins = listOf(
            PinEntry("album", lib.albums[1].id, "open"),
            PinEntry("playlist", lib.playlists[0].id, "play"),
            PinEntry("artist", lib.artists[0].id, "open"),
            PinEntry("track", lib.tracks[3].id, "play"),
            PinEntry("genre", lib.genres.first().name, "open"),
        )
        core.client.dispatch(Commands.setSetting(SettingKeys.LIBRARY_PINS, LibraryPins.encode(pins)))
        click("navBar.library")
        await("library.pins")
        compose.waitUntil(5_000) { compose.onAllNodesWithTag("library.pin").fetchSemanticsNodes().size == pins.size }
        click("library.pins.edit")
        await("library.pins.editor")
        compose.waitUntil(5_000) { tiles().size == pins.size }
        return pins
    }

    private val lib get() = core.fake.library

    @Test
    fun editsStayInTheDraftUntilDone() {
        val pins = start()
        val album = lib.albums[1].name
        val playlist = lib.playlists[0].name
        // Every kind but the song gets a chip.
        assertEquals(listOf("Tap opens $album. Change", "Tap plays $playlist. Change", "Tap opens ${lib.artists[0].name}. Change", "Tap opens ${lib.genres.first().name}. Change"), chips())
        assertTrue(exists("library.pins.editor.count"))
        compose.onAllNodesWithText("5 pins · Pin more from any item's menu").fetchSemanticsNodes().single()

        // The chip steps Opens → Plays → Shuffles → Opens.
        click("library.pins.editor.chip", 0)
        assertEquals("Tap plays $album. Change", chips()[0])
        click("library.pins.editor.chip", 1)
        assertEquals("Tap shuffles $playlist. Change", chips()[1])
        click("library.pins.editor.chip", 1)
        click("library.pins.editor.chip", 1)
        assertEquals("Tap plays $playlist. Change", chips()[1])

        // Unpin the playlist, then Undo puts it back where it was.
        compose.mainClock.autoAdvance = false
        click("library.pins.editor.unpin", 1)
        compose.mainClock.advanceTimeBy(500)
        assertEquals(listOf(album, lib.artists[0].name, lib.tracks[3].title, lib.genres.first().name), tiles())
        compose.onAllNodesWithText("Unpinned $playlist").fetchSemanticsNodes().single()
        assertFalse("the count makes way for the snackbar", exists("library.pins.editor.count"))
        compose.onAllNodesWithText("Undo")[0].performSemanticsAction(SemanticsActions.OnClick)
        compose.mainClock.advanceTimeBy(500)
        compose.mainClock.autoAdvance = true
        compose.waitForIdle()
        assertEquals(listOf(album, playlist, lib.artists[0].name, lib.tracks[3].title, lib.genres.first().name), tiles())

        // Move the album later, then the genre earlier, with the accessibility actions.
        compose.onAllNodesWithTag("library.pins.editor.pin")[0].performCustomAction(compose, "Move later")
        compose.onAllNodesWithTag("library.pins.editor.pin")[4].performCustomAction(compose, "Move earlier")
        compose.waitForIdle()
        assertEquals(listOf(playlist, album, lib.artists[0].name, lib.genres.first().name, lib.tracks[3].title), tiles())
        // The first tile can't move earlier, nor the last later.
        val firstActions = compose.onAllNodesWithTag("library.pins.editor.pin")[0].fetchSemanticsNode().config[SemanticsActions.CustomActions].map { it.label }
        assertEquals(listOf("Move later"), firstActions)

        // Nothing is saved yet.
        assertEquals(pins, saved())
        A11yChecks.assertAccessible(compose, "pin editor")

        click("library.pins.editor.done")
        compose.waitUntil(5_000) { !exists("library.pins.editor") }
        assertEquals(listOf(pins[1], pins[0].copy(tap = "play"), pins[2], pins[4], pins[3]), saved())
    }

    @Test
    fun cancelAndBackThrowTheDraftAway() {
        val pins = start()
        click("library.pins.editor.unpin", 0)
        click("library.pins.editor.chip", 0)
        click("library.pins.editor.cancel")
        compose.waitUntil(5_000) { !exists("library.pins.editor") }
        assertEquals(pins, saved())

        // Back leaves the same way.
        click("library.pins.edit")
        await("library.pins.editor")
        click("library.pins.editor.unpin", 0)
        pressBack()
        compose.waitUntil(5_000) { !exists("library.pins.editor") }
        assertEquals(pins, saved())
        assertTrue(exists("library.screen"))
    }

    @Test
    fun unpinningEverythingSavesAnEmptyList() {
        val pins = start()
        repeat(pins.size) { click("library.pins.editor.unpin", 0) }
        compose.waitUntil(5_000) { tiles().isEmpty() }
        click("library.pins.editor.done")
        compose.waitUntil(5_000) { !exists("library.pins.editor") }
        assertEquals(emptyList<PinEntry>(), saved())
        await("library.pins.empty")
    }

    @Test
    fun doneWithNothingChangedLeavesThePinsAlone() {
        val pins = start()
        // Changed and changed back: still the same pins, so nothing is written.
        compose.onAllNodesWithTag("library.pins.editor.pin")[0].performCustomAction(compose, "Move later")
        compose.onAllNodesWithTag("library.pins.editor.pin")[1].performCustomAction(compose, "Move earlier")
        val before = core.client.settings.value[SettingKeys.LIBRARY_PINS]
        click("library.pins.editor.done")
        compose.waitUntil(5_000) { !exists("library.pins.editor") }
        assertEquals(pins, saved())
        assertTrue("no write when the draft is unchanged", before === core.client.settings.value[SettingKeys.LIBRARY_PINS])
    }

    @Test
    fun genreAndFilterPinsPlayOrShuffleWhenTheirTapSaysSo() {
        core = TestCore(startPlaying = false)
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        await("navBar")
        val genre = lib.genres.first().name
        val pins = listOf(PinEntry("genre", genre, "shuffle"), PinEntry("filter", "f-loved", "play"))
        core.client.dispatch(Commands.setSetting(SettingKeys.LIBRARY_PINS, LibraryPins.encode(pins)))
        click("navBar.library")
        compose.waitUntil(5_000) { compose.onAllNodesWithTag("library.pin").fetchSemanticsNodes().size == 2 }
        compose.onAllNodesWithTag("library.pin")[0].performTouchInput { click() }
        compose.waitUntil(5_000) { core.client.queue.value.contextLabel == genre }
        assertTrue(core.client.queue.value.shuffle)
        compose.onAllNodesWithTag("library.pin")[1].performTouchInput { click() }
        compose.waitUntil(5_000) { core.client.queue.value.contextLabel == "Loved, not played lately" }
        // Played in place: the Library is still the page shown.
        assertTrue(exists("library.screen"))
    }

    /** Back, sent to the window on top (the editor's dialog). */
    private fun pressBack() {
        @Suppress("UNCHECKED_CAST")
        val views = Class.forName("android.view.WindowManagerGlobal").let { c ->
            val global = c.getMethod("getInstance").invoke(null)
            (c.getDeclaredField("mViews").apply { isAccessible = true }.get(global) as List<android.view.View>).toList()
        }
        val top = views.last()
        compose.runOnUiThread {
            top.dispatchKeyEvent(android.view.KeyEvent(android.view.KeyEvent.ACTION_DOWN, android.view.KeyEvent.KEYCODE_BACK))
            top.dispatchKeyEvent(android.view.KeyEvent(android.view.KeyEvent.ACTION_UP, android.view.KeyEvent.KEYCODE_BACK))
        }
        compose.waitForIdle()
    }
}
