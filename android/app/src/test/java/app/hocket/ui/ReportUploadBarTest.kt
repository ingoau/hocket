package app.hocket.ui

import android.graphics.Bitmap
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.api.ReportUploadError
import app.hocket.core.api.ReportUploadState
import app.hocket.core.api.ReportUploadStatus
import app.hocket.core.fake.FakeCore
import app.hocket.ui.components.REPORT_DONE_SHOWN_MS
import app.hocket.ui.components.ReportUploadBar
import app.hocket.ui.components.ReportUploadBarHost
import app.hocket.ui.theme.HocketTheme
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.File

/** The report upload bar across the top of the app: each state's message and buttons. Writes PNGs of each state when HOCKET_SCREENSHOT_DIR is set. */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp-xxhdpi")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class ReportUploadBarTest {
    @get:Rule
    val compose = createComposeRule()

    private fun status(state: ReportUploadState, sent: Double = 0.0, id: String? = null, error: ReportUploadError? = null) =
        ReportUploadStatus(state, sent, 1000.0, id, error, null, 1.0)

    private fun textOf(tag: String): String =
        compose.onNodeWithTag(tag).fetchSemanticsNode().config.getOrElse(SemanticsProperties.Text) { emptyList() }.joinToString("\n") { it.text }

    @Test
    fun eachStateSaysWhereTheUploadStandsAndItsButtonsAct() {
        var status by mutableStateOf(status(ReportUploadState.Uploading, sent = 340.0))
        val calls = mutableListOf<String>()
        compose.setContent {
            HocketTheme(client = null) {
                ReportUploadBar(status, insets = null, onCancel = { calls += "cancel" }, onRetry = { calls += "retry" }, onDismiss = { calls += "dismiss" }, onCopy = { calls += "copy $it" })
            }
        }
        assertEquals("Uploading report… 34%", textOf("report.bar.message"))
        // Announced politely, once per state rather than per percent.
        compose.onNodeWithTag("report.bar.message").assert(SemanticsMatcher.keyIsDefined(SemanticsProperties.LiveRegion))
        compose.onNodeWithTag("report.bar.progress").assertExists()
        compose.onNodeWithTag("report.bar.cancel").performClick()

        status = status(ReportUploadState.Failed, sent = 340.0, error = ReportUploadError.Network)
        compose.waitForIdle()
        assertTrue(textOf("report.bar.message").contains("couldn't be reached"))
        assertTrue(compose.onAllNodesWithTag("report.bar.progress").fetchSemanticsNodes().isEmpty())
        compose.onNodeWithTag("report.bar.retry").performClick()
        compose.onNodeWithTag("report.bar.dismiss").performClick()

        status = status(ReportUploadState.Failed, error = ReportUploadError.NotAllowed)
        compose.waitForIdle()
        assertTrue(textOf("report.bar.message").contains("no longer allows it"))

        status = status(ReportUploadState.Done, sent = 1000.0, id = "R-20261101-7k3f")
        compose.waitForIdle()
        assertEquals("Report R-20261101-7k3f saved to the coordinator", textOf("report.bar.message"))
        compose.onNodeWithTag("report.bar.copy").performClick()
        assertEquals(listOf("cancel", "retry", "dismiss", "copy R-20261101-7k3f"), calls)
    }

    @Test
    fun aFinishedUploadCopiesItsIdAndGoesBySelf() {
        val core = TestCore(startPlaying = false)
        compose.mainClock.autoAdvance = false
        compose.setThemedContent(core) { ReportUploadBarHost(core.client, insets = null) }
        core.start()
        compose.mainClock.advanceTimeByFrame()
        assertTrue(compose.onAllNodesWithTag("report.bar").fetchSemanticsNodes().isEmpty())
        core.client.startReportUpload("note", "bundle", emptyList())
        compose.mainClock.advanceTimeByFrame()
        assertEquals(ReportUploadState.Done, core.client.reportUpload.value?.state)
        assertTrue(textOf("report.bar.message").contains(FakeCore.FAKE_REPORT_ID))
        compose.onNodeWithTag("report.bar.copy").performClick()
        val clip = ApplicationProvider.getApplicationContext<android.content.Context>()
            .getSystemService(android.content.ClipboardManager::class.java).primaryClip?.getItemAt(0)?.text?.toString()
        assertEquals(FakeCore.FAKE_REPORT_ID, clip)
        compose.mainClock.advanceTimeBy(REPORT_DONE_SHOWN_MS + 500)
        compose.mainClock.advanceTimeByFrame()
        assertNull(core.client.reportUpload.value)
        assertTrue(compose.onAllNodesWithTag("report.bar").fetchSemanticsNodes().isEmpty())
    }

    @Test
    fun screenshots() {
        val dir = (System.getProperty("hocket.screenshotDir") ?: System.getenv("HOCKET_SCREENSHOT_DIR"))?.takeIf { it.isNotBlank() }?.let(::File) ?: return
        dir.mkdirs()
        var status by mutableStateOf(status(ReportUploadState.Uploading, sent = 340.0))
        compose.setContent { HocketTheme(client = null) { ReportUploadBar(status, insets = null, onCancel = {}, onRetry = {}, onDismiss = {}, onCopy = {}) } }
        for ((name, s) in listOf(
            "uploading" to status(ReportUploadState.Uploading, sent = 340.0),
            "done" to status(ReportUploadState.Done, sent = 1000.0, id = "R-20261101-7k3f"),
            "failed" to status(ReportUploadState.Failed, sent = 340.0, error = ReportUploadError.Network),
        )) {
            status = s
            compose.waitForIdle()
            val bitmap = compose.onNodeWithTag("report.bar").captureToImage().asAndroidBitmap()
            File(dir, "android-report-bar-$name.png").outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
        }
    }
}

