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
        assertEquals(MediaSessionAction.Pause, WidgetActionReceiver.command(WidgetActionReceiver.ACTION_PLAY_PAUSE, playing))
        assertEquals(MediaSessionAction.Play, WidgetActionReceiver.command(WidgetActionReceiver.ACTION_PLAY_PAUSE, playing.copy(playing = false)))
        assertEquals(MediaSessionAction.Next, WidgetActionReceiver.command(WidgetActionReceiver.ACTION_NEXT, playing))
        assertEquals(MediaSessionAction.Previous, WidgetActionReceiver.command(WidgetActionReceiver.ACTION_PREVIOUS, playing))
        assertNull(WidgetActionReceiver.command("something.else", playing))
    }
}
