package app.hocket.playback

import android.app.Application
import android.app.Service
import android.content.Intent
import android.os.IBinder
import androidx.test.ext.junit.runners.AndroidJUnit4
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

/** A media key's foreground start is always honoured, and let go of when nothing plays. */
@OptIn(ExperimentalCoroutinesApi::class)
@RunWith(AndroidJUnit4::class)
@Config(sdk = [34], application = Application::class)
class ForegroundHoldTest {
    class Host : Service() { override fun onBind(intent: Intent?): IBinder? = null }

    private val service = Robolectric.setupService(Host::class.java)
    private val scope = TestScope(StandardTestDispatcher())
    private var ongoing = false
    private val hold = ForegroundHold(service, scope) { ongoing }

    @Test
    fun goesForegroundAtOnceAndLetsGoWhenNothingPlays() {
        hold.hold()
        assertNotNull("foreground straight away", shadowOf(service).lastForegroundNotification)
        assertFalse(shadowOf(service).isForegroundStopped)
        scope.advanceTimeBy(ForegroundHold.HOLD_MS + 1_000)
        scope.runCurrent()
        assertTrue(shadowOf(service).isForegroundStopped)
    }

    @Test
    fun handsOverToPlaybackWithoutStoppingTheForeground() {
        hold.hold()
        ongoing = true
        scope.advanceTimeBy(1_000)
        scope.runCurrent()
        assertFalse("Media3's notification holds it now", shadowOf(service).isForegroundStopped)
    }

    @Test
    fun nothingToDoWhilePlaybackAlreadyHoldsIt() {
        ongoing = true
        hold.hold()
        assertNull(shadowOf(service).lastForegroundNotification)
    }
}
