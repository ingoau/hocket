package app.hocket.ui.a11y

import androidx.activity.ComponentActivity
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.unit.dp
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.ui.TestCore
import app.hocket.ui.nav.AppRoot
import app.hocket.ui.screens.settings.SettingsCategory
import app.hocket.ui.setThemedContent
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * [A11yChecks] over every main screen of the app on a phone: labels on everything actionable, 48 dp
 * targets, no redundant "button" wording, no stacked duplicate controls.
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class AccessibilityChecksTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private fun displayed(tag: String) = runCatching { compose.onNodeWithTag(tag).assertIsDisplayed() }.isSuccess

    private fun start(): TestCore {
        val core = TestCore()
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        compose.waitForIdle()
        return core
    }

    private fun check(what: String) {
        compose.waitForIdle()
        A11yChecks.assertAccessible(compose, what)
    }

    /** The checker itself: it must catch what ATF would. */
    @Test
    fun theCheckerFlagsUnlabelledSmallAndDuplicateControls() {
        compose.setContent {
            Column {
                Box(Modifier.size(20.dp).clickable { }) // no label (Compose extends its touch area to 48 dp)
                Box(Modifier.size(20.dp).semantics { contentDescription = "tiny"; onClick { true } }) // too small: no pointer input to extend
                IconButton(onClick = {}) { Icon(Icons.Filled.Add, "Add button") } // redundant "button"
                Box(Modifier.size(48.dp)) {
                    Box(Modifier.size(48.dp).semantics { contentDescription = "one" }.clickable { })
                    Box(Modifier.size(48.dp).semantics { contentDescription = "two" }.clickable { })
                }
            }
        }
        val problems = A11yChecks.issues(compose).map { it.problem }
        assertTrue(problems.toString(), "no label" in problems)
        assertTrue(problems.toString(), problems.any { it.startsWith("target below 24dp") })
        assertTrue(problems.toString(), "redundant description" in problems)
        assertTrue(problems.toString(), "duplicate clickable bounds" in problems)
    }

    @Test
    fun homeLibraryAndSearchPassTheChecks() {
        start()
        check("home")
        compose.onNodeWithTag("navBar.library").performClick()
        compose.waitForIdle()
        check("library: albums")
        for (tab in listOf("Artists", "Playlists", "Genres", "Tracks")) {
            val node = runCatching { compose.onNodeWithText(tab).performClick() }
            if (node.isSuccess) check("library: $tab")
        }
        compose.onNodeWithTag("navBar.search").performClick()
        check("search")
    }

    @Test
    fun albumDetailPassesTheChecks() {
        start()
        compose.onNodeWithTag("navBar.library").performClick()
        compose.waitForIdle()
        compose.onAllNodesWithTag("library.album")[0].performClick()
        compose.waitUntil(5_000) { displayed("detail.header") }
        check("album detail")
    }

    @Test
    fun theNowPlayingSheetAndItsPagesPassTheChecks() {
        start()
        check("mini player over home")
        compose.onNodeWithTag("miniPlayer").performClick()
        compose.waitUntil(5_000) { displayed("player.playPause") }
        check("now playing")
        compose.onNodeWithTag("player.tab.1").performClick()
        compose.waitForIdle()
        check("queue")
        // The lyrics page runs a frame loop while shown: drive the clock by hand from here.
        compose.mainClock.autoAdvance = false
        compose.onNodeWithTag("player.tab.2").performClick()
        repeat(60) { compose.mainClock.advanceTimeByFrame() }
        A11yChecks.assertAccessible(compose, "lyrics")
    }

    @Test
    fun everySettingsScreenPassesTheChecks() {
        start()
        compose.onNodeWithTag("navBar.settings").performClick()
        compose.waitForIdle()
        check("settings categories")
        var visited = 0
        for (category in SettingsCategory.entries) {
            // A semantics click: a row scrolled under the mini player would hand a touch to the player.
            compose.onNodeWithTag("settings.category.${category.id}").performScrollTo().performSemanticsAction(SemanticsActions.OnClick)
            compose.waitForIdle()
            check("settings: ${category.id}")
            visited++
            runCatching { compose.onNodeWithTag("settings.back").performClick() }.onFailure { throw AssertionError("no back on ${category.id}", it) }
            compose.waitForIdle()
        }
        assertEquals(SettingsCategory.entries.size, visited)
    }

    private fun open(settingsCategory: String, row: String) {
        click("navBar.settings")
        compose.onNodeWithTag("settings.category.$settingsCategory").performScrollTo().performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { displayed("setting.$row") || runCatching { compose.onNodeWithTag("setting.$row").performScrollTo() }.isSuccess }
        compose.onNodeWithTag("setting.$row").performScrollTo().performSemanticsAction(SemanticsActions.OnClick)
        compose.waitForIdle()
    }

    private fun click(tag: String) {
        compose.onNodeWithTag(tag).performSemanticsAction(SemanticsActions.OnClick)
        compose.waitForIdle()
    }

    @Test
    fun downloadsFiltersAndStatsPassTheChecks() {
        start()
        open("downloads", "open.downloads")
        check("downloads")
        compose.activityRule.scenario.onActivity { it.onBackPressedDispatcher.onBackPressed() }
        compose.waitForIdle()
        compose.activityRule.scenario.onActivity { it.onBackPressedDispatcher.onBackPressed() }
        compose.waitForIdle()
        compose.onNodeWithTag("settings.category.library").performScrollTo().performSemanticsAction(SemanticsActions.OnClick)
        compose.waitForIdle()
        compose.onNodeWithTag("setting.open.filters").performSemanticsAction(SemanticsActions.OnClick)
        compose.waitForIdle()
        check("filters")
        compose.activityRule.scenario.onActivity { it.onBackPressedDispatcher.onBackPressed() }
        compose.waitForIdle()
        compose.onNodeWithTag("setting.open.stats").performSemanticsAction(SemanticsActions.OnClick)
        compose.waitForIdle()
        check("stats")
    }

    private fun expanded() {
        if (!displayed("player.playPause")) compose.onNodeWithTag("nowPlaying.sheet").performSemanticsAction(SemanticsActions.Expand)
        compose.waitUntil(5_000) { displayed("player.playPause") }
    }

    private fun back() {
        compose.activityRule.scenario.onActivity { it.onBackPressedDispatcher.onBackPressed() }
        compose.waitForIdle()
    }

    @Test
    fun theRowMenuAndThePlayerSheetsPassTheChecks() {
        start()
        expanded()
        // The track's menu (ActionSheet), then the sleep timer and the Connect picker.
        compose.onNodeWithTag("player.more").performSemanticsAction(SemanticsActions.OnClick)
        compose.waitForIdle()
        check("track menu")
        back()
        expanded()
        compose.onNodeWithTag("player.sleep").performSemanticsAction(SemanticsActions.OnClick)
        compose.waitForIdle()
        check("sleep timer")
        back()
        expanded()
        compose.onNodeWithTag("player.connect").performSemanticsAction(SemanticsActions.OnClick)
        compose.waitForIdle()
        check("connect picker")
    }

    @Test
    fun theSetupScreenPassesTheChecks() {
        val core = TestCore(startWithServer = false)
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        compose.waitUntil(5_000) { displayed("setup.url") }
        check("server setup")
    }
}
