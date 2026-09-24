package app.hocket.ui

import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.ui.nav.AppRoot
import app.hocket.ui.screens.settings.SettingsCategory
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * Settings are a list of categories, each on its own screen. Walks every category and checks its
 * rows, and that together they hold exactly the rows of the old single settings page (plus the
 * settings that page never showed), each in one place only.
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class SettingsCategoriesTest {
    @get:Rule
    val compose = createComposeRule()

    /**
     * Every row of the old single page, by id. Rows that opened a sub-screen are `open.<screen>`;
     * those screens are now categories of their own (audio, transcoding, connect, customise, about)
     * or rows inside one (downloads, filters, stats).
     */
    private val oldPage = setOf(
        "sync.master", "server.info", "server.syncNow", "server.fullSync", "server.remove",
        "display.theme", "display.accent", "display.artworkColour", "display.animatedBackground",
        "open.audio", "open.transcoding", "open.connect",
        "queue.savedCap", "queue.autoplay",
        "open.downloads", "storage.clearCache", "storage.warnThreshold",
        "battery.saver", "battery.autoEngage",
        "lyrics.external",
        "ratings.loveThreshold",
        "open.customise", "open.filters", "open.stats",
        "backup.includeSecrets", "backup.export", "backup.import", "backup.diagnostics",
        "open.about",
    )

    /** Registry settings the old page did not show, now in their category. */
    private val added = setOf("queue.mode", "sleep.defaultMinutes", "sleep.stopAtEndOfTrack", "downloads.wifiOnly", "lyrics.defaultOffsetMs", "lyrics.showTranslations")

    /** Categories that open one of the older sub-screens: the old page's row for it. */
    private val screenCategories = mapOf(
        SettingsCategory.Audio to "open.audio", SettingsCategory.Streaming to "open.transcoding", SettingsCategory.Connect to "open.connect",
        SettingsCategory.Customise to "open.customise", SettingsCategory.About to "open.about",
    )

    private val expected = mapOf(
        SettingsCategory.Account to setOf("sync.master", "server.info", "server.syncNow", "server.fullSync", "server.remove"),
        SettingsCategory.Appearance to setOf("display.theme", "display.accent", "display.artworkColour", "display.animatedBackground"),
        SettingsCategory.Playback to setOf("queue.mode", "queue.savedCap", "queue.autoplay", "sleep.defaultMinutes", "sleep.stopAtEndOfTrack"),
        SettingsCategory.Downloads to setOf("open.downloads", "downloads.wifiOnly", "storage.clearCache", "storage.warnThreshold"),
        SettingsCategory.Lyrics to setOf("lyrics.external", "lyrics.defaultOffsetMs", "lyrics.showTranslations"),
        SettingsCategory.Library to setOf("ratings.loveThreshold", "open.filters", "open.stats"),
        SettingsCategory.Battery to setOf("battery.saver", "battery.autoEngage"),
        SettingsCategory.Backup to setOf("backup.includeSecrets", "backup.export", "backup.import", "backup.diagnostics"),
    )

    private val settingRow = SemanticsMatcher("a settings row") { node ->
        node.config.getOrElseNullable(SemanticsProperties.TestTag) { null }?.startsWith("setting.") == true
    }

    private fun rowsOnScreen(): List<String> =
        compose.onAllNodes(settingRow).fetchSemanticsNodes().map { it.config[SemanticsProperties.TestTag].removePrefix("setting.") }

    @Test
    fun everyCategoryOpensItsScreenAndTogetherTheyHoldEveryOldSetting() {
        val core = TestCore(startPlaying = false)
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        compose.waitUntil(5_000) { runCatching { compose.onNodeWithTag("navBar.settings").assertExists() }.isSuccess }
        compose.onNodeWithTag("navBar.settings").performClick()
        compose.waitUntil(5_000) { runCatching { compose.onNodeWithTag("settings.categories").assertExists() }.isSuccess }

        val seen = ArrayList<String>()
        for (category in SettingsCategory.entries) {
            compose.onNodeWithTag("settings.category.${category.id}").performScrollTo().assertIsDisplayed().performClick()
            compose.waitUntil(5_000) { runCatching { compose.onNodeWithTag("settings.screen").assertExists() }.isSuccess }
            val rows = rowsOnScreen()
            val old = screenCategories[category]
            if (old != null) {
                assertTrue("${category.id} is one of the older sub-screens and has no page rows of its own: $rows", rows.isEmpty())
                seen += old
            } else {
                assertEquals("rows of ${category.id}", expected.getValue(category), rows.toSet())
                assertEquals("no row twice on ${category.id}", rows.size, rows.toSet().size)
                seen += rows
            }
            compose.onNodeWithTag("settings.back").performClick()
            compose.waitUntil(5_000) { runCatching { compose.onNodeWithTag("settings.categories").assertExists() }.isSuccess }
        }
        assertEquals("each setting is in exactly one category", seen.size, seen.toSet().size)
        assertEquals("nothing from the old page is dropped", emptySet<String>(), oldPage - seen.toSet())
        assertEquals("nothing unexpected is added", added, seen.toSet() - oldPage)
    }

    @Test
    fun signingOutFromAccountStillAsksFirst() {
        val core = TestCore(startPlaying = false)
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        compose.waitUntil(5_000) { runCatching { compose.onNodeWithTag("navBar.settings").assertExists() }.isSuccess }
        compose.onNodeWithTag("navBar.settings").performClick()
        compose.waitUntil(5_000) { runCatching { compose.onNodeWithTag("settings.category.account").assertExists() }.isSuccess }
        compose.onNodeWithTag("settings.category.account").performClick()
        compose.waitUntil(5_000) { runCatching { compose.onNodeWithTag("setting.server.remove").assertExists() }.isSuccess }
        compose.onNodeWithTag("setting.server.remove").performScrollTo().performClick()
        compose.waitForIdle()
        // The confirmation dialog is up and the server is still there.
        compose.onNodeWithTag("confirm.ok").assertIsDisplayed()
        assertEquals(1, core.client.servers.value.size)
    }
}
