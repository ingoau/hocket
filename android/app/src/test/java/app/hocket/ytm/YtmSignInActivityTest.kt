package app.hocket.ytm

import android.webkit.CookieManager
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.YtmSignInActivity
import org.junit.Assert.assertNull
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.annotation.Config

/** The sign-in leaves no Google session in the process-wide jar the JS host page shares. */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class)
class YtmSignInActivityTest {
    @Test
    fun aCancelledCoordinatorSignInClearsTheJar() {
        val context = androidx.test.core.app.ApplicationProvider.getApplicationContext<android.content.Context>()
        val controller = Robolectric.buildActivity(YtmSignInActivity::class.java, YtmSignInActivity.forCoordinator(context)).setup()
        CookieManager.getInstance().setCookie("https://music.youtube.com", "SAPISID=coordinator")
        // Back: no result, the activity finishes.
        controller.get().finish()
        controller.pause().stop().destroy()
        assertNull(CookieManager.getInstance().getCookie("https://music.youtube.com"))
    }
}
