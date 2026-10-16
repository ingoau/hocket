package app.hocket.ui

import android.graphics.Bitmap
import androidx.activity.ComponentActivity
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Surface
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.api.Job
import app.hocket.core.api.JobKind
import app.hocket.core.api.JobState
import app.hocket.core.api.Problem
import app.hocket.core.api.SyncProgress
import app.hocket.ui.components.JobsSheet
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.File

/**
 * The Activity sheet's sync row (design D): one line per server (latest state, age, library size),
 * not a row per past sync. Asserts each state, and writes PNGs when HOCKET_SCREENSHOT_DIR is set.
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], qualifiers = "w411dp-h891dp-night-xxhdpi")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class JobsSheetScreenshotTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val outDir: File? = (System.getProperty("hocket.screenshotDir") ?: System.getenv("HOCKET_SCREENSHOT_DIR"))
        ?.takeIf { it.isNotBlank() }?.let(::File)

    private val now = System.currentTimeMillis().toDouble()

    private fun syncJob(state: JobState, done: UInt, createdAt: Double, updatedAt: Double = createdAt) =
        Job("job-sync", JobKind.LibrarySync, "Library sync", state, done, 4357u, 0u, createdAt, updatedAt, true)

    @Test
    fun lastSyncSummary() {
        val core = show()
        // The fake last synced an hour ago, in 40 s.
        compose.onNodeWithText("Synced 1h ago").assertExists()
        compose.onNodeWithText("music.example.net · ${core.fake.library.tracks.size} tracks").assertExists()
        compose.onNodeWithContentDescription("Sync library now").assertExists()
        compose.onNodeWithText("Library sync").assertDoesNotExist()
        shot("android-dark-activity-sync-summary")
    }

    @Test
    fun syncRunning() {
        val core = show()
        core.fake.stageSync(
            jobs = listOf(syncJob(JobState.Running, 1820u, now - 12_000.0)),
            progress = SyncProgress("fake-server", "tracks", 1820u, 4357u, listOf("artists", "albums"), false),
        )
        compose.waitForIdle()
        compose.onNodeWithText("Syncing tracks").assertExists()
        compose.onNodeWithText("1820 of 4357").assertExists()
        compose.onNodeWithText("Cancel").assertExists()
        shot("android-dark-activity-sync-running")
    }

    @Test
    fun syncPausedOffersResume() {
        val core = show()
        core.fake.stageSync(
            jobs = listOf(syncJob(JobState.Paused, 1820u, now - 12_000.0)),
            progress = SyncProgress("fake-server", "tracks", 1820u, 4357u, listOf("artists", "albums"), false),
        )
        compose.waitForIdle()
        compose.onNodeWithText("Sync paused").assertExists()
        compose.onNodeWithText("1820 of 4357").assertExists()
        compose.onNodeWithText("Resume").assertExists()
        shot("android-dark-activity-sync-paused")
    }

    @Test
    fun failedAfterTheLastGoodSync() {
        val core = show()
        // Failed five minutes ago; the last successful sync is still the one an hour ago.
        core.fake.stageSync(jobs = listOf(syncJob(JobState.Failed, 900u, now - 400_000.0, updatedAt = now - 300_000.0)))
        compose.waitForIdle()
        compose.onNodeWithText("Sync failed 5m ago").assertExists()
        compose.onNodeWithText("Retry").assertExists()
        shot("android-dark-activity-sync-failed")
    }

    @Test
    fun aFailureOlderThanTheLastSyncIsHistory() {
        val core = show()
        core.fake.stageSync(jobs = listOf(syncJob(JobState.Failed, 900u, now - 7_300_000.0, updatedAt = now - 7_200_000.0)))
        compose.waitForIdle()
        compose.onNodeWithText("Synced 1h ago").assertExists()
        compose.onNodeWithText("Sync failed", substring = true).assertDoesNotExist()
    }

    @Test
    fun aQueuedRetryIgnoresTheFailedRunsProgress() {
        val core = show()
        // The failed run's last progress never reported finished; the retry hasn't started yet.
        core.fake.stageSync(
            jobs = listOf(syncJob(JobState.Queued, 0u, now - 1_000.0)),
            progress = SyncProgress("fake-server", "tracks", 900u, 4357u, listOf("artists", "albums"), false),
        )
        compose.waitForIdle()
        compose.onNodeWithText("Waiting to sync").assertExists()
        compose.onNodeWithText("900 of 4357").assertDoesNotExist()
    }

    @Test
    fun onlyTheTracksPhaseShowsACount() {
        val core = show()
        core.fake.stageSync(
            jobs = listOf(syncJob(JobState.Running, 120u, now - 5_000.0)),
            progress = SyncProgress("fake-server", "albums", 120u, 4357u, listOf("artists"), false),
        )
        compose.waitForIdle()
        compose.onNodeWithText("Syncing albums").assertExists()
        compose.onNodeWithText("music.example.net").assertExists()
        compose.onNodeWithText("120 of 4357").assertDoesNotExist()
    }

    @Test
    fun cancelledAfterTheLastGoodSync() {
        val core = show()
        core.fake.stageSync(jobs = listOf(syncJob(JobState.Cancelled, 900u, now - 400_000.0, updatedAt = now - 300_000.0)))
        compose.waitForIdle()
        compose.onNodeWithText("Sync cancelled 5m ago").assertExists()
        compose.onNodeWithContentDescription("Sync library now").assertExists()
    }

    @Test
    fun retryOnTheRowAnswersTheFailuresProblem() {
        val core = show()
        val failed = syncJob(JobState.Failed, 900u, now - 400_000.0, updatedAt = now - 300_000.0)
        core.fake.stageSync(jobs = listOf(failed))
        core.fake.stageProblems(listOf(Problem("p-sync", failed.id, "Library sync failed", null, now - 300_000.0, true)))
        compose.waitForIdle()
        compose.onNodeWithText("Library sync failed").assertExists()
        compose.onNodeWithText("Retry").performClick()
        compose.waitForIdle()
        compose.onNodeWithText("Library sync failed").assertDoesNotExist()
        compose.onNodeWithText("Sync failed", substring = true).assertDoesNotExist()
    }

    @Test
    fun neverSynced() {
        val core = show()
        core.fake.stageSync(jobs = emptyList(), servers = core.client.servers.value.map { it.copy(lastSync = null, lastSyncTracks = null) })
        compose.waitForIdle()
        compose.onNodeWithText("Not synced yet").assertExists()
        shot("android-dark-activity-never-synced")
    }

    @Test
    fun oneTrackIsSingular() {
        val core = show()
        core.fake.stageSync(servers = core.client.servers.value.map { it.copy(lastSyncTracks = 1u) })
        compose.waitForIdle()
        compose.onNodeWithText("music.example.net · 1 track").assertExists()
    }

    private fun show(): TestCore {
        val core = TestCore(startPlaying = false)
        compose.setThemedContent(core) {
            // A screen behind the sheet, as in the app (and so the activity root has a size to capture).
            Surface(Modifier.fillMaxSize()) {}
            JobsSheet(onDismiss = {})
        }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        compose.waitForIdle()
        return core
    }

    @Suppress("UNCHECKED_CAST")
    private fun shot(name: String) {
        val dir = outDir ?: return
        dir.mkdirs()
        val bitmap = compose.onAllNodes(isRoot())[0].captureToImage().asAndroidBitmap().copy(Bitmap.Config.ARGB_8888, true)
        val canvas = android.graphics.Canvas(bitmap)
        val global = Class.forName("android.view.WindowManagerGlobal").getMethod("getInstance").invoke(null)
        val views = global.javaClass.getDeclaredField("mViews").apply { isAccessible = true }.get(global) as List<android.view.View>
        for (view in views) {
            if (view === compose.activity.window.decorView || view.width == 0) continue
            val at = IntArray(2).also { view.getLocationOnScreen(it) }
            canvas.save(); canvas.translate(at[0].toFloat(), at[1].toFloat()); view.draw(canvas); canvas.restore()
        }
        File(dir, "$name.png").outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
    }
}
