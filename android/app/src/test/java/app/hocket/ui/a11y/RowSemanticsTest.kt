package app.hocket.ui.a11y

import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.Modifier
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.toSummary
import app.hocket.ui.DetailNavigator
import app.hocket.ui.LocalDetailNavigator
import app.hocket.ui.TestCore
import app.hocket.ui.components.TrackRow
import app.hocket.ui.components.formatClock
import app.hocket.ui.queue.QueuePanel
import app.hocket.ui.setThemedContent
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/** List rows are one readable item each, with the row menu (and the swipe/drag gestures) as actions. */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class RowSemanticsTest {
    @get:Rule
    val compose = createComposeRule()

    private fun SemanticsNode.actions() = config.getOrNull(SemanticsActions.CustomActions)?.map { it.label } ?: emptyList()
    private fun SemanticsNode.desc() = config.getOrNull(SemanticsProperties.ContentDescription)?.joinToString(" ")

    @Test
    fun aTrackRowReadsAsOneItemWithItsStateAndTheRowMenuAsActions() {
        val core = TestCore()
        val opened = mutableListOf<String>()
        core.start()
        val track = core.fake.library.tracks.first { it.albumId != null && it.artistId != null }.toSummary().copy(loved = true, offline = app.hocket.core.api.OfflineState.None)
        var more = 0
        compose.setThemedContent(core) {
            CompositionLocalProvider(LocalDetailNavigator provides DetailNavigator({ opened += "album:$it" }, { opened += "artist:$it" })) {
                TrackRow(track, onClick = {}, onMore = { more++ }, nowPlaying = true, modifier = Modifier.testTag("row"))
            }
        }
        compose.waitUntil(5_000) { core.client.started.value }
        val row = compose.onNodeWithTag("row").fetchSemanticsNode()
        assertEquals("${track.title}, ${track.artist}, ${formatClock(track.durationMs)}, loved", row.desc())
        assertEquals("playing", row.config[SemanticsProperties.StateDescription])
        assertEquals("Play", row.config[SemanticsActions.OnClick].label)
        assertEquals("Select", row.config[SemanticsActions.OnLongClick].label)
        assertEquals(listOf("Play next", "Add to queue", "Go to album", "Go to artist", "Download", "Rate", "More options"), row.actions())
        // Icons inside the row say nothing of their own (their meaning is in the label/state).
        assertTrue("one row item plus its menu button", row.children.size == 1)
        assertEquals("More options for ${track.title}", row.children.single().desc())

        fun run(label: String) = compose.onNodeWithTag("row").performCustomAction(compose, label)
        run("Play next")
        compose.waitUntil(5_000) { core.client.queue.value.playingNext.any { it.track.id == track.id } }
        run("Add to queue")
        compose.waitUntil(5_000) { core.client.queue.value.playingNext.count { it.track.id == track.id } + core.client.queue.value.upcoming.count { it.track.id == track.id } >= 2 }
        run("Go to album"); run("Go to artist")
        assertEquals(listOf("album:${track.albumId}", "artist:${track.artistId}"), opened)
        run("More options")
        assertEquals(1, more)
        run("Rate")
        compose.waitForIdle()
        compose.onNodeWithTag("rating").performSemanticsAction(SemanticsActions.SetProgress) { it(4f) }
        compose.waitUntil(5_000) { core.fake.library.track(track.id)?.rating == 4u }
        compose.waitForIdle()
        assertTrue("picking a rating closes the dialog", runCatching { compose.onNodeWithTag("rating").assertExists() }.isFailure)
    }

    @Test
    fun readOnlyRowsOfferNoMenuActions() {
        val core = TestCore(startPlaying = false)
        core.start()
        val track = core.fake.library.tracks.first().toSummary().copy(offline = app.hocket.core.api.OfflineState.None)
        compose.setThemedContent(core) { TrackRow(track, onClick = {}, modifier = Modifier.testTag("row")) }
        assertEquals(emptyList<String>(), compose.onNodeWithTag("row").fetchSemanticsNode().actions())
        assertNull(compose.onNodeWithTag("row").fetchSemanticsNode().config.getOrNull(SemanticsProperties.StateDescription))
    }

    @Test
    fun queueRowsRemoveAndMoveWithoutSwipeOrDrag() {
        val core = TestCore()
        compose.setThemedContent(core) { QueuePanel() }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        val queue = core.client.queue.value
        val movable = queue.playingNext + queue.upcoming
        assertTrue(movable.size >= 3)
        fun actionsOf(title: String): List<String> {
            compose.onNodeWithTag("queue.list").performScrollToNode(hasText(title))
            return compose.onAllNodesWithText(title)[0].fetchSemanticsNode().actions()
        }
        assertFalse("the first movable item cannot move up", "Move up" in actionsOf(movable[0].track.title))
        // Two items of the context section: move the first below the second.
        val first = queue.upcoming[0]
        val second = queue.upcoming[1]
        val actions = actionsOf(first.track.title)
        assertTrue(actions.toString(), "Remove from queue" in actions && "Move down" in actions)
        compose.onAllNodesWithText(first.track.title)[0].performCustomAction(compose, "Move down")
        compose.waitUntil(5_000) { core.client.queue.value.upcoming.take(2).map { it.track.id } == listOf(second.track.id, first.track.id) }
        val moved = core.client.queue.value.upcoming[1]
        compose.onNodeWithTag("queue.list").performScrollToNode(hasText(first.track.title))
        compose.onAllNodesWithText(first.track.title)[0].performCustomAction(compose, "Remove from queue")
        compose.waitUntil(5_000) { (core.client.queue.value.playingNext + core.client.queue.value.upcoming).none { it.item.key == moved.item.key } }
        // The drag handle and the swipe background are drawn only.
        assertTrue(compose.onAllNodesWithText("Drag to reorder").fetchSemanticsNodes().isEmpty())
        compose.onNodeWithText("Now playing").assertExists()
    }
}
