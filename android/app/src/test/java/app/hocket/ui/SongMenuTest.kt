package app.hocket.ui

import androidx.activity.ComponentActivity
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipe
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.ActionIds
import app.hocket.core.api.ActionDescriptor
import app.hocket.core.api.OfflineState
import app.hocket.core.api.TrackSummary
import app.hocket.ui.components.songMenuRows
import app.hocket.ui.nav.AppRoot
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * The one song menu (hosted at the app level): swiping it away from the full player dismisses the
 * menu and only the menu, it opens at its final height, and it is the same menu from the queue.
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class SongMenuTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private fun displayed(tag: String): Boolean = runCatching { compose.onNodeWithTag(tag).assertIsDisplayed() }.isSuccess
    private fun present(tag: String): Boolean = compose.onAllNodesWithTag(tag).fetchSemanticsNodes().isNotEmpty()
    private fun sheetTop(): Float = compose.onNodeWithTag("nowPlaying.sheet").fetchSemanticsNode().boundsInRoot.top
    private fun contentTop(): Float = runCatching { compose.onNodeWithTag("debug.fakeCoreBanner").fetchSemanticsNode().boundsInRoot.bottom }.getOrDefault(0f)

    private fun openPlayer(core: TestCore) {
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        compose.onNodeWithTag("nowPlaying.sheet").performSemanticsAction(SemanticsActions.Expand)
        compose.waitUntil(5_000) { displayed("player.playPause") }
    }

    private fun openMenu() {
        compose.onNodeWithTag("player.more").performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { present("songMenu.header") }
        compose.waitForIdle()
    }

    private fun swipeMenu(by: Float, millis: Long) {
        compose.onNodeWithTag("songMenu.header").performTouchInput { swipe(start = center, end = center + Offset(0f, by), durationMillis = millis) }
    }

    @Test
    fun swipingTheMenuAwayFromThePlayerDismissesOnlyTheMenu() {
        val core = TestCore()
        openPlayer(core)
        val openTop = sheetTop()
        assertTrue(openTop <= contentTop() + 1f)
        repeat(3) { round ->
            openMenu()
            // A fling on odd rounds, a slow drag past half the screen on even ones.
            if (round % 2 == 0) swipeMenu(1200f, 1_500) else swipeMenu(600f, 80)
            compose.waitUntil(5_000) { !present("songMenu.header") }
            compose.waitForIdle()
            // The player stayed where it was: fully open, not dragged along with the menu.
            assertTrue("player moved on round $round: ${sheetTop()} vs $openTop", sheetTop() <= openTop + 1f)
            assertTrue(displayed("player.playPause"))
        }
    }

    private fun top(tag: String): Float = compose.onNodeWithTag(tag).fetchSemanticsNode().boundsInRoot.top

    /**
     * The player's menu and a queue row's menu are one menu: the same header and star row, the
     * common actions, and the context's own rows (the queue's remove, the player's sleep timer)
     * after them. Removing from the menu removes that entry and closes only the menu.
     */
    @Test
    fun theQueueAndThePlayerOpenTheSameMenuWithTheirExtrasLast() {
        val core = TestCore()
        openPlayer(core)
        openMenu()
        assertTrue(present("rating"))
        assertTrue(top("player.sleep") > top("songMenu.action.${ActionIds.PLAY_NEXT}"))
        assertTrue(!present("songMenu.action.${ActionIds.REMOVE_FROM_QUEUE}"))
        compose.onNodeWithTag("songMenu.header").performTouchInput { swipe(start = center, end = center + Offset(0f, 1200f), durationMillis = 100) }
        compose.waitUntil(5_000) { !present("songMenu.header") }

        compose.onNodeWithTag("player.mode.queue").performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { present("queue.list") }
        val before = core.client.queue.value.let { it.history.size + it.playingNext.size + it.upcoming.size }
        val more = SemanticsMatcher("offers More options") { n -> n.config.getOrNull(SemanticsActions.CustomActions)?.any { it.label == "More options" } == true }
        val row = compose.onAllNodes(more and hasAnyAncestor(hasTestTag("queue.list"))).fetchSemanticsNodes()
            .first { n -> n.config.getOrNull(SemanticsActions.CustomActions)?.any { it.label == "Remove from queue" } == true }
        row.config[SemanticsActions.CustomActions].first { it.label == "More options" }.action()
        compose.waitUntil(5_000) { present("songMenu.header") }
        compose.waitForIdle()
        assertTrue(present("rating"))
        assertTrue(!present("player.sleep"))
        val remove = "songMenu.action.${ActionIds.REMOVE_FROM_QUEUE}"
        assertTrue(top(remove) > top("songMenu.action.${ActionIds.PLAY_NEXT}"))
        compose.onNodeWithTag(remove).performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { !present("songMenu.header") }
        compose.waitUntil(5_000) { core.client.queue.value.let { it.history.size + it.playingNext.size + it.upcoming.size } == before - 1 }
        assertTrue(displayed("player.playPause"))
    }

    /** From a list row the menu is the same sheet: header, stars, the common actions, go-to that navigates. */
    @Test
    fun aListRowOpensTheSameMenu() {
        val core = TestCore(startPlaying = false)
        val album = core.fake.library.albums[2]
        val track = core.fake.library.albumTracks(album.id)[0]
        val opened = mutableListOf<String>()
        core.start()
        compose.setThemedContent(core) {
            androidx.compose.runtime.CompositionLocalProvider(LocalDetailNavigator provides DetailNavigator({ opened += "album:$it" }, { opened += "artist:$it" })) {
                app.hocket.ui.screens.detail.AlbumDetailScreen(androidx.navigation.compose.rememberNavController(), album.id)
            }
        }
        compose.waitUntil(5_000) { compose.onAllNodesWithContentDescription("More options for ${track.title}").fetchSemanticsNodes().isNotEmpty() }
        compose.onAllNodesWithContentDescription("More options for ${track.title}")[0].performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { present("songMenu.header") }
        compose.waitForIdle()
        assertTrue(present("rating"))
        assertTrue(present("songMenu.action.${ActionIds.PLAY_NEXT}"))
        assertTrue(!present("songMenu.action.${ActionIds.REMOVE_FROM_QUEUE}"))
        compose.onNodeWithTag("songMenu.action.${ActionIds.GO_TO_ARTIST}").performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { !present("songMenu.header") }
        assertEquals(listOf("artist:${track.artistId}"), opened)
    }

    @Test
    fun theRowsKeepTheRegistryOrderAndSplitOffTheContextActions() {
        fun d(id: String) = ActionDescriptor(id, id, "x", "c", true, null, true, false)
        val track = TrackSummary("t", "s", "T", albumId = "al", artistId = null, durationMs = 1u, rating = 0u, loved = false, offline = OfflineState.None)
        val ids = listOf(ActionIds.REMOVE_FROM_QUEUE, ActionIds.PLAY_NEXT, ActionIds.rate(5), ActionIds.rate(0), ActionIds.GO_TO_ALBUM, ActionIds.GO_TO_ARTIST, ActionIds.DOWNLOAD)
        val rows = songMenuRows(ids.map(::d), track, canNavigate = true)
        assertEquals(listOf(ActionIds.PLAY_NEXT, ActionIds.GO_TO_ALBUM, ActionIds.DOWNLOAD), rows.common.map { it.id })
        assertEquals(listOf(ActionIds.REMOVE_FROM_QUEUE), rows.context.map { it.id })
        assertEquals(listOf(ActionIds.PLAY_NEXT, ActionIds.DOWNLOAD), songMenuRows(ids.map(::d), track, canNavigate = false).common.map { it.id })
    }

    /** The sheet's height from the frame it first shows: it opens at its final size, never grows under the finger. */
    @Test
    fun theMenuOpensAtItsFinalHeight() {
        val core = TestCore()
        openPlayer(core)
        compose.mainClock.autoAdvance = false
        try {
            compose.onNodeWithTag("player.more").performSemanticsAction(SemanticsActions.OnClick)
            val heights = mutableListOf<Int>()
            repeat(60) {
                compose.mainClock.advanceTimeByFrame()
                compose.onAllNodesWithTag("songMenu").fetchSemanticsNodes().firstOrNull()?.let { heights += it.size.height }
            }
            assertTrue("never shown", heights.isNotEmpty())
            assertTrue("the sheet changed height while opening: ${heights.distinct()}", heights.distinct().size == 1)
        } finally {
            compose.mainClock.autoAdvance = true
        }
    }
}
