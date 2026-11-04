package app.hocket.diagnostics

import android.view.KeyEvent
import app.hocket.core.DevLog
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class)
class DiagnosticsReportTest {
    private val report = DiagnosticsReport(
        generatedAt = 1_792_465_445_000,
        head = "Hocket diagnostics\n==================\napp  0.1.0\n",
        coreLog = (1..300).joinToString("") { "2026-10-20T03:04:05.000Z  INFO hocket_core: line $it\n" },
        logcat = "10-20 03:04:05.000  1234  1234 I Hocket: hello\n",
        windowMs = DevLog.WINDOW_MS,
    )

    @Test
    fun theTextCarriesTheHeadBothLogsAndTheirWindow() {
        val text = report.text
        assertTrue(text.startsWith("Hocket diagnostics\n"))
        assertTrue(text.contains("Core log (last 60 minutes)\n"))
        assertTrue(text.contains("hocket_core: line 300\n"))
        assertTrue(text.contains("Logcat (last 60 minutes)\n"))
        assertTrue(text.endsWith("I Hocket: hello\n"))
        assertEquals(300, report.coreLines)
        assertEquals(1, report.logcatLines)
        assertTrue(report.fileName.matches(Regex("hocket-diagnostics-\\d{8}-\\d{6}\\.txt")))
    }

    @Test
    fun theOtherDevicesComeLastAndOnlyWhenCollected() {
        assertFalse(report.text.contains("Other devices"))
        val all = report.copy(otherDevices = "Other devices in the Connect room\n\nRoom\n----\nmembers (2):\n")
        assertTrue(all.text.endsWith("members (2):\n"))
        assertTrue(all.text.indexOf("Other devices") > all.text.indexOf("I Hocket: hello"))
    }

    @Test
    fun thePreviewIsTheNewestLines() {
        val tail = report.coreTail(2).lines()
        assertEquals(listOf("2026-10-20T03:04:05.000Z  INFO hocket_core: line 299", "2026-10-20T03:04:05.000Z  INFO hocket_core: line 300"), tail)
    }

    @Test
    fun emptyLogsSaySo() {
        val empty = report.copy(coreLog = "", logcat = "")
        assertTrue(empty.text.contains("-----\n(empty)\n\nLogcat"))
        assertEquals(0, empty.coreLines)
    }

    @Test
    fun linesMatchTheCoresShape() {
        assertEquals("2000-02-29T00:00:00.123Z  INFO kotlin/Test: hi\n", DevLog.formatLine(951_782_400_123, "info", "kotlin/Test", "hi"))
        assertEquals("1970-01-01T00:00:00.000Z ERROR crash: x\n", DevLog.formatLine(0, "error", "crash", "x"))
    }

    @Test
    fun theKeyboardShortcutIsOnlyTakenWhileSomethingListens() {
        fun ctrlShiftD() = KeyEvent(0, 0, KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_D, 0, KeyEvent.META_CTRL_ON or KeyEvent.META_SHIFT_ON)
        assertFalse("nothing listens: the key goes on", DeveloperTriggers.onKeyDown(ctrlShiftD()))
        val scope = CoroutineScope(Dispatchers.Unconfined)
        var opened = 0
        scope.launch { DeveloperTriggers.requests.collect { opened++ } }
        try {
            assertTrue(DeveloperTriggers.onKeyDown(ctrlShiftD()))
            assertFalse(DeveloperTriggers.onKeyDown(KeyEvent(0, 0, KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_D, 0, KeyEvent.META_CTRL_ON)))
            assertFalse(DeveloperTriggers.onKeyDown(KeyEvent(0, 0, KeyEvent.ACTION_UP, KeyEvent.KEYCODE_D, 0, KeyEvent.META_CTRL_ON or KeyEvent.META_SHIFT_ON)))
            assertEquals(1, opened)
        } finally {
            scope.cancel()
        }
    }
}
