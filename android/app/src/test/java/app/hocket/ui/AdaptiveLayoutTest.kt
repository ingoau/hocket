package app.hocket.ui

import androidx.activity.ComponentActivity
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.StateRestorationTester
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.performTextInput
import androidx.navigation.compose.rememberNavController
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.R
import app.hocket.core.Commands
import app.hocket.core.SettingKeys
import app.hocket.ui.screens.home.HomeScreen
import app.hocket.ui.screens.home.HomeSections
import app.hocket.ui.screens.home.SectionPref
import app.hocket.ui.screens.library.LibraryList
import app.hocket.ui.screens.library.LibraryListScreen
import app.hocket.ui.screens.settings.SettingsScreen
import app.hocket.ui.theme.HocketTheme
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * Content screens on tablet-sized and landscape content areas (the screens on their own, so the
 * content area is the whole window): the library lists and Settings as list and detail side by
 * side from 840 dp, one pane below it, and Home's grids fitting more columns as they widen while a
 * phone keeps two.
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w1280dp-h800dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class AdaptiveLayoutTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val context get() = ApplicationProvider.getApplicationContext<android.content.Context>()

    private fun exists(tag: String) = compose.onAllNodesWithTag(tag).fetchSemanticsNodes().isNotEmpty()

    private fun await(tag: String) {
        compose.waitUntil(5_000) { exists(tag) }
        compose.waitForIdle()
    }

    private fun click(tag: String, index: Int = 0) {
        await(tag)
        compose.onAllNodesWithTag(tag)[index].performSemanticsAction(SemanticsActions.OnClick)
        compose.waitForIdle()
    }

    // --- Library ---

    private fun startLibrary(core: TestCore) {
        core.start()
        compose.setThemedContent(core) { LibraryListScreen(rememberNavController(), LibraryList.Albums) }
        await("libraryList.albums")
    }

    /** The album the pane's header names, among the fake library's. */
    private fun paneAlbum(core: TestCore): String? = core.fake.library.albums.map { it.name }.firstOrNull { name ->
        compose.onAllNodes(hasText(name) and hasAnyAncestor(hasTestTag("detail.header")) and hasAnyAncestor(hasTestTag("library.pane")), useUnmergedTree = true)
            .fetchSemanticsNodes().isNotEmpty()
    }

    private fun awaitPaneAlbum(core: TestCore, not: String? = null): String {
        compose.waitUntil(5_000) { paneAlbum(core).let { it != null && it != not } }
        compose.waitForIdle()
        return paneAlbum(core)!!
    }

    @Test
    fun wideLibraryShowsTheListAndPicksOpenInThePaneBesideIt() {
        val core = TestCore(startPlaying = false)
        startLibrary(core)
        // Nothing picked yet: the pane says so, beside the list.
        await("library.pane.empty")
        val list = compose.onNodeWithTag("libraryList.albums").fetchSemanticsNode().boundsInRoot
        val pane = compose.onNodeWithTag("library.pane").fetchSemanticsNode().boundsInRoot
        assertTrue("the pane is to the list's right", pane.left >= list.right - 1f)
        assertTrue("the list is a phone's width", list.width < pane.width)

        click("library.album", 0)
        val first = awaitPaneAlbum(core)
        assertFalse(exists("library.pane.empty"))
        assertTrue("the list stays", exists("libraryList.albums"))
        // A detail page in the pane has no back arrow of its own: Back is the list's.
        click("library.album", 1)
        val second = awaitPaneAlbum(core, not = first)
        assertNotEquals(first, second)
    }

    @Test
    fun theLibraryPaneKeepsItsPickAcrossRecreation() {
        val core = TestCore(startPlaying = false)
        core.start()
        val restore = StateRestorationTester(compose)
        restore.setContent {
            HocketTheme(client = core.client) {
                CompositionLocalProvider(LocalCoreClient provides core.client) { LibraryListScreen(rememberNavController(), LibraryList.Albums) }
            }
        }
        click("library.album", 1)
        val picked = awaitPaneAlbum(core)
        restore.emulateSavedInstanceStateRestore()
        assertEquals(picked, awaitPaneAlbum(core))
    }

    @Test
    @Config(qualifiers = "w891dp-h411dp")
    fun aLandscapePhoneWideEnoughGetsBothPanes() {
        val core = TestCore(startPlaying = false)
        startLibrary(core)
        await("library.pane.empty")
        click("library.album", 0)
        awaitPaneAlbum(core)
    }

    @Test
    @Config(qualifiers = "w800dp-h1280dp")
    fun aPortraitTabletKeepsOnePane() {
        val core = TestCore(startPlaying = false)
        startLibrary(core)
        await("library.album")
        assertFalse(exists("library.pane"))
    }

    // --- Settings ---

    private fun startSettings() {
        val core = TestCore(startPlaying = false)
        core.start()
        compose.setThemedContent(core) { SettingsScreen(rememberNavController()) }
        await("settings.categories")
    }

    private fun paneTitle(): String {
        val node = compose.onAllNodes(hasTestTag("settings.title") and hasAnyAncestor(hasTestTag("settings.pane")))
            .fetchSemanticsNodes().single()
        return node.config[SemanticsProperties.Text].joinToString("") { it.text }
    }

    private fun awaitPaneTitle(res: Int) {
        val want = context.getString(res)
        compose.waitUntil(5_000) { runCatching { paneTitle() == want }.getOrDefault(false) }
        compose.waitForIdle()
    }

    @Test
    fun wideSettingsShowTheCategoryBesideTheList() {
        startSettings()
        await("settings.pane")
        // Account until another is picked; nothing to go back to in the pane.
        awaitPaneTitle(R.string.settings_category_account)
        assertFalse(exists("settings.back"))
        click("settings.category.appearance")
        awaitPaneTitle(R.string.settings_section_appearance)
        assertTrue(exists("settings.categories"))
        val row = compose.onNodeWithTag("settings.category.appearance").fetchSemanticsNode()
        assertEquals(true, row.config.getOrNull(SemanticsProperties.Selected))
    }

    @Test
    fun wideSettingsSearchOpensTheHitInThePane() {
        startSettings()
        compose.onNodeWithTag("settings.search").performTextInput("cache size")
        compose.waitForIdle()
        click("settings.hit.storage.cacheMaxBytes")
        awaitPaneTitle(R.string.settings_category_downloads)
        compose.mainClock.advanceTimeBy(1_000)
        compose.waitForIdle()
        assertNotNull(compose.onAllNodes(hasTestTag("setting.storage.cacheMaxBytes") and hasAnyAncestor(hasTestTag("settings.pane"))).fetchSemanticsNodes().singleOrNull())
        // The results stay in the list, for the next pick.
        assertTrue(exists("settings.search.results"))
    }

    @Test
    @Config(qualifiers = "w800dp-h1280dp")
    fun narrowerSettingsKeepOnePane() {
        startSettings()
        await("settings.category.appearance")
        assertFalse(exists("settings.pane"))
    }

    // --- Home ---

    /** Home with only New in library, as a grid; how many cells its first row holds. */
    private fun homeGridColumns(): Int {
        val core = TestCore(startPlaying = false)
        core.start()
        core.client.dispatch(Commands.setSetting(SettingKeys.HOME_SECTIONS, HomeSections.encode(listOf(SectionPref("newInLibrary", true, "grid")))))
        compose.setThemedContent(core) { HomeScreen(rememberNavController()) }
        val inGrid: SemanticsMatcher = hasTestTag("home.item") and hasAnyAncestor(hasTestTag("home.grid"))
        compose.waitUntil(5_000) { compose.onAllNodes(inGrid).fetchSemanticsNodes().isNotEmpty() }
        compose.waitForIdle()
        val cells = compose.onAllNodes(inGrid).fetchSemanticsNodes()
        val rows = cells.groupBy { it.boundsInRoot.top.toInt() }
        assertTrue("three rows at most", rows.size <= 3)
        return rows.values.maxOf { it.size }
    }

    @Test
    @Config(qualifiers = "w411dp-h891dp")
    fun aPhoneHomeGridKeepsTwoColumns() = assertEquals(2, homeGridColumns())

    @Test
    @Config(qualifiers = "w800dp-h1280dp")
    fun aPortraitTabletHomeGridHasFourColumns() = assertEquals(4, homeGridColumns())

    @Test
    @Config(qualifiers = "w891dp-h411dp")
    fun aLandscapePhoneHomeGridHasFiveColumns() = assertEquals(5, homeGridColumns())

    @Test
    fun aLandscapeTabletHomeGridStopsAtSix() = assertEquals(6, homeGridColumns())
}
