package app.hocket.playback

import android.app.Application
import android.os.Looper
import androidx.media3.common.DeviceInfo as PlayerDeviceInfo
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.api.DeviceInfo
import app.hocket.core.api.MediaSessionAction
import app.hocket.core.api.MediaSessionMetadata
import app.hocket.core.api.MediaSessionState
import app.hocket.core.api.Platform
import app.hocket.core.api.PositionStamp
import app.hocket.core.api.RepeatMode
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

/** Which Connect device the system shows as the session's output, and how the session reports it. */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [34], application = Application::class)
class ConnectRoutesTest {
    private fun state(owns: Boolean, loaded: Boolean = true) = MediaSessionState(
        if (loaded) MediaSessionMetadata("Song", "Artist", "Album", 200_000u, null, "t1", false, 0u) else null, true,
        PositionStamp(1000u, 0.0, 1.0, true), false, RepeatMode.Off, 1.0,
        listOf(MediaSessionAction.Play, MediaSessionAction.Pause), owns,
    )

    private fun device(id: String, playing: Boolean = false, self: Boolean = false, platform: Platform = Platform.MacOs) =
        DeviceInfo(id, "Device $id", platform, "1.0", playing, false, 0.0, self)

    private val phone = device("phone", self = true, platform = Platform.Android)
    private val laptop = device("laptop", playing = true)
    private val desktop = device("desktop")

    @After
    fun tearDown() = ConnectRoutes.clear()

    @Test
    fun remoteIsTheOwnerWhenAnotherDevicePlays() {
        val devices = listOf(phone, laptop, desktop)
        assertEquals(laptop, ConnectRoutes.remotePlayback(state(owns = false), devices, "laptop"))
        assertEquals("the lease owner wins over a stale playing flag", desktop, ConnectRoutes.remotePlayback(state(owns = false), devices, "desktop"))
        assertEquals("no owner reported yet: whoever plays", laptop, ConnectRoutes.remotePlayback(state(owns = false), devices, null))
        assertNull("this device plays", ConnectRoutes.remotePlayback(state(owns = true), devices, "phone"))
        assertNull("nothing loaded", ConnectRoutes.remotePlayback(state(owns = false, loaded = false), devices, "laptop"))
        assertNull("never this device", ConnectRoutes.remotePlayback(state(owns = false), listOf(phone.copy(playing = true)), "phone"))
    }

    @Test
    fun stateListsOtherDevicesAndTheRemote() {
        ConnectRoutes.onDevices(listOf(phone, laptop, desktop, device("relay", platform = Platform.Coordinator)))
        ConnectRoutes.onOwner("laptop")
        ConnectRoutes.onMediaSession(state(owns = false))
        val s = ConnectRoutes.state.value
        assertEquals(listOf(laptop, desktop), s.devices)
        assertEquals(laptop, s.remote)
        assertEquals("phone", s.selfId)
        ConnectRoutes.onOwner("phone")
        ConnectRoutes.onMediaSession(state(owns = true))
        assertNull(ConnectRoutes.state.value.remote)
    }

    @Test
    fun remotePlaybackCarriesTheRoutingSessionId() {
        val player = CoreSessionPlayer(Looper.getMainLooper(), {})
        val context = ApplicationProvider.getApplicationContext<android.content.Context>()
        val scope = kotlinx.coroutines.CoroutineScope(kotlinx.coroutines.Dispatchers.Unconfined)
        val bridge = MediaSessionBridge(context, player, {}, null, LibraryBrowser(context, { error("unused") }, { null }, scope), scope, ExternalControl(context))
        try {
            bridge.apply(state(owns = false), 0.0, remote = true)
            shadowOf(Looper.getMainLooper()).idle()
            assertEquals(PlayerDeviceInfo.PLAYBACK_TYPE_REMOTE, player.deviceInfo.playbackType)
            assertEquals(ConnectRoutes.SESSION_ID, player.deviceInfo.routingControllerId)
            // Media3 turns this into the platform session's remote PlaybackInfo with this id as its
            // volume control id, which SystemUI matches to the routing session (not observable under
            // Robolectric, whose MediaController has no PlaybackInfo).
            bridge.apply(state(owns = true), 0.0, remote = false)
            shadowOf(Looper.getMainLooper()).idle()
            assertEquals(PlayerDeviceInfo.PLAYBACK_TYPE_LOCAL, player.deviceInfo.playbackType)
        } finally {
            bridge.release()
        }
    }
}
