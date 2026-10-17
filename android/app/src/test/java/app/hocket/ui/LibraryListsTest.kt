package app.hocket.ui

import androidx.activity.ComponentActivity
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.test.performSemanticsAction
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.ListSortPrefs
import app.hocket.core.Commands
import app.hocket.core.SettingKeys
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
 * The Library's lists on the fake core: the sort chip and sheet (an order applies at once, the
 * direction toggles, both are remembered per list on this device), Artists as a grid in the core's
 * order, the Pinned and Recent groups above the full list, and the Library's Recently added
 * heading without its old "Newest first".
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class LibraryListsTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

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

    private fun start(pins: List<PinEntry> = emptyList()) {
        core = TestCore(startPlaying = false)
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        await("navBar")
        if (pins.isNotEmpty()) core.client.dispatch(Commands.setSetting(SettingKeys.LIBRARY_PINS, LibraryPins.encode(pins)))
        click("navBar.library")
        await("library.grid")
    }

    /** Opens a list from the Library's Browse tiles. */
    private fun openList(list: String) {
        compose.onNodeWithTag("library.grid").performScrollToNode(hasTestTag("library.browse.$list"))
        click("library.browse.$list")
        await("libraryList.$list")
        await("library.sort")
    }

    /** Back to the Library from a list. */
    private fun back() {
        compose.runOnUiThread { compose.activity.onBackPressedDispatcher.onBackPressed() }
        await("library.grid")
    }

    private fun chip(): String = compose.onNodeWithTag("library.sort").fetchSemanticsNode().config[SemanticsProperties.ContentDescription].first()

    private fun selected(tag: String): Boolean = compose.onNodeWithTag(tag).fetchSemanticsNode().config.getOrNull(SemanticsProperties.Selected) == true

    @Suppress("UNCHECKED_CAST")
    private fun closeSheet() {
        // The sheet is a window of its own: back goes to it, as a user's would.
        val global = Class.forName("android.view.WindowManagerGlobal").getMethod("getInstance").invoke(null)
        val views = (global.javaClass.getDeclaredField("mViews").apply { isAccessible = true }.get(global) as List<android.view.View>).toList()
        views.lastOrNull { it !== compose.activity.window.decorView }?.let { v ->
            compose.runOnUiThread {
                v.dispatchKeyEvent(android.view.KeyEvent(android.view.KeyEvent.ACTION_DOWN, android.view.KeyEvent.KEYCODE_BACK))
                v.dispatchKeyEvent(android.view.KeyEvent(android.view.KeyEvent.ACTION_UP, android.view.KeyEvent.KEYCODE_BACK))
            }
        }
        compose.mainClock.advanceTimeBy(1_000)
        compose.waitUntil(5_000) { !exists("library.sortSheet") }
    }

    @Test
    fun theSortSheetAppliesAtOnceAndTheListRemembersIt() {
        start()
        openList("albums")
        assertEquals("Sort: Name, A–Z", chip())
        click("library.sort")
        await("library.sortSheet")
        assertTrue(selected("library.sort.option.name"))
        assertTrue(selected("library.sort.ascending"))
        app.hocket.ui.a11y.A11yChecks.assertAccessible(compose, "sort sheet")

        // Choosing an order applies it in its usual direction; the sheet stays for the direction.
        click("library.sort.option.dateAdded")
        assertEquals("Sort: Recently added, Newest", chip())
        assertTrue(selected("library.sort.option.dateAdded"))
        assertFalse(selected("library.sort.option.name"))
        assertTrue(selected("library.sort.descending"))
        click("library.sort.ascending")
        assertEquals("Sort: Recently added, Oldest", chip())
        assertTrue(selected("library.sort.ascending"))
        closeSheet()

        // Stored for this list only, on this device.
        assertEquals("dateAdded:asc", ListSortPrefs(compose.activity).get("sort.albums"))

        // Leave and come back: the list opens as it was left; another list keeps its own.
        back()
        openList("songs")
        assertEquals("Sort: Title, A–Z", chip())
        back()
        openList("albums")
        assertEquals("Sort: Recently added, Oldest", chip())
    }

    @Test
    fun artistsAreAGridInTheCoresOrder() {
        start()
        openList("artists")
        val inGrid = hasTestTag("library.artist") and !hasAnyAncestor(hasTestTag("library.group.row"))
        fun gridArtists() = compose.onAllNodes(inGrid).fetchSemanticsNodes()
        compose.waitUntil(5_000) { gridArtists().size >= 2 }
        // Two columns: the first two cards side by side.
        val (first, second) = gridArtists()
        assertEquals(first.boundsInRoot.top, second.boundsInRoot.top, 1f)
        assertTrue(second.boundsInRoot.left > first.boundsInRoot.right - 1f)
        val lib = core.fake.library
        fun firstName() = gridArtists().first().config[SemanticsProperties.ContentDescription].first()
        assertEquals("Artist ${lib.artists.minBy { it.name.lowercase() }.name}", firstName().substringBefore(","))

        app.hocket.ui.a11y.A11yChecks.assertAccessible(compose, "artists grid")
        click("library.sort")
        await("library.sortSheet")
        click("library.sort.option.albumCount")
        assertEquals("Sort: Albums, Most", chip())
        compose.waitUntil(5_000) { firstName().endsWith("${lib.artists.maxOf { it.albumCount }} albums") }
        click("library.sort.ascending")
        compose.waitUntil(5_000) { firstName().endsWith("${lib.artists.minOf { it.albumCount }} albums") }
        assertEquals("Sort: Albums, Fewest", chip())
        closeSheet()
    }

    @Test
    fun pinnedThenRecentThenTheWholeList() {
        val lib0 = TestCore(startPlaying = false).fake.library
        val pinned = listOf(lib0.playlists[3], lib0.playlists[0])
        start(pinned.map { PinEntry("playlist", it.id) } + PinEntry("album", lib0.albums[0].id))
        openList("playlists")
        await("library.group.pinned")
        await("library.group.recent")
        fun top(tag: String) = compose.onNodeWithTag(tag).fetchSemanticsNode().boundsInRoot.top
        assertTrue(top("library.group.pinned") < top("library.group.recent"))
        assertTrue(top("library.group.recent") < top("library.group.all"))

        val lib = core.fake.library
        fun rowTop(name: String, after: Float) = compose.onAllNodes(hasText(name, substring = true)).fetchSemanticsNodes().map { it.boundsInRoot.top }.filter { it > after }.min()
        // The pins of this kind, in pin order, between Pinned and Recent.
        val p0 = rowTop(pinned[0].name, top("library.group.pinned"))
        val p1 = rowTop(pinned[1].name, top("library.group.pinned"))
        assertTrue(p0 < p1 && p1 < top("library.group.recent"))
        // Recent: the playlists changed last, pins left out.
        val recent = lib.playlists.filter { it.id !in pinned.map { p -> p.id } }.sortedByDescending { it.changed }.take(3)
        val tops = recent.map { rowTop(it.name, top("library.group.recent")) }
        assertEquals(tops.sorted(), tops)
        assertTrue(tops.all { it < top("library.group.all") })
        // The full list stays complete under its header, sorted by name.
        val full = compose.onAllNodesWithTag("library.playlist").fetchSemanticsNodes()
        assertTrue(full.all { it.boundsInRoot.top > top("library.group.all") })
        assertEquals("Playlist ${lib.playlists.minBy { it.name.lowercase() }.name}", full.first().config[SemanticsProperties.ContentDescription].first().substringBefore(","))
    }

    @Test
    fun recentlyAddedHasNoNewestFirstNote() {
        start()
        compose.onNodeWithTag("library.grid").performScrollToNode(hasText("Recently added"))
        compose.waitForIdle()
        assertTrue(compose.onAllNodesWithText("Recently added").fetchSemanticsNodes().isNotEmpty())
        assertTrue(compose.onAllNodesWithText("Newest first").fetchSemanticsNodes().isEmpty())
    }
}
