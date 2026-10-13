package app.hocket.playback

import android.app.Application
import android.content.Intent
import android.content.pm.ActivityInfo
import android.content.pm.ResolveInfo
import android.media.audiofx.AudioEffect
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

/** Equalizer apps hear about the player's audio session once, and when it goes. */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [34], application = Application::class)
class AudioEffectSessionTest {
    private val app: Application get() = ApplicationProvider.getApplicationContext()
    private fun sent() = shadowOf(app).broadcastIntents.map { it.action to it.getIntExtra(AudioEffect.EXTRA_AUDIO_SESSION, -1) }

    @Test
    fun opensOncePerSessionAndClosesTheOldOneFirst() {
        val effects = AudioEffectSession(app)
        effects.onSessionIdChanged(7) // nothing announced yet: nothing to move
        effects.open(7)
        effects.open(7)
        effects.onSessionIdChanged(9)
        effects.close()
        effects.close()
        assertEquals(
            listOf(
                AudioEffect.ACTION_OPEN_AUDIO_EFFECT_CONTROL_SESSION to 7,
                AudioEffect.ACTION_CLOSE_AUDIO_EFFECT_CONTROL_SESSION to 7,
                AudioEffect.ACTION_OPEN_AUDIO_EFFECT_CONTROL_SESSION to 9,
                AudioEffect.ACTION_CLOSE_AUDIO_EFFECT_CONTROL_SESSION to 9,
            ),
            sent(),
        )
        val open = shadowOf(app).broadcastIntents.first()
        assertEquals(app.packageName, open.getStringExtra(AudioEffect.EXTRA_PACKAGE_NAME))
        assertEquals(AudioEffect.CONTENT_TYPE_MUSIC, open.getIntExtra(AudioEffect.EXTRA_CONTENT_TYPE, -1))
    }

    @Test
    fun theEqualizerLinkNamesThePlayingSessionAndOnlyExistsWhenSomethingOffersOne() {
        assertNull(AudioEffectSession.controlPanelIntent(app))
        val panel = Intent(AudioEffect.ACTION_DISPLAY_AUDIO_EFFECT_CONTROL_PANEL)
        shadowOf(app.packageManager).addResolveInfoForIntent(panel, ResolveInfo().apply { activityInfo = ActivityInfo().apply { packageName = "eq"; name = "eq.Panel" } })
        assertEquals(0, AudioEffectSession.controlPanelIntent(app)!!.getIntExtra(AudioEffect.EXTRA_AUDIO_SESSION, -1))
        val effects = AudioEffectSession(app)
        effects.open(11)
        assertEquals(11, AudioEffectSession.controlPanelIntent(app)!!.getIntExtra(AudioEffect.EXTRA_AUDIO_SESSION, -1))
        effects.close()
        assertEquals(0, AudioEffectSession.controlPanelIntent(app)!!.getIntExtra(AudioEffect.EXTRA_AUDIO_SESSION, -1))
    }
}
