package app.hocket.playback

import android.app.Application
import android.content.Intent
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.LifecycleRegistry
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

/**
 * The UI-driven service binding: bound while UI is started, unbound when it stops (so the idle stop
 * and task removal can end the service), started plainly (never as a foreground service), and only
 * the in-process bind (with the per-process token) counts as a core client.
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [34], application = Application::class)
class ServiceLifecycleTest {
    private val app: Application get() = ApplicationProvider.getApplicationContext()

    private class Owner : LifecycleOwner {
        override val lifecycle: Lifecycle by lazy { LifecycleRegistry(this) }
    }

    @Test
    fun boundOnlyWhileTheProcessHasStartedUi() {
        val connection = PlaybackServiceConnection(app)
        val binder = ForegroundBinder(connection)
        val owner = Owner()
        assertFalse(connection.bound)

        binder.onStart(owner)
        assertTrue(connection.bound)
        val shadow = shadowOf(app)
        assertEquals(1, shadow.boundServiceConnections.size)
        val started = shadow.nextStartedService
        assertEquals(PlaybackService::class.java.name, started.component?.className)
        assertNull("a plain start intent (Media3 promotes to foreground itself); never the bind action", started.action)
        binder.onStart(owner)
        assertEquals("a second start does not bind twice", 1, shadow.boundServiceConnections.size)

        binder.onStop(owner)
        assertFalse(connection.bound)
        assertEquals(1, shadow.unboundServiceConnections.size)
    }

    @Test
    fun onlyTheInProcessBindIsALocalBind() {
        val own = PlaybackServiceConnection.bindIntent(app)
        assertTrue(PlaybackService.isLocalBind(own))
        val foreign = Intent(app, PlaybackService::class.java).setAction(PlaybackService.ACTION_BIND_CORE)
        assertFalse("BIND_CORE without the token is not a client", PlaybackService.isLocalBind(foreign))
        val guessed = Intent(foreign).putExtra(PlaybackService.EXTRA_BIND_TOKEN, "not-the-token")
        assertFalse(PlaybackService.isLocalBind(guessed))
        assertFalse("Media3 controller binds are not clients", PlaybackService.isLocalBind(Intent("androidx.media3.session.MediaSessionService")))
        assertFalse(PlaybackService.isLocalBind(null))
        val tokenOnlyInExtras = Intent(foreign).putExtra(PlaybackService.EXTRA_BIND_TOKEN, CoreHost.bindToken)
        assertFalse("the token must be in the data too", PlaybackService.isLocalBind(tokenOnlyInExtras))
        // The system caches onBind's answer per filterEquals (extras ignored): a foreign bind with
        // the same action must never share the in-process bind's cache entry, either way round.
        assertFalse(own.filterEquals(foreign))
        assertFalse(own.filterEquals(guessed))
        assertTrue(own.filterEquals(PlaybackServiceConnection.bindIntent(app)))
    }

    @Test
    fun theMediaSessionIsAddedToTheServiceOnCreate() {
        // The UI binds for the core, never as a Media3 controller, so without an explicit addSession
        // Media3 never posts the media notification or promotes the service to the foreground.
        CoreHost.forceFake = true
        val controller = org.robolectric.Robolectric.buildService(PlaybackService::class.java).create()
        try {
            val service = controller.get()
            assertEquals(1, service.sessions.size)
            assertTrue(service.isSessionAdded(service.sessions.single()))
        } finally {
            controller.destroy()
            CoreHost.forceFake = false
        }
    }
}
