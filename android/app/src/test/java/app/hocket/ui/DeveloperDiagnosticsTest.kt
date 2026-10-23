package app.hocket.ui

import android.graphics.Bitmap
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.assertIsOff
import androidx.compose.ui.test.assertIsOn
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performSemanticsAction
import androidx.navigation.compose.rememberNavController
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.Commands
import app.hocket.core.DevLog
import app.hocket.core.SettingKeys
import app.hocket.diagnostics.DeveloperTriggers
import app.hocket.diagnostics.DiagnosticsReport
import app.hocket.ui.nav.AppRoot
import app.hocket.ui.screens.settings.DeveloperDiagnosticsScreen
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.File

/**
 * Developer mode: Settings → Advanced holds the switch and the way in; a shake (here, the request
 * the shake detector makes) opens developer diagnostics only while the mode is on; the page shows
 * the report. Writes PNGs of Advanced and the page in both themes when HOCKET_SCREENSHOT_DIR is set.
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp-xxhdpi")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class DeveloperDiagnosticsTest {
    @get:Rule
    val compose = createComposeRule()

    private val outDir: File? = (System.getProperty("hocket.screenshotDir") ?: System.getenv("HOCKET_SCREENSHOT_DIR"))
        ?.takeIf { it.isNotBlank() }?.let(::File)

    private fun awaitTag(tag: String) {
        compose.waitUntil(5_000) { compose.onAllNodesWithTag(tag).fetchSemanticsNodes().isNotEmpty() }
        compose.waitForIdle()
    }

    @Test
    fun aShakeOpensDiagnosticsOnlyInDeveloperMode() {
        val core = TestCore(startPlaying = false)
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        compose.openSettingsFromAccount()

        // Off: a shake does nothing.
        DeveloperTriggers.request()
        compose.waitForIdle()
        assertTrue(compose.onAllNodesWithTag("developer.modeCard").fetchSemanticsNodes().isEmpty())

        // Advanced has the switch and the way in.
        compose.onNodeWithTag("settings.category.advanced").performScrollTo().performSemanticsAction(SemanticsActions.OnClick)
        awaitTag("setting.developer.mode")
        compose.onNodeWithTag("setting.developer.mode").assertIsOff().performClick()
        compose.waitUntil(5_000) { core.client.settings.value[SettingKeys.DEVELOPER_MODE]?.value == "true" }
        compose.onNodeWithTag("setting.developer.mode").assertIsOn()
        compose.onNodeWithTag("settings.back").performClick()
        awaitTag("settings.categories")

        // On: a shake opens the page, which shows the report.
        DevLog.append("warn", "kotlin/Test", "marker-from-the-test")
        DeveloperTriggers.request()
        awaitTag("developer.modeCard")
        compose.onNodeWithTag("developer.modeSwitch").assertIsOn()
        compose.waitUntil(10_000) { runCatching { compose.onNodeWithTag("developer.head", useUnmergedTree = true).assertExists() ; textOf("developer.head").startsWith("Hocket diagnostics") }.getOrDefault(false) }
        assertTrue(textOf("developer.head").contains("Hocket fake core"))
        compose.waitUntil(5_000) { textOf("developer.log").contains("marker-from-the-test") }
        // A second shake on the page doesn't stack another copy.
        DeveloperTriggers.request()
        compose.waitForIdle()
        compose.onNodeWithTag("settings.back").performClick()
        awaitTag("settings.categories")
    }

    @Test
    fun theOtherDevicesAreCollectedOnlyWhenSwitchedOn() {
        val core = TestCore(startPlaying = false)
        core.fake.dispatch(Commands.setSetting(SettingKeys.DEVELOPER_MODE, "true"))
        var asked = 0
        compose.setThemedContent(core) {
            DeveloperDiagnosticsScreen(rememberNavController(), collect = { _, _ -> sampleReport() }, collectOthers = { asked++; "Other devices: the phone says hi\n" })
        }
        core.start()
        awaitTag("developer.allDevicesSwitch")
        compose.onNodeWithTag("developer.copy").performClick()
        compose.waitUntil(5_000) { clipboardText()?.contains("Hocket diagnostics") == true }
        assertEquals(0, asked)
        compose.onNodeWithTag("developer.allDevicesSwitch").assertIsOff().performClick()
        compose.onNodeWithTag("developer.copy").performClick()
        compose.waitUntil(5_000) { clipboardText()?.contains("the phone says hi") == true }
        assertEquals(1, asked)
    }

    private fun clipboardText(): String? =
        androidx.test.core.app.ApplicationProvider.getApplicationContext<android.content.Context>()
            .getSystemService(android.content.ClipboardManager::class.java).primaryClip?.getItemAt(0)?.text?.toString()

    private fun textOf(tag: String): String =
        compose.onNodeWithTag(tag, useUnmergedTree = true).fetchSemanticsNode().config[androidx.compose.ui.semantics.SemanticsProperties.Text].joinToString("") { it.text }

    @Test
    @Config(qualifiers = "+notnight")
    fun screenshotsLight() = shoot("light")

    @Test
    @Config(qualifiers = "+night")
    fun screenshotsDark() = shoot("dark")

    private fun shoot(theme: String) {
        val dir = outDir ?: return
        dir.mkdirs()
        val core = TestCore(startPlaying = false)
        core.fake.dispatch(Commands.setSetting(SettingKeys.DEVELOPER_MODE, "true"))
        compose.setThemedContent(core) { DeveloperDiagnosticsScreen(rememberNavController(), collect = { _, _ -> sampleReport() }) }
        core.start()
        awaitTag("developer.stats")
        compose.mainClock.advanceTimeBy(1_000)
        save(compose.onRoot().captureToImage().asAndroidBitmap(), File(dir, "android-$theme-developer-diagnostics.png"))
        compose.onNodeWithTag("developer.log", useUnmergedTree = true).performScrollTo()
        compose.mainClock.advanceTimeBy(1_000)
        save(compose.onRoot().captureToImage().asAndroidBitmap(), File(dir, "android-$theme-developer-diagnostics-2.png"))
    }

    /** A report as a phone would collect it: real-looking head, core log and logcat lines. */
    private fun sampleReport(): DiagnosticsReport {
        val t0 = 1_792_465_445_000L
        val head = """
            Hocket diagnostics
            ==================
            generated       2026-10-20T14:04:05+11:00
            app             0.1.0 (1) debug · app.hocket
            core            native 0.1.0 (Native)
            device          Google Pixel 9 (tokay, tokay)
            android         16 (API 36, patch 2026-10-05)
            abis            arm64-v8a
            locale          en-AU · Australia/Sydney
            display         1080x2424 px, 420 dpi, font scale 1.0
            memory          3120.4 MiB free of 11724.9 MiB; heap 38.2 MiB of 256.0 MiB; class 256 MiB
            storage         81234.5 MiB free of 117712.0 MiB
            power           saver false, ignoring battery optimisations false, idle false
            network         wifi validated
            process         pid 12873, up 1834 s, device up 402118 s
            developer mode  on (detailed logging)
            log             60 min window; core buffer

            Core
            ----
            Hocket core 0.1.0 (api schema 14)
            device: Pixel 9 (a3f0c1) platform Android app 0.1.0
            audio: External backend=external gapless=true device=None
            developer mode: true (detailed logging true)
            server: Home https://music.example.net user=ingo reachable=true version=Some("0.63.0")
            connect: tier=Lan connected=true peers=1 offset=3ms rtt=Some(12.0) error=None
            playback: key=Some(42) playing=true pos=73412 loaded=true
        """.trimIndent() + "\n"
        val lines = listOf(
            Triple("info", "hocket", "log capture started (pid 12873, core 0.1.0)"),
            Triple("info", "hocket", "developer mode on: detailed logging"),
            Triple("debug", "hocket_core::subsonic::client", "GET getAlbumList2 type=newest size=40 status=200 ms=84"),
            Triple("debug", "hocket_core::db::sync", "page tracks offset=4000 got=357"),
            Triple("info", "hocket_core::db::sync", "library sync finished tracks=4357 albums=389 ms=6120"),
            Triple("debug", "hocket_core::connect::engine", "lease renewed epoch=7 holder=a3f0c1"),
            Triple("debug", "hocket_core::audio::external", "backend report Position { ms: 73412 }"),
            Triple("warn", "hocket_core::outbox", "scrobble retry in 30s attempt=2 error=network"),
            Triple("info", "kotlin/PlaybackService", "audio focus gained"),
            Triple("debug", "hocket_core::cache::images", "miss al-389 size=300 fetched ms=41"),
            Triple("error", "crash", "uncaught on thread 'main': java.lang.IllegalStateException: example"),
        )
        val coreLog = lines.mapIndexed { i, (level, tag, message) -> DevLog.formatLine(t0 + i * 1_700L, level, tag, message) }.joinToString("")
        val logcat = "10-20 14:04:05.112 12873 12873 I ExoPlayerImpl: Init 1a2b3c [AndroidXMedia3/1.8.0]\n"
        return DiagnosticsReport(t0 + 20_000, head, coreLog, logcat, DevLog.WINDOW_MS)
    }

    private fun save(bitmap: Bitmap, file: File) {
        file.outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
        println("screenshot: ${file.absolutePath} (${bitmap.width}x${bitmap.height})")
    }
}
