package app.hocket.ui

import android.graphics.Bitmap
import androidx.activity.ComponentActivity
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performSemanticsAction
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.CoreHandle
import app.hocket.core.api.Query
import app.hocket.core.api.QueryResult
import app.hocket.core.client.CoreClient
import app.hocket.core.fake.FakeCore
import app.hocket.ui.theme.HocketTheme
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.File

/**
 * A share link the server is slow to make: the menu sheet stays up showing it load, back is held off,
 * and once the link comes the sheet goes and the share sheet opens. With `HOCKET_SCREENSHOT_DIR` it
 * also saves the loading sheet.
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], qualifiers = "w411dp-h891dp-xxhdpi")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class ShareLoadingTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    /** The fake core, with createShare held until [release] completes. */
    private class SlowShare(private val fake: FakeCore, val release: CompletableDeferred<Unit>) : CoreHandle by fake {
        var asked = false
        override suspend fun query(query: Query): QueryResult {
            if (query is Query.CreateShare) {
                asked = true
                release.await()
            }
            return fake.query(query)
        }
    }

    @Test
    @Config(qualifiers = "+night")
    fun slowLinkLoadsInTheSheet() {
        val core = TestCore(startPlaying = false)
        val slow = SlowShare(core.fake, CompletableDeferred())
        val client = CoreClient(slow, CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)) { System.currentTimeMillis().toDouble() }
        client.requestSnapshot()
        compose.waitUntil(5_000) { client.started.value }
        val trackId = core.fake.library.playlistTracks.getValue("pl0").first()
        val title = core.fake.library.track(trackId)!!.title
        compose.setContent {
            HocketTheme(client = client) {
                CompositionLocalProvider(LocalCoreClient provides client, LocalDetailNavigator provides DetailNavigator({}, {}, {})) {
                    app.hocket.ui.screens.detail.PlaylistDetailScreen(androidx.navigation.compose.rememberNavController(), "pl0")
                }
            }
        }
        compose.waitUntil(5_000) { compose.onAllNodesWithContentDescription("More options for $title").fetchSemanticsNodes().isNotEmpty() }
        compose.onAllNodesWithContentDescription("More options for $title")[0].performSemanticsAction(SemanticsActions.OnClick)
        awaitTag("songMenu.share")
        compose.onNodeWithTag("songMenu.share").performSemanticsAction(SemanticsActions.OnClick)
        awaitTag("songMenu.share.navidrome")
        compose.onNodeWithTag("songMenu.share.navidrome").performSemanticsAction(SemanticsActions.OnClick)

        // The sheet stays up, its menu giving way to the loader.
        awaitTag("songMenu.sharing")
        assertTrue(slow.asked)
        compose.onNodeWithText("Getting link…").assertExists()
        compose.mainClock.advanceTimeBy(1_000)
        compose.waitForIdle()
        shot("android-share-loading")

        // Back is held off while it loads: neither the Share page nor closing the sheet.
        pressBack()
        compose.onNodeWithTag("songMenu.sharing").assertExists()
        assertNull(shadowOf(compose.activity).nextStartedActivity)

        // The link arrives: the share sheet opens and the menu goes.
        slow.release.complete(Unit)
        compose.waitUntil(5_000) { compose.onAllNodesWithTag("songMenu").fetchSemanticsNodes().isEmpty() }
        assertEquals(android.content.Intent.ACTION_CHOOSER, shadowOf(compose.activity).nextStartedActivity?.action)
    }

    private fun pressBack() {
        val v = rootViews().last { it !== compose.activity.window.decorView }
        val down = android.view.KeyEvent(android.view.KeyEvent.ACTION_DOWN, android.view.KeyEvent.KEYCODE_BACK)
        val up = android.view.KeyEvent(android.view.KeyEvent.ACTION_UP, android.view.KeyEvent.KEYCODE_BACK)
        compose.runOnUiThread { v.dispatchKeyEvent(down); v.dispatchKeyEvent(up) }
        compose.mainClock.advanceTimeBy(1_000)
        compose.waitForIdle()
    }

    private fun awaitTag(tag: String) {
        compose.waitUntil(5_000) { compose.onAllNodesWithTag(tag).fetchSemanticsNodes().isNotEmpty() }
        compose.waitForIdle()
    }

    @Suppress("UNCHECKED_CAST")
    private fun rootViews(): List<android.view.View> {
        val global = Class.forName("android.view.WindowManagerGlobal").getMethod("getInstance").invoke(null)
        val field = global.javaClass.getDeclaredField("mViews").apply { isAccessible = true }
        return (field.get(global) as List<android.view.View>).toList()
    }

    /** The activity's window with the sheet drawn over it; only when HOCKET_SCREENSHOT_DIR is set. */
    private fun shot(name: String) {
        val dir = (System.getProperty("hocket.screenshotDir") ?: System.getenv("HOCKET_SCREENSHOT_DIR"))?.takeIf { it.isNotBlank() }?.let(::File) ?: return
        dir.mkdirs()
        val bitmap = compose.onAllNodes(isRoot())[0].captureToImage().asAndroidBitmap().copy(Bitmap.Config.ARGB_8888, true)
        val canvas = android.graphics.Canvas(bitmap)
        val activityDecor = compose.activity.window.decorView
        for (view in rootViews()) {
            if (view === activityDecor || view.width == 0) continue
            val at = IntArray(2).also { view.getLocationOnScreen(it) }
            canvas.save()
            canvas.translate(at[0].toFloat(), at[1].toFloat())
            view.draw(canvas)
            canvas.restore()
        }
        File(dir, "$name.png").outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
    }
}
