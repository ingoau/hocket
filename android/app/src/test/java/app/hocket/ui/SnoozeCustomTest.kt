package app.hocket.ui

import androidx.activity.ComponentActivity
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performSemanticsAction
import androidx.navigation.compose.rememberNavController
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.ActionIds
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import java.time.LocalDateTime
import java.time.ZoneId

/** Snooze's Custom row: a date, then a time, then the song is snoozed until that moment. */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], qualifiers = "w411dp-h891dp-xxhdpi")
class SnoozeCustomTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    @Test
    fun customSnoozesUntilThePickedDateAndTime() {
        val core = TestCore(startPlaying = false)
        val lib = core.fake.library
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        val trackId = lib.playlistTracks.getValue("pl0").first()
        val title = lib.track(trackId)!!.title
        compose.setThemedContent(core) {
            CompositionLocalProvider(LocalDetailNavigator provides DetailNavigator({}, {}, {})) {
                app.hocket.ui.screens.detail.PlaylistDetailScreen(rememberNavController(), "pl0")
            }
        }
        compose.waitUntil(5_000) { compose.onAllNodesWithContentDescription("More options for $title").fetchSemanticsNodes().isNotEmpty() }
        settle()
        compose.onAllNodesWithContentDescription("More options for $title")[0].performSemanticsAction(SemanticsActions.OnClick)
        awaitTag("songMenu.more")
        settle()
        compose.onNodeWithTag("songMenu.more").performSemanticsAction(SemanticsActions.OnClick)
        settle()
        compose.onNodeWithTag("songMenu.action.${ActionIds.SNOOZE}").performSemanticsAction(SemanticsActions.OnClick)
        awaitTag("songMenu.snooze.custom")
        settle()
        compose.onNodeWithTag("songMenu.snooze.custom").performSemanticsAction(SemanticsActions.OnClick)
        awaitTag("songMenu.snoozeUntil.next")
        // A week from now is preselected; take it, then the time it offers.
        val expected = LocalDateTime.now().plusDays(7).withSecond(0).withNano(0)
        compose.onNodeWithTag("songMenu.snoozeUntil.next").performSemanticsAction(SemanticsActions.OnClick)
        awaitTag("songMenu.snoozeUntil.ok")
        compose.onNodeWithTag("songMenu.snoozeUntil.ok").performSemanticsAction(SemanticsActions.OnClick)
        settle()
        compose.waitUntil(5_000) { core.client.snoozes.value.any { it.trackId == trackId } }
        val until = core.client.snoozes.value.first { it.trackId == trackId }.until
        val want = expected.atZone(ZoneId.systemDefault()).toInstant().toEpochMilli().toDouble()
        assertTrue("snoozed until a moment, not indefinitely", until != null)
        // Within a minute: the clock may tick over between the picker opening and this line.
        assertEquals(want, until!!, 60_000.0)
    }

    private fun settle() {
        compose.mainClock.advanceTimeBy(1_200)
        compose.waitForIdle()
    }

    private fun awaitTag(tag: String) {
        compose.waitUntil(5_000) { compose.onAllNodesWithTag(tag).fetchSemanticsNodes().isNotEmpty() }
        compose.waitForIdle()
    }
}
