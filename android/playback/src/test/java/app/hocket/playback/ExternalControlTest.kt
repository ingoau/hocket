package app.hocket.playback

import android.app.Application
import android.content.Context
import android.os.Build
import android.os.Process
import android.provider.Settings
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

/**
 * "Allow control by other apps": off by default; the system's own controls and apps with
 * notification access always get in; the last value survives a restart so an early controller is
 * judged by it.
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [34], application = Application::class)
class ExternalControlTest {
    private val app: Application get() = ApplicationProvider.getApplicationContext()
    private val systemUiUid = 10_050
    private val otherAppUid = 10_200
    private val watchUid = 10_300
    private val watch = "com.example.watch"
    private var listeners = setOf<String>()

    private fun control(sdk: Int = 34) = ExternalControl(
        app, sdk,
        hasMediaContentControl = { uid -> uid == systemUiUid },
        enabledListeners = { listeners },
        ownsPackage = { uid, pkg -> uid == watchUid && pkg == watch },
    )

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

    @Test
    fun appsWithNotificationAccessAlwaysWork() {
        listeners = setOf(watch)
        val c = control()
        assertFalse(c.allowed)
        assertTrue("a notification listener calling as itself", c.permits(watch, watchUid))
        assertFalse("another uid naming the listener's package", c.permits(watch, otherAppUid))
        assertFalse("an unknown uid", c.permits(watch, -1))
        assertFalse("an app without notification access", c.permits("com.example.browser", otherAppUid))
    }

    @Test
    fun notificationAccessIsReadAgainOnRefresh() {
        val c = control()
        assertFalse(c.permits(watch, watchUid))
        listeners = setOf(watch)
        assertFalse("read once until refreshed", c.permits(watch, watchUid))
        assertTrue(c.refreshListeners())
        assertFalse("no change", c.refreshListeners())
        assertTrue(c.permits(watch, watchUid))
        listeners = emptySet()
        assertTrue(c.refreshListeners())
        assertFalse("revoked", c.permits(watch, watchUid))
    }

    @Test
    fun enabledListenersAreReadFromTheSecureSetting() {
        Settings.Secure.putString(app.contentResolver, ExternalControl.ENABLED_NOTIFICATION_LISTENERS, "$watch/$watch.Listener:org.kde.kdeconnect_tp/.Notifications:")
        assertEquals(setOf(watch, "org.kde.kdeconnect_tp"), ExternalControl.enabledListenerPackages(app))
        Settings.Secure.putString(app.contentResolver, ExternalControl.ENABLED_NOTIFICATION_LISTENERS, null)
        assertEquals(emptySet<String>(), ExternalControl.enabledListenerPackages(app))
    }
}
