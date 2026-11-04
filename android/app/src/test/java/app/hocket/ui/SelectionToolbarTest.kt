package app.hocket.ui

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
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.ActionIds
import app.hocket.core.client.SelectionKind
import app.hocket.ui.components.SelectionToolbar
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * The selected songs' menu (More on the selection toolbar): "3 songs · …", the rating starting
 * "Mixed" while they differ and rating every selected song, love that loves them all.
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class SelectionToolbarTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private fun present(tag: String) = compose.onAllNodesWithTag(tag).fetchSemanticsNodes().isNotEmpty()

    /** Three songs, rated differently (the fake library's first ones, with the first re-rated). */
    private fun selectThree(core: TestCore): List<String> {
        compose.setThemedContent(core) { SelectionToolbar() }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        val ids = core.fake.library.tracks.take(3).map { it.id }
        core.client.dispatch(app.hocket.core.Commands.rateTrack(ids[0], if (core.fake.library.track(ids[1])!!.rating.toInt() == 2) 3 else 2))
        compose.runOnIdle { ids.forEach { core.client.toggleSelected(SelectionKind.Tracks, it) } }
        compose.waitUntil(5_000) { present("selection.more") }
        compose.onNodeWithTag("selection.more").performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { present("selectionMenu.header") }
        compose.waitForIdle()
        return ids
    }

    private fun ratings(core: TestCore, ids: List<String>) = ids.map { core.fake.library.track(it)!!.rating.toInt() }

    @Test
    fun theRatingStartsMixedAndRatesEverySelectedSong() {
        val core = TestCore(startPlaying = false)
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
    fun playingFromTheMenuActsOnAllAndClearsTheSelection() {
        val core = TestCore(startPlaying = false)
        val ids = selectThree(core)
        compose.onNodeWithTag("selectionMenu.action.${ActionIds.PLAY}").performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { !core.client.selection.value.active }
        compose.waitUntil(5_000) { core.client.queue.value.current?.track?.id in ids }
    }

    private fun bounds(tag: String) = compose.onNodeWithTag(tag, useUnmergedTree = true).fetchSemanticsNode().boundsInRoot

    /** R-20261103-hpkz: "1 selected" sat above the close button instead of beside it. */
    @Test
    fun theCountIsCentredOnTheCloseButton() {
        val core = TestCore(startPlaying = false)
        compose.setThemedContent(core) { SelectionToolbar() }
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
        compose.waitUntil(5_000) { core.client.started.value && core.client.queue.value.upcoming.isNotEmpty() }
        compose.runOnIdle { core.client.toggleSelected(SelectionKind.QueueItems, core.client.queue.value.upcoming.first().item.key) }
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
