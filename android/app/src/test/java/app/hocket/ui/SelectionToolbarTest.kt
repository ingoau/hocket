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
}
