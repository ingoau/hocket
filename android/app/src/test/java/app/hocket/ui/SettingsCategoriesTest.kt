package app.hocket.ui

import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
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
    private val added = setOf("queue.mode", "sleep.defaultMinutes", "sleep.stopAtEndOfTrack", "downloads.wifiOnly", "lyrics.defaultOffsetMs", "lyrics.showTranslations",
        "storage.cacheUsage", "storage.cacheMaxBytes", "storage.prefetchOnMobileData", "storage.dataSaved")

    /** Categories that open one of the older sub-screens: the old page's row for it. */
    private val screenCategories = mapOf(
        SettingsCategory.Audio to "open.audio", SettingsCategory.Streaming to "open.transcoding", SettingsCategory.Connect to "open.connect",
        SettingsCategory.Customise to "open.customise", SettingsCategory.About to "open.about",
    )

    private val expected = mapOf(
        SettingsCategory.Account to setOf("sync.master", "server.info", "server.syncNow", "server.fullSync", "server.remove"),
        SettingsCategory.Appearance to setOf("display.theme", "display.accent", "display.artworkColour", "display.animatedBackground"),
        SettingsCategory.Playback to setOf("queue.mode", "queue.savedCap", "queue.autoplay", "sleep.defaultMinutes", "sleep.stopAtEndOfTrack"),
        SettingsCategory.Downloads to setOf("open.downloads", "downloads.wifiOnly", "storage.clearCache", "storage.warnThreshold", "storage.cacheUsage", "storage.cacheMaxBytes", "storage.prefetchOnMobileData", "storage.dataSaved"),
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
        compose.openSettingsFromAccount()

        val seen = ArrayList<String>()
        for (category in SettingsCategory.entries) {
            compose.onNodeWithTag("settings.category.${category.id}").performScrollTo().assertIsDisplayed().performSemanticsAction(androidx.compose.ui.semantics.SemanticsActions.OnClick)
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
        compose.openSettingsFromAccount()
        compose.onNodeWithTag("settings.category.account").performClick()
        compose.waitUntil(5_000) { runCatching { compose.onNodeWithTag("setting.server.remove").assertExists() }.isSuccess }
        compose.onNodeWithTag("setting.server.remove").performScrollTo().performSemanticsAction(androidx.compose.ui.semantics.SemanticsActions.OnClick)
        compose.waitForIdle()
        // The confirmation dialog is up and the server is still there.
        compose.onNodeWithTag("confirm.ok").assertIsDisplayed()
        assertEquals(1, core.client.servers.value.size)
    }

    private fun openSettings(core: TestCore) {
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        compose.openSettingsFromAccount()
    }

    private fun open(category: SettingsCategory) {
        compose.onNodeWithTag("settings.category.${category.id}").performScrollTo().performSemanticsAction(androidx.compose.ui.semantics.SemanticsActions.OnClick)
        compose.waitUntil(5_000) { runCatching { compose.onNodeWithTag("settings.screen").assertExists() }.isSuccess }
        compose.waitForIdle()
    }

    private fun back() {
        compose.onNodeWithTag("settings.back").performClick()
        compose.waitUntil(5_000) { runCatching { compose.onNodeWithTag("settings.categories").assertExists() }.isSuccess }
    }

    private fun heading(): String = compose.onAllNodes(SemanticsMatcher("page title") { it.config.getOrNull(SemanticsProperties.TestTag) == "settings.title" }).fetchSemanticsNodes()
        .first().config[SemanticsProperties.Text].joinToString("") { it.text }

    @Test
    fun everyPageIsTitledLikeItsCategoryAndSummariesUseSpacedDots() {
        val core = TestCore(startPlaying = false)
        openSettings(core)
        for (category in SettingsCategory.entries) {
            val row = compose.onNodeWithTag("settings.category.${category.id}").performScrollTo().fetchSemanticsNode()
            val texts = row.config[SemanticsProperties.Text].map { it.text }
            val title = texts.first()
            texts.drop(1).forEach { summary ->
                assertTrue("'${category.id}' summary uses ' · ' separators: $summary", Regex("\\S·|·\\S").find(summary) == null)
                assertTrue("'${category.id}' summary has no comma lists: $summary", category == SettingsCategory.Account || !summary.contains(", "))
            }
            open(category)
            assertEquals("page title of ${category.id}", title, heading())
            back()
        }
    }

    @Test
    fun accentChoicesAreRealSwatchesSpokenByName() {
        val core = TestCore(startPlaying = false)
        openSettings(core)
        open(SettingsCategory.Appearance)
        val teal = compose.onNodeWithTag("accent.#1B6B5E")
        val node = teal.fetchSemanticsNode()
        assertEquals("Teal", node.config[SemanticsProperties.ContentDescription].single())
        assertEquals(androidx.compose.ui.semantics.Role.RadioButton, node.config[SemanticsProperties.Role])
        assertEquals(false, node.config[SemanticsProperties.Selected])
        teal.performClick()
        compose.waitUntil(5_000) { core.client.settings.value["display.accent"]?.value == "\"#1B6B5E\"" }
        compose.waitForIdle()
        assertEquals(true, compose.onNodeWithTag("accent.#1B6B5E").fetchSemanticsNode().config[SemanticsProperties.Selected])
        // No symbol stands in for a colour.
        assertTrue(compose.onAllNodes(SemanticsMatcher("a dot label") { n -> n.config.getOrNull(SemanticsProperties.Text)?.any { it.text == "●" } == true }).fetchSemanticsNodes().isEmpty())
    }

    @Test
    fun customiseShowsHumanLabelsAndEachListIsAsTallAsItsRows() {
        val core = TestCore(startPlaying = false)
        openSettings(core)
        open(SettingsCategory.Customise)
        fun textOf(tag: String) = compose.onNodeWithTag(tag).performScrollTo().fetchSemanticsNode().config[SemanticsProperties.Text].joinToString("") { it.text }
        assertEquals("Shuffle play", textOf("customise.contextMenu.playShuffled"))
        assertEquals("Rate 5 stars", textOf("customise.contextMenu.rate5"))
        assertEquals("Go to album", textOf("customise.contextMenu.goToAlbum"))
        assertEquals("Play / pause", textOf("customise.mediaSession.togglePlay"))
        for (list in listOf("customise.contextMenu", "customise.mediaSession")) {
            val column = compose.onNodeWithTag(list).performScrollTo().fetchSemanticsNode().boundsInRoot
            val rows = compose.onAllNodes(SemanticsMatcher("rows of $list") { it.config.getOrNull(SemanticsProperties.TestTag)?.startsWith("$list.") == true }).fetchSemanticsNodes()
            assertTrue(rows.isNotEmpty())
            val rowsHeight = rows.sumOf { it.boundsInRoot.height.toDouble() }
            assertEquals("no gap after $list", rowsHeight, column.height.toDouble(), 2.0)
        }
    }

    @Test
    fun signOutIsLastAndAccountRowsCarryScopes() {
        val core = TestCore(startPlaying = false)
        openSettings(core)
        open(SettingsCategory.Account)
        assertEquals("server.remove", rowsOnScreen().last())
        for (tag in listOf("sync.master", "server.info")) {
            compose.onNode(hasAnyAncestor(hasTestTag("setting.$tag")).or(hasTestTag("setting.$tag")).and(hasText("This device", substring = true))).assertExists()
        }
    }

    @Test
    fun theDebugBannerNeverCoversTheTopAppBar() {
        val core = TestCore(startPlaying = false)
        openSettings(core)
        open(SettingsCategory.Account)
        val banner = compose.onNodeWithTag("debug.fakeCoreBanner").fetchSemanticsNode().boundsInRoot
        val backButton = compose.onNodeWithTag("settings.back").fetchSemanticsNode().boundsInRoot
        assertTrue("banner $banner ends above the back arrow $backButton", banner.bottom <= backButton.top + 0.5f)
    }

    @Test
    fun theRecentQueuesSliderIsContinuousToTheEyeButSnapsForTalkBack() {
        val core = TestCore(startPlaying = false)
        openSettings(core)
        open(SettingsCategory.Playback)
        val slot = compose.onNodeWithTag("queue.savedCap.slider").fetchSemanticsNode()
        assertEquals(49, slot.config[SemanticsProperties.ProgressBarRangeInfo].steps)
    }
}
