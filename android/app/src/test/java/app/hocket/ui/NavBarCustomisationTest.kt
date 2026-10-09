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
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.AppPrefs
import app.hocket.InMemoryNavBarPrefs
import app.hocket.NavBarPrefs
import app.hocket.SyncedNavBarPrefs
import app.hocket.ui.a11y.A11yChecks
import app.hocket.ui.a11y.performCustomAction
import app.hocket.ui.nav.AppRoot
import app.hocket.ui.nav.LocalNavBarPrefs
import app.hocket.ui.nav.NavItem
import app.hocket.core.SettingKeys
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
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

    /** The editor sheet, from the account sheet's "Customise bottom bar" (holding the bar does not open it). */
    private fun openEditor() {
        click("account.button")
        click("account.editBar")
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
        // And in the app: an old stored order with Settings in it.
        start(InMemoryNavBarPrefs(listOf("downloads", "settings", "home", "filters")))
        compose.waitUntil(5_000) { bar() == listOf("downloads", "home", "filters") }
    }

    @Test
    fun addReorderAndRemovePersistAcrossRecreation() {
        val core = TestCore(startPlaying = false)
        var prefs by mutableStateOf<NavBarPrefs>(SyncedNavBarPrefs(core.client))
        var generation by mutableIntStateOf(0)
        compose.setThemedContent(core) {
            key(generation) { CompositionLocalProvider(LocalNavBarPrefs provides prefs) { AppRoot(core.client) } }
        }
        core.start()
        await("navBar")
        openEditor()
        // Add Albums, move it up once (the TalkBack action), remove Search.
        click("bottomBar.add")
        click("bottomBar.addItem.albums")
        compose.waitUntil(5_000) { exists("bottomBar.item.albums") }
        compose.onNodeWithTag("bottomBar.item.albums").performScrollTo().performCustomAction(compose, "Move up")
        compose.waitUntil(5_000) { bar() == listOf("home", "search", "albums", "library") }
        click("bottomBar.remove.search")
        compose.waitUntil(5_000) { bar() == listOf("home", "albums", "library") }
        // Saved as the synced phone bar; the desktop sidebar's order is left alone.
        compose.waitUntil(5_000) { core.client.settings.value[SettingKeys.NAV_MOBILE_BAR]?.value == """["home","albums","library"]""" }
        assertEquals("[]", core.client.settings.value[SettingKeys.ACTIONS_ORDER_SIDEBAR]?.value)

        // Recreate: a fresh store over the same core (another phone on the account), a fresh composition.
        compose.runOnUiThread { prefs = SyncedNavBarPrefs(core.client); generation++ }
        await("navBar")
        compose.waitUntil(5_000) { bar() == listOf("home", "albums", "library") }
    }

    @Test
    fun aDeviceLocalBarIsAdoptedOnce() = runBlocking {
        val file = tmp.newFile("bar.preferences_pb").also { it.delete() }
        val local = AppPrefs(PreferenceDataStoreFactory.create(scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)) { file })
        local.setNavItems(listOf("downloads", "home", "filters"))
        val core = TestCore(startPlaying = false)
        core.start()
        val synced = SyncedNavBarPrefs(core.client)
        synced.adoptLocal(local)
        assertEquals(listOf("downloads", "home", "filters"), synced.navItems.first { it.isNotEmpty() })
        assertEquals(emptyList<String>(), local.navItems.first())
        // An account that already has a bar keeps it.
        local.setNavItems(listOf("home", "stats"))
        synced.adoptLocal(local)
        assertEquals(listOf("downloads", "home", "filters"), synced.navItems.first())
        assertEquals(emptyList<String>(), local.navItems.first())
    }

    @Test
    fun theBarHoldsTwoToFiveItemsAndSaysWhy() {
        start(InMemoryNavBarPrefs(listOf("home", "search", "library", "albums", "songs")))
        compose.waitUntil(5_000) { bar().size == 5 }
        openEditor()
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
        openEditor()
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
    fun switchingPlacesClosesSettings() {
        start()
        compose.waitUntil(5_000) { bar().size == 3 }
        // Over Home: Library then Home is Home itself, not Settings brought back with Home's stack.
        compose.openSettingsFromAccount()
        click("navBar.library")
        await("library.links")
        click("navBar.home")
        compose.waitUntil(5_000) { !exists("settings.categories") && !exists("library.links") }
        assertEquals(listOf("home"), selected())
        // Over Library, a sub-screen deep: Home closes it, and Library comes back at its own screen.
        click("navBar.library")
        await("library.links")
        compose.openSettingsFromAccount()
        click("settings.category.audio")
        await("settings.screen")
        click("navBar.home")
        compose.waitUntil(5_000) { !exists("settings.screen") && !exists("library.links") }
        assertEquals(listOf("home"), selected())
        click("navBar.library")
        await("library.links")
        assertFalse(exists("settings.screen") || exists("settings.categories"))
        assertEquals(listOf("library"), selected())
        // Re-tapping the selected place over Settings pops back to its root.
        compose.openSettingsFromAccount()
        click("navBar.library")
        compose.waitUntil(5_000) { !exists("settings.categories") }
        await("library.links")
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
        openEditor()
        A11yChecks.assertAccessible(compose, "bottom bar editor")
    }

    @Test
    fun holdingTheBarDoesNotOpenTheEditor() {
        start()
        compose.waitUntil(5_000) { bar().size == 3 }
        compose.onNodeWithTag("navBar").performTouchInput { longClick(center) }
        compose.waitForIdle()
        assertFalse(exists("bottomBar.editor"))
        // Nor does an item offer an "edit" accessibility action any more.
        val actions = compose.onNodeWithTag("navBar.home").fetchSemanticsNode().config.getOrNull(SemanticsActions.CustomActions).orEmpty()
        assertTrue(actions.none { it.label == "Customise bottom bar" })
    }
}
