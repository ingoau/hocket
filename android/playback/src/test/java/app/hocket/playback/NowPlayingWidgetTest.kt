package app.hocket.playback

import android.app.Application
import android.view.View
import android.widget.FrameLayout
import android.widget.TextView
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.api.MediaSessionAction
import app.hocket.core.api.MediaSessionMetadata
import app.hocket.core.api.MediaSessionState
import app.hocket.core.api.PositionStamp
import app.hocket.core.api.RepeatMode
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

/** The widget's state, its persistence, what it draws and what its buttons send. */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [34], application = Application::class)
class NowPlayingWidgetTest {
    private val app: Application get() = ApplicationProvider.getApplicationContext()
    private fun session(playing: Boolean) = MediaSessionState(
        MediaSessionMetadata("Blue Train", "John Coltrane", "Blue Train", 643_000u, null, "t1", false, 0u),
        playing, PositionStamp(0u, 0.0, 1.0, playing), false, RepeatMode.Off, 1.0, emptyList(), true,
    )

    @Test
    fun stateFollowsTheSessionAndSurvivesARestart() {
        val state = WidgetState.of(session(playing = true), remoteDevice = "MacBook")
        assertEquals(WidgetState("Blue Train", "John Coltrane", null, true, "MacBook"), state)
        assertFalse(WidgetState.of(session(true).copy(metadata = null), "MacBook").hasTrack)
        state.save(app)
        assertEquals(state, WidgetState.load(app))
    }

    @Test
    fun drawsTheTrackAndThePlayingDevice() {
        val root = NowPlayingWidget.views(app, WidgetState("Blue Train", "John Coltrane", null, true, "MacBook")).apply(app, FrameLayout(app))
        assertEquals("Blue Train", root.findViewById<TextView>(R.id.widget_title).text.toString())
        assertEquals("Playing on MacBook", root.findViewById<TextView>(R.id.widget_device).text.toString())
        assertEquals("Pause", root.findViewById<View>(R.id.widget_play).contentDescription)
        assertEquals(View.VISIBLE, root.findViewById<View>(R.id.widget_next).visibility)

        val local = NowPlayingWidget.views(app, WidgetState("Blue Train", null, null, false, null)).apply(app, FrameLayout(app))
        assertEquals("On this device", local.findViewById<TextView>(R.id.widget_device).text.toString())
        assertEquals(View.GONE, local.findViewById<View>(R.id.widget_artist).visibility)

        val empty = NowPlayingWidget.views(app, WidgetState()).apply(app, FrameLayout(app))
        assertEquals("Nothing playing", empty.findViewById<TextView>(R.id.widget_title).text.toString())
        assertEquals(View.GONE, empty.findViewById<View>(R.id.widget_device).visibility)
        assertEquals(View.GONE, empty.findViewById<View>(R.id.widget_next).visibility)
    }

    @Test
    fun buttonsMapToSessionCommands() {
        val playing = WidgetState(title = "x", playing = true)
        assertEquals(MediaSessionAction.Pause, WidgetActionReceiver.command(WidgetActionReceiver.ACTION_PLAY_PAUSE, playing, running = true))
        assertEquals(MediaSessionAction.Play, WidgetActionReceiver.command(WidgetActionReceiver.ACTION_PLAY_PAUSE, playing.copy(playing = false), running = true))
        assertEquals(MediaSessionAction.Next, WidgetActionReceiver.command(WidgetActionReceiver.ACTION_NEXT, playing, running = true))
        assertEquals(MediaSessionAction.Previous, WidgetActionReceiver.command(WidgetActionReceiver.ACTION_PREVIOUS, playing, running = true))
        assertNull(WidgetActionReceiver.command("something.else", playing, running = true))
        // Saved as playing, but the process died without onDestroy (a reboot): play, not pause.
        assertEquals(MediaSessionAction.Play, WidgetActionReceiver.command(WidgetActionReceiver.ACTION_PLAY_PAUSE, playing, running = false))
    }

    @Test
    fun aStoppedServiceIsDrawnAsNotPlaying() {
        val stored = WidgetState("Blue Train", null, null, true, "MacBook")
        assertEquals(stored, NowPlayingWidget.shown(stored, running = true))
        assertEquals(stored.copy(playing = false, remoteDevice = null), NowPlayingWidget.shown(stored, running = false))
    }

    @Test
    fun theMediaKeyReceiverOnlyStartsTheServiceWithSomethingToResume() {
        val receiver = HocketMediaButtonReceiver()
        val start = android.content.Intent(android.content.Intent.ACTION_MEDIA_BUTTON)
        WidgetState().save(app)
        assertFalse(receiver.shouldStartForegroundServiceForTest(start))
        WidgetState(title = "Blue Train").save(app)
        assertTrue(receiver.shouldStartForegroundServiceForTest(start))
    }

    private fun HocketMediaButtonReceiver.shouldStartForegroundServiceForTest(intent: android.content.Intent): Boolean {
        val method = HocketMediaButtonReceiver::class.java.getDeclaredMethod("shouldStartForegroundService", android.content.Context::class.java, android.content.Intent::class.java)
        method.isAccessible = true
        return method.invoke(this, app, intent) as Boolean
    }

    @Test
    fun coversAreScaledToTheWidgetSize() {
        val file = java.io.File(app.cacheDir, "cover.png")
        file.outputStream().use { android.graphics.Bitmap.createBitmap(1000, 600, android.graphics.Bitmap.Config.ARGB_8888).compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it) }
        val art = NowPlayingWidget.decode(file.path)!!
        assertTrue("longer side ${art.width} at most 256", art.width <= 256 && art.height <= 256)
        assertNull(NowPlayingWidget.decode(java.io.File(app.cacheDir, "gone.png").path))
    }
}
