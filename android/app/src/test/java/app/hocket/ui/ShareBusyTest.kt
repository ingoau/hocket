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
import org.junit.Assert.assertFalse
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
 * A share link the server is slow to make: after a beat a dialog says it is on its way, and Cancel
 * gives up on it (the share sheet never opens). With `HOCKET_SCREENSHOT_DIR` it also saves the dialog.
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], qualifiers = "w411dp-h891dp-xxhdpi")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class ShareBusyTest {
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
    fun slowLinkShowsBusyDialogAndCancels() {
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

        awaitTag("share.busy")
        assertTrue(slow.asked)
        compose.onNodeWithText("Getting link…").assertExists()
        compose.mainClock.advanceTimeBy(1_000)
        compose.waitForIdle()
        shot("android-share-busy")

        compose.onNodeWithText("Cancel").performSemanticsAction(SemanticsActions.OnClick)
        compose.waitUntil(5_000) { compose.onAllNodesWithTag("share.busy").fetchSemanticsNodes().isEmpty() }
        // Given up: the link arriving now opens nothing.
        slow.release.complete(Unit)
        compose.waitForIdle()
        assertFalse(compose.onAllNodesWithTag("share.busy").fetchSemanticsNodes().isNotEmpty())
        assertNull(shadowOf(compose.activity).nextStartedActivity)
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

    /** The activity's window with the dialog drawn over it, placed as on a device; only when HOCKET_SCREENSHOT_DIR is set. */
    private fun shot(name: String) {
        val dir = (System.getProperty("hocket.screenshotDir") ?: System.getenv("HOCKET_SCREENSHOT_DIR"))?.takeIf { it.isNotBlank() }?.let(::File) ?: return
        dir.mkdirs()
        val bitmap = compose.onAllNodes(isRoot())[0].captureToImage().asAndroidBitmap().copy(Bitmap.Config.ARGB_8888, true)
        val canvas = android.graphics.Canvas(bitmap)
        val activityDecor = compose.activity.window.decorView
        for (view in rootViews()) {
            if (view === activityDecor || view.width == 0) continue
            // Robolectric leaves a dialog's window at the top left with no dim; place it as the window
            // manager does on a device: centred, over the dialog scrim.
            canvas.drawColor(android.graphics.Color.argb(82, 0, 0, 0))
            canvas.save()
            canvas.translate((bitmap.width - view.width) / 2f, (bitmap.height - view.height) / 2f)
            view.draw(canvas)
            canvas.restore()
        }
        File(dir, "$name.png").outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
    }
}
