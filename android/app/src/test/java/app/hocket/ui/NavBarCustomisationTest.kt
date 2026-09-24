package app.hocket.ui

import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.longClick
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.performTouchInput
import androidx.datastore.preferences.core.PreferenceDataStoreFactory
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.AppPrefs
import app.hocket.InMemoryNavBarPrefs
import app.hocket.NavBarPrefs
import app.hocket.ui.a11y.A11yChecks
import app.hocket.ui.a11y.performCustomAction
import app.hocket.ui.nav.AppRoot
import app.hocket.ui.nav.LocalNavBarPrefs
import app.hocket.ui.nav.NavItem
import app.hocket.core.SettingKeys
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/** The customisable bottom bar: default, editor, bounds, migration, selection, the account sheet. */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class NavBarCustomisationTest {
    @get:Rule
    val compose = createComposeRule()

    @get:Rule
    val tmp = TemporaryFolder()

    private fun exists(tag: String) = compose.onAllNodesWithTag(tag).fetchSemanticsNodes().isNotEmpty()

    private fun await(tag: String) {
        compose.waitUntil(5_000) { exists(tag) }
        compose.waitForIdle()
    }

    private fun click(tag: String) {
        await(tag)
        compose.onAllNodesWithTag(tag).onFirst().performSemanticsAction(SemanticsActions.OnClick)
        compose.waitForIdle()
    }

    /** The bar's item ids, left to right. */
    private fun bar(): List<String> = compose.onAllNodes(SemanticsMatcher("a bar item") { it.config.getOrNull(SemanticsProperties.TestTag)?.startsWith("navBar.") == true })
        .fetchSemanticsNodes().sortedBy { it.boundsInRoot.left }.map { it.config[SemanticsProperties.TestTag].removePrefix("navBar.") }

    private fun selected(): List<String> = compose.onAllNodes(SemanticsMatcher("a selected bar item") {
        it.config.getOrNull(SemanticsProperties.TestTag)?.startsWith("navBar.") == true && it.config.getOrNull(SemanticsProperties.Selected) == true
    }).fetchSemanticsNodes().map { it.config[SemanticsProperties.TestTag].removePrefix("navBar.") }

    private fun start(prefs: NavBarPrefs = InMemoryNavBarPrefs()): TestCore {
        val core = TestCore(startPlaying = false)
        compose.setThemedContent(core) { CompositionLocalProvider(LocalNavBarPrefs provides prefs) { AppRoot(core.client) } }
        core.start()
        await("navBar")
        return core
    }

    private fun openEditorByLongPress() {
        compose.onNodeWithTag("navBar").performTouchInput { longClick(center) }
        await("bottomBar.editor")
    }

    @Test
    fun theDefaultBarIsHomeSearchLibraryWithoutSettings() {
        start()
        compose.waitUntil(5_000) { bar().size == 3 }
        assertEquals(listOf("home", "search", "library"), bar())
        assertFalse(exists("navBar.settings"))
        assertEquals(listOf("home"), selected())
    }

    @Test
    fun oldListsMigrate() {
        assertEquals(listOf(NavItem.Home, NavItem.Library, NavItem.Stats), NavItem.fromIds(listOf("home", "settings", "library", "stats")))
        assertEquals("fewer than two left: the default", NavItem.DEFAULT, NavItem.fromIds(listOf("settings", "home")))
        assertEquals(NavItem.DEFAULT, NavItem.fromIds(emptyList()))
        assertEquals(NavItem.DEFAULT, NavItem.fromIds(listOf("nonsense", "settings")))
        assertEquals("duplicates collapse", listOf(NavItem.Home, NavItem.Search), NavItem.fromIds(listOf("home", "home", "search")))
        assertEquals("capped at five", 5, NavItem.fromIds(listOf("home", "search", "library", "albums", "songs", "genres")).size)
        // Every place has its own registry action; Library no longer borrows navigateAlbums.
        assertEquals("navigateLibrary", NavItem.Library.canonicalActionId)
        assertEquals(NavItem.entries.size, NavItem.entries.map { it.canonicalActionId }.distinct().size)
        // And in the app: an old stored order with Settings in it.
        start(InMemoryNavBarPrefs(listOf("downloads", "settings", "home", "filters")))
        compose.waitUntil(5_000) { bar() == listOf("downloads", "home", "filters") }
    }

    @Test
    fun addReorderAndRemovePersistAcrossRecreation() = runBlocking {
        val file = tmp.newFile("bar.preferences_pb").also { it.delete() }
        val scope1 = CoroutineScope(SupervisorJob() + Dispatchers.IO)
        val store1 = PreferenceDataStoreFactory.create(scope = scope1) { file }
        var prefs by mutableStateOf<NavBarPrefs>(AppPrefs(store1))
        var generation by mutableIntStateOf(0)
        val core = TestCore(startPlaying = false)
        compose.setThemedContent(core) {
            key(generation) { CompositionLocalProvider(LocalNavBarPrefs provides prefs) { AppRoot(core.client) } }
        }
        core.start()
        await("navBar")
        openEditorByLongPress()
        // Add Albums, move it up once (the TalkBack action), remove Search.
        click("bottomBar.add")
        click("bottomBar.addItem.albums")
        compose.waitUntil(5_000) { exists("bottomBar.item.albums") }
        compose.onNodeWithTag("bottomBar.item.albums").performScrollTo().performCustomAction(compose, "Move up")
        compose.waitUntil(5_000) { bar() == listOf("home", "search", "albums", "library") }
        click("bottomBar.remove.search")
        compose.waitUntil(5_000) { bar() == listOf("home", "albums", "library") }
        val key = stringPreferencesKey("navItems")
        compose.waitUntil(5_000) { runBlocking { store1.data.first()[key] } == "home,albums,library" }
        // The core's sidebar surface follows, Library as its own action (not a second Albums).
        compose.waitUntil(5_000) {
            core.client.settings.value[SettingKeys.ACTIONS_ORDER_SIDEBAR]?.value == """["navigateHome","navigateAlbums","navigateLibrary"]"""
        }

        // Recreate: a fresh DataStore over the same file, a fresh composition.
        scope1.coroutineContext[kotlinx.coroutines.Job]!!.cancelAndJoin()
        val store2 = PreferenceDataStoreFactory.create(scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)) { file }
        compose.runOnUiThread { prefs = AppPrefs(store2); generation++ }
        await("navBar")
        compose.waitUntil(5_000) { bar() == listOf("home", "albums", "library") }
    }

    @Test
    fun theBarHoldsTwoToFiveItemsAndSaysWhy() {
        start(InMemoryNavBarPrefs(listOf("home", "search", "library", "albums", "songs")))
        compose.waitUntil(5_000) { bar().size == 5 }
        openEditorByLongPress()
        click("bottomBar.add")
        await("bottomBar.message")
        assertTrue(compose.onNodeWithTag("bottomBar.message").fetchSemanticsNode().config[SemanticsProperties.Text].joinToString { it.text }.contains("at most 5"))
        assertFalse("no picker past five", exists("bottomBar.addItem.genres"))
        assertEquals(5, bar().size)
    }

    @Test
    fun removingBelowTwoIsRefusedWithAMessage() {
        start(InMemoryNavBarPrefs(listOf("home", "search")))
        compose.waitUntil(5_000) { bar() == listOf("home", "search") }
        openEditorByLongPress()
        click("bottomBar.remove.home")
        await("bottomBar.message")
        assertTrue(compose.onNodeWithTag("bottomBar.message").fetchSemanticsNode().config[SemanticsProperties.Text].joinToString { it.text }.contains("at least 2"))
        assertEquals(listOf("home", "search"), bar())
        // Reset brings the default back.
        click("bottomBar.reset")
        compose.waitUntil(5_000) { bar() == listOf("home", "search", "library") }
    }

    @Test
    fun theSelectedTabIsTheOneWhoseStackIsShowing() {
        start(InMemoryNavBarPrefs(listOf("home", "albums", "library")))
        compose.waitUntil(5_000) { bar() == listOf("home", "albums", "library") }
        // An album opened from Library keeps Library selected.
        click("navBar.library")
        compose.waitUntil(5_000) { exists("library.album") }
        click("library.album")
        await("detail.header")
        assertEquals(listOf("library"), selected())
        // The same from the Albums place keeps Albums.
        click("navBar.albums")
        await("libraryList.albums")
        compose.waitUntil(5_000) { exists("library.album") }
        click("library.album")
        await("detail.header")
        assertEquals(listOf("albums"), selected())
        click("navBar.home")
        assertEquals(listOf("home"), selected())
        // Back to Library: its stack (the album) is restored, and Library is selected.
        click("navBar.library")
        await("detail.header")
        assertEquals(listOf("library"), selected())
    }

    @Test
    fun placesAndSettingsOpenedInsideAStackKeepItsTab() {
        start()
        compose.waitUntil(5_000) { bar().size == 3 }
        // Recent queues is not in the bar: opened from the Library, it is inside Library's stack.
        click("navBar.library")
        click("library.link.recentQueues")
        compose.waitForIdle()
        assertEquals(listOf("library"), selected())
        // Settings and its sub-screens, pushed from the account button, keep the place they were opened over.
        compose.openSettingsFromAccount()
        assertEquals(listOf("library"), selected())
        click("settings.category.audio")
        await("settings.screen")
        assertEquals(listOf("library"), selected())
        // A place from the account sheet's "More places" becomes the current stack (no bar item selected).
        click("navBar.home")
        click("account.button")
        click("account.place.downloads")
        await("downloads.availableOffline")
        assertEquals(emptyList<String>(), selected())
    }

    @Test
    fun theAccountButtonOpensTheSheetAndSettings() {
        start()
        compose.waitUntil(5_000) { bar().size == 3 }
        val button = compose.onAllNodesWithTag("account.button").onFirst().fetchSemanticsNode()
        assertEquals("Account and settings", button.config[SemanticsProperties.ContentDescription].single())
        click("account.button")
        await("account.sheet")
        // Every place not in the bar is reachable from here.
        for (item in NavItem.entries.filter { it !in NavItem.DEFAULT && it != NavItem.Stats }) assertTrue(item.id, exists("account.place.${item.id}"))
        assertFalse(exists("account.place.home"))
        A11yChecks.assertAccessible(compose, "account sheet")
        click("account.settings")
        await("settings.categories")
        assertFalse(exists("account.sheet"))
    }

    @Test
    fun theEditorPassesTheAccessibilityChecks() {
        start()
        openEditorByLongPress()
        A11yChecks.assertAccessible(compose, "bottom bar editor")
        // Long-press did not also navigate.
        assertEquals(listOf("home"), selected())
    }
}
