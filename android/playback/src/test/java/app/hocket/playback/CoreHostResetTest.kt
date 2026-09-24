package app.hocket.playback

import android.app.Application
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config

/** The fatal screen's "reset app data": the core's data dir and stream cache go, stored logins and the device id stay. */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [34], application = Application::class)
class CoreHostResetTest {
    @Test
    fun resetDeletesTheCoreDataAndCacheKeepsLoginsAndExits() {
        val app: Application = ApplicationProvider.getApplicationContext()
        val data = File(app.filesDir, "hocket").apply { mkdirs() }
        File(data, "hocket.sqlite").writeText("db")
        File(data, "downloads").apply { mkdirs() }.resolve("t1.flac").writeText("audio")
        val stream = File(app.cacheDir, "stream").apply { mkdirs() }
        File(stream, "chunk").writeText("x")
        val logins = app.getSharedPreferences(KeystoreCredentialStore.PREFS, 0)
        logins.edit().putString("https://music.example|alice", "iv:data").commit()
        val deviceId = CoreHost.deviceId(app)
        var exited = false
        CoreHost.resetData(app) { exited = true }
        assertTrue(exited)
        assertFalse(data.exists())
        assertFalse(stream.exists())
        assertEquals("the stored login survives a data reset", "iv:data", logins.getString("https://music.example|alice", null))
        assertEquals("the device id is not part of the reset", deviceId, CoreHost.deviceId(app))
        assertNull(CoreHost.fatalError.value)
    }
}
