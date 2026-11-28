package app.hocket.ui

import app.hocket.core.api.TrackRef

import androidx.activity.ComponentActivity
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipeUp
import androidx.compose.ui.test.swipeDown
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.ActionIds
import app.hocket.core.client.SelectionKind
import app.hocket.ui.components.SelectionDrawer
import androidx.compose.runtime.CompositionLocalProvider
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * The selection drawer: it peeks with the count and the quick actions; opened, the selected songs'
 * menu: "3 songs · …", the rating starting "Mixed" while they differ and rating every selected song,
 * love that loves them all.
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class SelectionDrawerTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private fun present(tag: String) = compose.onAllNodesWithTag(tag).fetchSemanticsNodes().isNotEmpty()

    /** Three songs, rated differently (the fake library's first ones, with the first re-rated); [open]: the drawer opened. */
    private fun selectThree(core: TestCore, open: Boolean = true): List<String> {
        compose.setThemedContent(core) { SelectionDrawer() }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        val ids = core.fake.library.tracks.take(3).map { it.id }
        core.client.dispatch(app.hocket.core.Commands.rateTrack(TrackRef("fake-server", ids[0]), if (core.fake.library.track(ids[1])!!.rating.toInt() == 2) 3 else 2))
        compose.runOnIdle { ids.forEach { core.client.toggleSelected(SelectionKind.Tracks, it) } }
        compose.waitUntil(5_000) { present("selection.action.${ActionIds.PLAY}") }
        compose.waitForIdle()
        if (!open) return ids
        // At the peek the menu is out of reach.
        assertFalse(present("selectionMenu.header"))
        compose.onNodeWithTag("selection.handle").performSemanticsAction(SemanticsActions.Expand)
        compose.waitUntil(5_000) { present("selectionMenu.header") }
        compose.waitForIdle()
        return ids
    }

    private fun ratings(core: TestCore, ids: List<String>) = ids.map { core.fake.library.track(it)!!.rating.toInt() }

    @Test
    fun theRatingStartsMixedAndRatesEverySelectedSong() {
        val core = TestCore(startPlaying = false)
        // The stars are hidden by default: the user has added them.
        core.fake.dispatch(app.hocket.core.Commands.setMenuItem(app.hocket.core.MenuItemIds.RATING, app.hocket.core.api.MenuTier.Shown))
        val ids = selectThree(core)
        assertTrue(compose.onAllNodesWithText("3 songs", substring = true).fetchSemanticsNodes().isNotEmpty())
        val state = compose.onNodeWithTag("rating").fetchSemanticsNode().config[SemanticsProperties.StateDescription]
        assertEquals("Mixed", state)
        compose.onNodeWithTag("rating").performSemanticsAction(SemanticsActions.SetProgress) { it(3f) }
        compose.waitUntil(5_000) { ratings(core, ids) == listOf(3, 3, 3) }
        // A toggle: the menu stays, the selection too.
        assertTrue(present("selectionMenu.header"))
        assertEquals(true, core.client.selection.value.active)
    }

    @Test
    fun theStarsCanClearTheRatingAndLoveLovesThemAll() {
        val core = TestCore(startPlaying = false)
        // The stars are hidden by default: the user has added them.
        core.fake.dispatch(app.hocket.core.Commands.setMenuItem(app.hocket.core.MenuItemIds.RATING, app.hocket.core.api.MenuTier.Shown))
        val ids = selectThree(core)
        compose.onNodeWithTag("rating").performSemanticsAction(SemanticsActions.SetProgress) { it(4f) }
        compose.waitUntil(5_000) { ratings(core, ids) == listOf(4, 4, 4) }
        val clear = compose.onNodeWithTag("rating").fetchSemanticsNode().config[SemanticsActions.CustomActions].first { it.label == "Clear rating" }
        compose.runOnIdle { clear.action() }
        compose.waitUntil(5_000) { ratings(core, ids) == listOf(0, 0, 0) }
        compose.onNodeWithTag("selectionMenu.love").performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { ids.all { core.fake.library.track(it)!!.loved } }
    }

    @Test
    fun playingFromTheOpenDrawerActsOnAllAndClearsTheSelection() {
        val core = TestCore(startPlaying = false)
        val ids = selectThree(core)
        compose.onNodeWithTag("selection.action.${ActionIds.PLAY}").performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { !core.client.selection.value.active }
        compose.waitUntil(5_000) { core.client.queue.value.current?.track?.id in ids }
    }

    @Test
    fun theQuickRowPlaysTheSelectionFromThePeek() {
        val core = TestCore(startPlaying = false)
        val ids = selectThree(core, open = false)
        compose.onNodeWithTag("selection.action.${ActionIds.PLAY}").performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { !core.client.selection.value.active }
        compose.waitUntil(5_000) { core.client.queue.value.current?.track?.id in ids }
        // The drawer slides away once the selection ends.
        compose.waitUntil(5_000) { !present("selection.drawer") }
    }

    @Test
    fun theOpenDrawerClosesToThePeekAndBackEndsTheSelection() {
        val core = TestCore(startPlaying = false)
        selectThree(core)
        compose.onNodeWithTag("selection.handle").performSemanticsAction(SemanticsActions.Collapse)
        compose.waitUntil(5_000) { !present("selectionMenu.header") }
        assertTrue(core.client.selection.value.active)
        compose.runOnIdle { compose.activity.onBackPressedDispatcher.onBackPressed() }
        compose.waitUntil(5_000) { !core.client.selection.value.active }
    }

    @Test
    fun swipingThePeekUpOpensTheDrawer() {
        val core = TestCore(startPlaying = false)
        selectThree(core, open = false)
        compose.onNodeWithTag("selection.peek").performTouchInput { swipeUp(startY = centerY, endY = centerY - 900f, durationMillis = 400) }
        compose.waitUntil(5_000) { present("selectionMenu.header") }
        // And down by the handle, back to the peek (not off the screen).
        compose.onNodeWithTag("selection.handle").performTouchInput { swipeDown(startY = centerY, endY = centerY + 500f, durationMillis = 400) }
        compose.waitUntil(5_000) { !present("selectionMenu.header") }
        assertTrue(core.client.selection.value.active)
    }

    @Test
    fun selectAllInTheQueuePicksEveryUpcomingSong() {
        val core = TestCore(startPlaying = true)
        compose.setThemedContent(core) { app.hocket.ui.queue.QueueList(androidx.compose.ui.Modifier.fillMaxSize()) }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value && core.client.queue.value.entries.size > 2 }
        val q = core.client.queue.value
        val next = q.entries[q.entries.indexOfFirst { it.item.key == q.current?.item?.key } + 1].item.key
        compose.runOnIdle { core.client.toggleSelected(SelectionKind.QueueItems, next) }
        compose.waitUntil(5_000) { compose.onAllNodesWithContentDescription("Select all").fetchSemanticsNodes().isNotEmpty() }
        compose.onAllNodesWithContentDescription("Select all").onFirst().performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { core.client.selection.value.count == q.totalUpcoming.toInt() }
        assertFalse(core.client.selection.value.contains(q.current!!.item.key))
    }

    /** A sub-page left open (Share) as the drawer closes to its peek no longer holds on to back. */
    @Test
    fun closingOnASubPageGoesBackToTheMenuAndBackStillEndsTheSelection() {
        val core = TestCore(startPlaying = false)
        selectThree(core)
        compose.onNodeWithTag("selectionMenu.share").performSemanticsAction(SemanticsActions.OnClick)
        compose.waitForIdle()
        assertFalse(present("selectionMenu.header") && present("selectionMenu.share"))
        compose.onNodeWithTag("selection.handle").performSemanticsAction(SemanticsActions.Collapse)
        compose.waitForIdle()
        compose.runOnIdle { compose.activity.onBackPressedDispatcher.onBackPressed() }
        compose.waitUntil(5_000) { !core.client.selection.value.active }
    }

    /** Opens the drawer over [kind]'s [ids], selected one by one, with a navigator that records where it went. */
    private fun selectAndOpen(core: TestCore, kind: SelectionKind, ids: List<String>, went: MutableList<String> = mutableListOf()) {
        val navigator = DetailNavigator(openAlbum = { went += "album:$it" }, openArtist = { went += "artist:$it" })
        compose.setThemedContent(core) { CompositionLocalProvider(LocalDetailNavigator provides navigator) { SelectionDrawer() } }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        compose.runOnIdle { ids.forEach { core.client.toggleSelected(kind, it) } }
        compose.waitUntil(5_000) { present("selection.handle") }
        compose.waitForIdle()
        compose.onNodeWithTag("selection.handle").performSemanticsAction(SemanticsActions.Expand)
        compose.waitUntil(5_000) { present("selectionMenu.header") }
        compose.waitForIdle()
    }

    private fun shows(text: String) = compose.onAllNodesWithText(text, substring = true).fetchSemanticsNodes().isNotEmpty()

    /** One song picked: the drawer opens the menu its three dots do (its title, Go to album, Song info), not "1 song". */
    @Test
    fun oneSelectedSongOpensItsOwnMenu() {
        val core = TestCore(startPlaying = false)
        val track = core.fake.library.tracks.first()
        val went = mutableListOf<String>()
        selectAndOpen(core, SelectionKind.Tracks, listOf(track.id), went)
        compose.waitUntil(5_000) { shows(track.title) }
        assertFalse(shows("1 song"))
        assertTrue(present("selectionMenu.info"))
        // Going to the album leaves the selection behind.
        compose.onNodeWithTag("selectionMenu.action.${ActionIds.GO_TO_ALBUM}").performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { !core.client.selection.value.active }
        assertEquals(listOf("album:${track.albumId}"), went)
    }

    /** A second song turns it into the songs' menu. */
    @Test
    fun aSecondSongSwitchesToTheSongsMenu() {
        val core = TestCore(startPlaying = false)
        val (a, b) = core.fake.library.tracks.take(2)
        selectAndOpen(core, SelectionKind.Tracks, listOf(a.id))
        compose.waitUntil(5_000) { shows(a.title) && present("selectionMenu.info") }
        compose.runOnIdle { core.client.toggleSelected(SelectionKind.Tracks, b.id) }
        compose.waitUntil(5_000) { shows("2 songs") }
        assertFalse(present("selectionMenu.info"))
        assertFalse(present("selectionMenu.action.${ActionIds.GO_TO_ALBUM}"))
    }

    /** One album picked: the album's own menu (its name, Album info), not a bare list of actions. */
    @Test
    fun oneSelectedAlbumOpensItsOwnMenu() {
        val core = TestCore(startPlaying = false)
        val album = core.fake.library.albums.first()
        selectAndOpen(core, SelectionKind.Albums, listOf(album.id))
        compose.waitUntil(5_000) { shows(album.name) }
        assertTrue(present("selectionMenu.info"))
    }

    /** A page's drawer leaves queue entries to the queue's own. */
    @Test
    fun aPagesDrawerIgnoresAQueueSelection() {
        val core = TestCore(startPlaying = true)
        compose.setThemedContent(core) { SelectionDrawer() }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value && core.client.queue.value.entries.isNotEmpty() }
        compose.runOnIdle { core.client.toggleSelected(SelectionKind.QueueItems, core.client.queue.value.entries.last().item.key) }
        compose.mainClock.advanceTimeBy(1_000)
        compose.waitForIdle()
        assertFalse(present("selection.drawer"))
    }

    private fun bounds(tag: String) = compose.onNodeWithTag(tag, useUnmergedTree = true).fetchSemanticsNode().boundsInRoot

    /** R-20261103-hpkz: "1 selected" sat above the close button instead of beside it. */
    @Test
    fun theCountIsCentredOnTheCloseButton() {
        val core = TestCore(startPlaying = false)
        compose.setThemedContent(core) { SelectionDrawer() }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        compose.runOnIdle { core.client.toggleSelected(SelectionKind.Tracks, core.fake.library.tracks.first().id) }
        compose.waitUntil(5_000) { present("selection.count") }
        compose.waitForIdle()
        val close = compose.onAllNodesWithContentDescription("Clear selection", useUnmergedTree = true).onFirst().fetchSemanticsNode().boundsInRoot
        val count = bounds("selection.count")
        assertEquals("count centred on the close button", close.center.y, count.center.y, 2f)
        assertEquals("count centred in the toolbar", bounds("selection.toolbar").center.y, count.center.y, 2f)
    }

    /**
     * R-20261103-hpkz: over the full player's queue the toolbar sat inside the list's bottom fade,
     * so its lower half faded into the backdrop. Its container is one colour, top to bottom.
     */
    @Test
    fun theToolbarOverThePlayersQueueIsNotFaded() {
        val core = TestCore(startPlaying = true)
        compose.setThemedContent(core) {
            androidx.compose.foundation.layout.Box(androidx.compose.ui.Modifier.fillMaxSize().background(androidx.compose.ui.graphics.Color.Black)) {
                app.hocket.ui.queue.QueueList(androidx.compose.ui.Modifier.fillMaxSize(), controlsAtBottom = true)
            }
        }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value && core.client.queue.value.entries.isNotEmpty() }
        compose.runOnIdle { core.client.toggleSelected(SelectionKind.QueueItems, core.client.queue.value.entries.last().item.key) }
        compose.waitUntil(5_000) { present("selection.toolbar") }
        compose.mainClock.advanceTimeBy(1_000)
        compose.waitForIdle()
        val image = compose.onRoot().captureToImage().asAndroidBitmap()
        val box = bounds("selection.toolbar")
        val x = box.center.x.toInt()
        val top = image.getPixel(x, (box.top + box.height * 0.12f).toInt())
        val bottom = image.getPixel(x, (box.top + box.height * 0.88f).toInt())
        fun channels(c: Int) = listOf(android.graphics.Color.red(c), android.graphics.Color.green(c), android.graphics.Color.blue(c))
        channels(top).zip(channels(bottom)).forEach { (t, b) -> assertEquals("toolbar colour top %08x vs bottom %08x".format(top, bottom), t.toFloat(), b.toFloat(), 6f) }
    }
}
