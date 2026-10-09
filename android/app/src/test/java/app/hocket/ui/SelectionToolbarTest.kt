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

/** Bulk rating from the selection toolbar: one "Set rating…" entry whose dialog rates every selected track. */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class SelectionToolbarTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private fun present(tag: String) = compose.onAllNodesWithTag(tag).fetchSemanticsNodes().isNotEmpty()

    private fun selectThree(core: TestCore): List<String> {
        compose.setThemedContent(core) { SelectionToolbar() }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        val ids = core.fake.library.tracks.take(3).map { it.id }
        compose.runOnIdle { ids.forEach { core.client.toggleSelected(SelectionKind.Tracks, it) } }
        compose.waitUntil(5_000) { present("selection.action.${ActionIds.rate(5)}") || compose.onAllNodesWithContentDescription("More options").fetchSemanticsNodes().isNotEmpty() }
        return ids
    }

    private fun openSetRating() {
        if (!present("selection.action.${ActionIds.rate(5)}")) compose.onAllNodesWithContentDescription("More options").onFirst().performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { present("selection.action.${ActionIds.rate(5)}") }
        val entry = compose.onNodeWithTag("selection.action.${ActionIds.rate(5)}").fetchSemanticsNode().config
        val text = (entry.getOrElseNullable(SemanticsProperties.Text) { null }?.joinToString { it.text } ?: "") + (entry.getOrElseNullable(SemanticsProperties.ContentDescription) { null }?.joinToString() ?: "")
        // The fixed-value rateN entries are gone: the one entry sets any rating.
        assertTrue(text, text.contains("Set rating…"))
        assertTrue(compose.onAllNodesWithText("Rate 5 stars").fetchSemanticsNodes().isEmpty())
        compose.onNodeWithTag("selection.action.${ActionIds.rate(5)}").performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { present("rating") }
    }

    private fun ratings(core: TestCore, ids: List<String>) = ids.map { core.fake.library.track(it)!!.rating.toInt() }

    @Test
    fun setRatingRatesEverySelectedTrack() {
        val core = TestCore(startPlaying = false)
        val ids = selectThree(core)
        openSetRating()
        compose.onNodeWithTag("rating").performSemanticsAction(SemanticsActions.SetProgress) { it(3f) }
        compose.waitUntil(5_000) { ratings(core, ids) == listOf(3, 3, 3) }
        compose.waitUntil(5_000) { !present("rating") }
        assertEquals(false, core.client.selection.value.active)
    }

    @Test
    fun theDialogCanClearTheRating() {
        val core = TestCore(startPlaying = false)
        val ids = selectThree(core)
        openSetRating()
        compose.onNodeWithTag("rating").performSemanticsAction(SemanticsActions.SetProgress) { it(4f) }
        compose.waitUntil(5_000) { ratings(core, ids) == listOf(4, 4, 4) }
        compose.runOnIdle { ids.forEach { core.client.toggleSelected(SelectionKind.Tracks, it) } }
        openSetRating()
        compose.onNodeWithTag("rating.clear").performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { ratings(core, ids) == listOf(0, 0, 0) }
    }
}
