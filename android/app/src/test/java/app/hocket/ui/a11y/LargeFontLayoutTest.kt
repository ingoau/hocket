package app.hocket.ui.a11y

import androidx.activity.ComponentActivity
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.unit.Density
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.TestCore
import app.hocket.ui.nav.AppRoot
import app.hocket.ui.screens.settings.SettingsCategory
import app.hocket.ui.theme.HocketTheme
import app.hocket.ui.openSettingsFromAccount
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * Font scale 2.0 on a phone at display size "largest" (about 320 dp wide): the settings categories
 * and their screens, the now-playing sheet, the mini player, the queue and the album header keep
 * every control whole and apart, and no text is cut off (an ellipsis is fine; clipping is not).
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w320dp-h690dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class LargeFontLayoutTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val core = TestCore()

    private fun start() {
        compose.setContent {
            val base = LocalDensity.current
            CompositionLocalProvider(LocalDensity provides Density(base.density, fontScale = 2f)) {
                HocketTheme(client = core.client) {
                    CompositionLocalProvider(LocalCoreClient provides core.client) { AppRoot(core.client) }
                }
            }
        }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        compose.waitForIdle()
    }

    private fun exists(tag: String) = runCatching { compose.onNodeWithTag(tag).assertExists() }.isSuccess
    private fun click(tag: String) = compose.onNodeWithTag(tag).performSemanticsAction(SemanticsActions.OnClick)

    private fun holds(what: String) {
        compose.waitForIdle()
        A11yChecks.assertLayoutHolds(compose, what)
        A11yChecks.assertAccessible(compose, what)
    }

    @Test
    fun theMiniPlayerAndHomeAtFontScaleTwo() {
        start()
        compose.onNodeWithTag("miniPlayer").assertIsDisplayed()
        holds("home with the mini player")
    }

    @Test
    fun settingsCategoriesAndTheirScreensAtFontScaleTwo() {
        start()
        compose.openSettingsFromAccount()
        holds("settings categories")
        for (category in SettingsCategory.entries) {
            compose.onNodeWithTag("settings.category.${category.id}").performScrollTo().performSemanticsAction(SemanticsActions.OnClick)
            compose.waitUntil(5_000) { exists("settings.back") }
            holds("settings: ${category.id}")
            click("settings.back")
            compose.waitUntil(5_000) { exists("settings.categories") }
        }
    }

    @Test
    fun theNowPlayingSheetAndQueueAtFontScaleTwo() {
        start()
        compose.onNodeWithTag("nowPlaying.sheet").performSemanticsAction(SemanticsActions.Expand)
        compose.waitUntil(5_000) { exists("player.playPause") }
        holds("now playing")
        compose.onNodeWithTag("player.page").performScrollTo()
        compose.onNodeWithTag("player.playPause").performScrollTo()
        holds("now playing, transport in view")
        click("player.mode.queue")
        compose.waitForIdle()
        holds("queue")
    }

    @Test
    fun theAlbumHeaderAtFontScaleTwo() {
        start()
        click("navBar.library")
        compose.waitUntil(5_000) { compose.onAllNodesWithTag("library.album").fetchSemanticsNodes().isNotEmpty() }
        compose.onAllNodesWithTag("library.album")[0].performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { exists("detail.header") }
        holds("album header")
    }
}
