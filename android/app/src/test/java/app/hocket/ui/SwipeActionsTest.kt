package app.hocket.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.assertIsOn
import androidx.compose.ui.test.filterToOne
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.onChildren
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipeLeft
import androidx.compose.ui.test.swipeRight
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.ActionIds
import app.hocket.core.Commands
import app.hocket.core.SettingKeys
import app.hocket.core.SwipeOptions
import app.hocket.core.toSummary
import app.hocket.ui.components.TrackRow
import app.hocket.ui.queue.QueuePanel
import app.hocket.ui.screens.settings.CustomiseSettingsScreen
import androidx.navigation.compose.rememberNavController
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/** Swipe actions on song rows: the queue's and every other list's, as set in Customise. */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class SwipeActionsTest {
    @get:Rule
    val compose = createComposeRule()

    private fun TestCore.set(key: String, id: String) {
        client.dispatch(Commands.setSetting(key, "\"$id\""))
        compose.waitUntil(5_000) { client.settings.value[key]?.value == "\"$id\"" }
    }

    @Test
    fun songRowsSwipeToPlayNextAndLaterByDefault() {
        val core = TestCore()
        val (a, b) = core.fake.library.tracks.takeLast(2)
        compose.setThemedContent(core) {
            Column {
                TrackRow(a.toSummary(), onClick = {}, onMore = {}, modifier = Modifier.testTag("row.a"))
                TrackRow(b.toSummary(), onClick = {}, onMore = {}, modifier = Modifier.testTag("row.b"))
            }
        }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        compose.onNodeWithTag("row.a").performTouchInput { swipeRight() }
        compose.waitUntil(5_000) { core.client.queue.value.playingNext.firstOrNull()?.track?.id == a.id }
        compose.onNodeWithTag("row.b").performTouchInput { swipeLeft() }
        compose.waitUntil(5_000) { core.client.queue.value.playingNext.lastOrNull()?.track?.id == b.id }
    }

    @Test
    fun aShortSwipeOrNoneDoesNothingAndLoveToggles() {
        val core = TestCore()
        val track = core.fake.library.tracks.first { !it.loved }
        compose.setThemedContent(core) {
            TrackRow(track.toSummary(), onClick = {}, onMore = {}, modifier = Modifier.testTag("row"))
        }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        val next0 = core.client.queue.value.playingNext.size
        // Short of the threshold: springs back, nothing runs.
        compose.onNodeWithTag("row").performTouchInput { swipeRight(startX = left + 10f, endX = left + width * 0.15f) }
        compose.waitForIdle()
        assertEquals(next0, core.client.queue.value.playingNext.size)
        // "None" on a side: that side does not move at all.
        core.set(SettingKeys.SWIPE_LIST_START_TO_END, SwipeOptions.NONE)
        compose.onNodeWithTag("row").performTouchInput { swipeRight() }
        compose.waitForIdle()
        assertEquals(next0, core.client.queue.value.playingNext.size)
        // Love through the registry.
        core.set(SettingKeys.SWIPE_LIST_END_TO_START, ActionIds.LOVE)
        compose.onNodeWithTag("row").performTouchInput { swipeLeft() }
        compose.waitUntil(5_000) { core.fake.library.track(track.id)!!.loved }
        assertEquals(next0, core.client.queue.value.playingNext.size)
    }

    @Test
    fun queueRowsSwipeToRemoveByDefaultAndFollowTheirOwnSetting() {
        val core = TestCore()
        compose.setThemedContent(core) { QueuePanel() }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        compose.waitForIdle()
        val gone = core.client.queue.value.upcoming.first()
        compose.onNodeWithTag("queue.list").performScrollToNode(hasTestTag("queue.row.${gone.item.key}"))
        compose.onNodeWithTag("queue.row.${gone.item.key}").performTouchInput { swipeLeft() }
        compose.waitUntil(5_000) { core.client.queue.value.upcoming.none { it.item.key == gone.item.key } }
        // The queue's own setting: swiping right now loves, and the list's setting does not apply here.
        core.set(SettingKeys.SWIPE_QUEUE_START_TO_END, ActionIds.LOVE)
        core.set(SettingKeys.SWIPE_LIST_START_TO_END, ActionIds.PLAY_NEXT)
        compose.waitForIdle()
        val loved = core.client.queue.value.upcoming.first { !it.track.loved }
        val count = core.client.queue.value.let { it.playingNext.size + it.upcoming.size }
        compose.onNodeWithTag("queue.list").performScrollToNode(hasTestTag("queue.row.${loved.item.key}"))
        compose.onNodeWithTag("queue.row.${loved.item.key}").performTouchInput { swipeRight() }
        compose.waitUntil(5_000) { core.fake.library.track(loved.track.id)!!.loved }
        assertEquals(count, core.client.queue.value.let { it.playingNext.size + it.upcoming.size })
    }

    @Test
    fun theCurrentTrackIsNeverSwipedAway() {
        val core = TestCore()
        compose.setThemedContent(core) { QueuePanel() }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        compose.waitForIdle()
        val current = core.client.queue.value.current!!
        compose.onNodeWithTag("queue.row.${current.item.key}").performTouchInput { swipeLeft() }
        compose.waitForIdle()
        assertEquals(current.item.key, core.client.queue.value.current?.item?.key)
    }

    @Test
    fun customiseSetsEachDirectionPerSurface() {
        val core = TestCore(startPlaying = false)
        compose.setThemedContent(core) { CustomiseSettingsScreen(rememberNavController()) }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        compose.onNodeWithText("Swipe actions").assertExists()
        // Lists never offer "Remove from queue"; the queue does.
        val list = compose.onNodeWithTag("customise.${SettingKeys.SWIPE_LIST_END_TO_START}").performScrollTo()
        assertFalse(list.onChildren().fetchSemanticsNodes().any { it.config.getOrNull(SemanticsProperties.Text)?.joinToString() == "Remove from queue" })
        compose.onNodeWithTag("customise.${SettingKeys.SWIPE_QUEUE_END_TO_START}").performScrollTo().onChildren().filterToOne(hasText("Remove from queue")).assertIsOn()
        list.onChildren().filterToOne(hasText("Add to playlist")).performClick()
        compose.waitUntil(5_000) { core.client.settings.value[SettingKeys.SWIPE_LIST_END_TO_START]?.value == "\"${ActionIds.ADD_TO_PLAYLIST}\"" }
        compose.onNodeWithTag("customise.${SettingKeys.SWIPE_QUEUE_START_TO_END}").performScrollTo().onChildren().filterToOne(hasText("None")).performClick()
        compose.waitUntil(5_000) { core.client.settings.value[SettingKeys.SWIPE_QUEUE_START_TO_END]?.value == "\"${SwipeOptions.NONE}\"" }
    }
}
