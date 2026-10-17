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

    private fun height(tag: String): Float = compose.onNodeWithTag(tag).fetchSemanticsNode().boundsInRoot.height

    /**
     * A quick swipe settles all the way (never stuck part way): a short fast flick up opens the rest;
     * then, open, one long fast swipe down closes the rest and carries on to dismiss the menu.
     */
    @Test
    fun quickSwipesSettleAllTheWayAndAFlickDownStillDismisses() {
        val core = TestCore()
        openPlayer(core)
        repeat(3) {
            openMenu()
            swipeMenu(-200f, 60)
            compose.waitForIdle()
            assertTrue("stuck part way after a quick flick up", !present("songMenu.more") && present("songMenu.showLess"))
            swipeMenu(1400f, 160)
            compose.waitUntil(5_000) { !present("songMenu.header") }
            compose.waitForIdle()
            assertTrue(displayed("player.playPause"))
        }
    }

    /**
     * Swiping up on the menu opens the rest of it with the finger: part way, the rest shows part of
     * its height and More is still folding away; let go short of half and it settles back; a full
     * swipe opens it, and a swipe down (the list at its top) closes it again before the sheet moves.
     */
    @Test
    fun swipingUpOpensTheRestOfTheMenuWithTheFinger() {
        val core = TestCore()
        openPlayer(core)
        openMenu()
        assertTrue(!present("songMenu.showLess"))
        val moreHeight = height("songMenu.more")
        // Held part way: both the folding More and the opening rest are there, More shorter than it was.
        compose.mainClock.autoAdvance = false
        compose.onNodeWithTag("songMenu.header").performTouchInput { down(center); moveBy(Offset(0f, -40f)); moveBy(Offset(0f, -40f)); moveBy(Offset(0f, -40f)) }
        compose.mainClock.advanceTimeBy(100)
        assertTrue(present("songMenu.showLess"))
        assertTrue(present("songMenu.more"))
        assertTrue("More didn't fold with the drag", height("songMenu.more") < moreHeight - 1f)
        // Let go slowly, short of half: it settles closed again.
        compose.onNodeWithTag("songMenu.header").performTouchInput { up() }
        compose.mainClock.autoAdvance = true
        compose.waitForIdle()
        assertTrue(!present("songMenu.showLess"))
        assertTrue(present("songMenu.more"))
        // A full swipe up opens it.
        swipeMenu(-900f, 600)
        compose.waitUntil(5_000) { !present("songMenu.more") }
        assertTrue(present("songMenu.showLess"))
        // A short flick down closes the rest, and the menu stays.
        swipeMenu(300f, 100)
        compose.waitForIdle()
        assertTrue(present("songMenu.header"))
        assertTrue(!present("songMenu.showLess"))
        assertTrue(present("songMenu.more"))
    }

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
        // The destructive row is under More.
        assertTrue(!present(remove))
        compose.onNodeWithTag("songMenu.more").performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { present(remove) }
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
        // Go to album is in the short list; Go to artist under More.
        assertTrue(present("songMenu.action.${ActionIds.GO_TO_ALBUM}"))
        compose.onNodeWithTag("songMenu.more").performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { present("songMenu.action.${ActionIds.GO_TO_ARTIST}") }
        compose.onNodeWithTag("songMenu.action.${ActionIds.GO_TO_ARTIST}").performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { !present("songMenu.header") }
        assertEquals(listOf("artist:${track.artistId}"), opened)
    }

    /**
     * Add to playlist from the song menu: the song's playlists come ticked, the one holding it twice
     * with its ×2 badge; unticking that one asks, "Remove one" leaves one copy, and Done applies it.
     */
    @Test
    fun addToPlaylistRemovesOneOfTwoCopies() {
        val core = TestCore(startPlaying = false)
        val lib = core.fake.library
        val track = lib.playlistTracks.getValue("pl0").first()
        val album = lib.track(track)!!.albumId!!
        core.start()
        compose.setThemedContent(core) {
            androidx.compose.runtime.CompositionLocalProvider(LocalDetailNavigator provides DetailNavigator({}, {})) {
                app.hocket.ui.screens.detail.AlbumDetailScreen(androidx.navigation.compose.rememberNavController(), album)
            }
        }
        val title = lib.track(track)!!.title
        compose.waitUntil(5_000) { compose.onAllNodesWithContentDescription("More options for $title").fetchSemanticsNodes().isNotEmpty() }
        compose.onAllNodesWithContentDescription("More options for $title")[0].performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { present("songMenu.header") }
        compose.onNodeWithTag("songMenu.action.${ActionIds.ADD_TO_PLAYLIST}").performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { present("picker.row.pl2") }
        compose.waitForIdle()
        // The badge is part of its row's merged semantics ("2 copies").
        fun badge(id: String) = compose.onAllNodesWithTag("picker.badge.$id", useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty()
        assertTrue(badge("pl2"))
        assertTrue(!badge("pl0"))
        compose.onNodeWithTag("picker.row.pl2").performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { present("picker.removeOne") }
        compose.onNodeWithTag("picker.removeOne").performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { !present("picker.removeOne") }
        assertTrue(!badge("pl2"))
        compose.onNodeWithTag("picker.done").performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { lib.playlistTracks.getValue("pl2").count { it == track } == 1 }
        assertEquals(1, lib.playlistTracks.getValue("pl0").count { it == track })
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
