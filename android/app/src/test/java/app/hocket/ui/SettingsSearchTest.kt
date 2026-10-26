package app.hocket.ui

import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTextReplacement
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.R
import app.hocket.ui.nav.AppRoot
import app.hocket.ui.screens.settings.SETTINGS_INDEX
import app.hocket.ui.screens.settings.SettingsCategory
import app.hocket.ui.screens.settings.searchSettings
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * Settings search: the index finds settings by their labels and the words people use, the field
 * swaps the categories for the matches, and picking one opens its screen on that setting.
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class SettingsSearchTest {
    @get:Rule
    val compose = createComposeRule()

    private val context = ApplicationProvider.getApplicationContext<android.content.Context>()
    private val titles = mapOf(
        SettingsCategory.Account to R.string.settings_category_account, SettingsCategory.Appearance to R.string.settings_section_appearance,
        SettingsCategory.Playback to R.string.settings_category_playback, SettingsCategory.Audio to R.string.settings_section_audio,
        SettingsCategory.Streaming to R.string.settings_category_streaming, SettingsCategory.Downloads to R.string.settings_category_downloads,
        SettingsCategory.Lyrics to R.string.settings_section_lyrics, SettingsCategory.Library to R.string.settings_category_library,
        SettingsCategory.Battery to R.string.settings_section_battery, SettingsCategory.Connect to R.string.settings_category_connect,
        SettingsCategory.Pebble to R.string.settings_category_pebble, SettingsCategory.Customise to R.string.settings_section_customise,
        SettingsCategory.Backup to R.string.settings_category_backup, SettingsCategory.Advanced to R.string.settings_category_advanced,
        SettingsCategory.About to R.string.settings_section_about,
    )

    private fun search(q: String) = searchSettings(q, { context.getString(it) }, { context.getString(titles.getValue(it)) }).map { it.tag ?: "category:${it.category.id}" }

    @Test
    fun theIndexMatchesLabelsAndTheWordsPeopleUse() {
        assertEquals(emptyList<String>(), search("  "))
        assertTrue(search("GAPLESS").contains("audio.gapless"))
        assertEquals("display.theme", search("dark mode").first())
        assertTrue(search("bass").contains("audio.eq"))
        assertTrue(search("watch").contains("category:pebble"))
        assertEquals("a category comes first", "category:lyrics", search("lyrics").first())
        assertEquals(emptyList<String>(), search("gapless bass"))
        assertEquals("ids are unique", SETTINGS_INDEX.size, SETTINGS_INDEX.map { it.tag }.toSet().size)
    }

    @Test
    fun typingShowsTheMatchesAndPickingOneOpensItsScreenOnIt() {
        val core = TestCore(startPlaying = false)
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        compose.openSettingsFromAccount()
        compose.onNodeWithTag("settings.search").performTextInput("cache size")
        compose.waitForIdle()
        // The categories give way to the results.
        assertTrue(compose.onAllNodesWithTag("settings.category.audio").fetchSemanticsNodes().isEmpty())
        compose.onNodeWithTag("settings.search.results").assertIsDisplayed()
        compose.onNodeWithTag("settings.hit.storage.cacheMaxBytes").assertIsDisplayed().performClick()
        compose.waitUntil(5_000) { compose.onAllNodesWithTag("settings.screen").fetchSemanticsNodes().isNotEmpty() }
        compose.mainClock.advanceTimeBy(1_000)
        compose.waitForIdle()
        val title = compose.onNodeWithTag("settings.title").fetchSemanticsNode().config[SemanticsProperties.Text].joinToString("") { it.text }
        assertEquals(context.getString(R.string.settings_category_downloads), title)
        compose.onNodeWithTag("setting.storage.cacheMaxBytes").assertIsDisplayed()
        // Back returns to the results, the query kept.
        compose.onNodeWithTag("settings.back").performClick()
        compose.waitUntil(5_000) { compose.onAllNodesWithTag("settings.search.results").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun aQueryWithNoMatchesSaysSoAndClearingBringsBackTheCategories() {
        val core = TestCore(startPlaying = false)
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        compose.openSettingsFromAccount()
        compose.onNodeWithTag("settings.search").performTextReplacement("zzzz")
        compose.waitForIdle()
        compose.onNodeWithTag("settings.search.empty").assertIsDisplayed()
        compose.onNodeWithTag("settings.search.clear").performClick()
        compose.waitForIdle()
        compose.onNodeWithTag("settings.category.audio").assertExists()
    }

    @Test
    fun everyIndexedSettingIsOnItsCategoryScreen() {
        val core = TestCore(startPlaying = false)
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        compose.openSettingsFromAccount()
        // Shown only where the device has them, or in the app's own preferences (absent in tests).
        val conditional = setOf("widget.add", "home.shortcuts", "audio.systemEq", "audio.output")
        val missing = ArrayList<String>()
        for ((category, entries) in SETTINGS_INDEX.filter { it.tag !in conditional }.groupBy { it.category }) {
            compose.onNodeWithTag("settings.search").performTextReplacement("")
            compose.onNodeWithTag("settings.category.${category.id}").performScrollTo().performSemanticsAction(androidx.compose.ui.semantics.SemanticsActions.OnClick)
            compose.waitUntil(5_000) { compose.onAllNodesWithTag("settings.screen").fetchSemanticsNodes().isNotEmpty() }
            compose.waitForIdle()
            for (e in entries) if (compose.onAllNodesWithTag("setting.${e.tag}", useUnmergedTree = true).fetchSemanticsNodes().isEmpty()) missing += e.tag
            compose.onNodeWithTag("settings.back").performClick()
            compose.waitUntil(5_000) { compose.onAllNodesWithTag("settings.categories").fetchSemanticsNodes().isNotEmpty() }
        }
        assertEquals("indexed settings without their row", emptyList<String>(), missing)
    }
}
