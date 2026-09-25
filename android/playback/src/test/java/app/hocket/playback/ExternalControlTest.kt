package app.hocket.playback

import android.app.Application
import android.content.Context
import android.os.Build
import android.os.Process
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

/**
 * "Allow control by other apps": off by default; the system's own controls always get in; the last
 * value survives a restart so an early controller is judged by it.
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [34], application = Application::class)
class ExternalControlTest {
    private val app: Application get() = ApplicationProvider.getApplicationContext()
    private val systemUiUid = 10_050
    private val otherAppUid = 10_200

    private fun control(sdk: Int = 34) = ExternalControl(app, sdk) { uid -> uid == systemUiUid }

    @Before
    fun clear() { app.getSharedPreferences(ExternalControl.PREFS, Context.MODE_PRIVATE).edit().clear().commit() }

    @Test
    fun otherAppsAreRefusedByDefault() {
        val c = control()
        assertFalse(c.allowed)
        assertFalse(c.permits("com.google.android.projection.gearhead", otherAppUid))
        assertFalse("an unknown uid is not trusted", c.permits("com.example", -1))
    }

    @Test
    fun theSystemsControlsAlwaysWork() {
        val c = control()
        assertTrue("SystemUI / Bluetooth hold MEDIA_CONTENT_CONTROL", c.permits("com.android.systemui", systemUiUid))
        assertTrue("this app", c.permits(app.packageName, Process.myUid()))
        assertFalse("an anonymous legacy controller is identifiable from API 28", c.permits(ExternalControl.LEGACY_CONTROLLER, -1))
        assertTrue("before API 28 it cannot be told apart from the system", control(Build.VERSION_CODES.O_MR1).permits(ExternalControl.LEGACY_CONTROLLER, -1))
    }

    @Test
    fun turningItOnLetsOtherAppsInAndIsRemembered() {
        val c = control()
        assertTrue(c.update(true))
        assertFalse("no change", c.update(true))
        assertTrue(c.permits("com.example.browser", otherAppUid))
        assertTrue("a restarted service starts from the last value", control().allowed)
        assertTrue(c.update(false))
        assertFalse(control().permits("com.example.browser", otherAppUid))
    }
}
