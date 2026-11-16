package app.hocket

import android.annotation.SuppressLint
import android.app.Activity
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.webkit.CookieManager
import android.webkit.WebResourceRequest
import android.webkit.WebStorage
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.activity.ComponentActivity
import app.hocket.playback.YtmLogin

/**
 * YouTube Music sign-in (docs/providers.md, Phase 4): Google's sign-in page in a WebView of its
 * own, finished once it lands on music.youtube.com with a session. Its cookies for
 * music.youtube.com go back as [EXTRA_COOKIES]; the core keeps them in memory and the keystore
 * keeps them on this device only (never synced). The user agent drops WebView's `; wv` token,
 * which Google's embedded-browser check keys on (docs/spikes/manual.md, 1).
 *
 * [forCoordinator] opens it for the coordinator's own YouTube Music analysis session (Settings →
 * Connect's devices, owner only): the same cleared jar, so the coordinator never gets this
 * device's session.
 *
 * The jar (cookies and web storage, process-wide) is cleared again whenever the sign-in closes,
 * signed in, cancelled or failed, for either kind: the captured session lives in the keystore and
 * the core only, and the hidden JS host page ([app.hocket.playback.WebViewJsPage], on
 * www.youtube.com and sharing the jar) must never carry anyone's Google session. It doesn't need
 * one: its PO tokens are bound to the video id and its fetches (player, BotGuard) are anonymous.
 */
class YtmSignInActivity : ComponentActivity() {
    private lateinit var web: WebView
    private val forCoordinator get() = intent.getBooleanExtra(EXTRA_FOR_COORDINATOR, false)

    @SuppressLint("SetJavaScriptEnabled")
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        title = getString(if (forCoordinator) R.string.identity_coord_ytm_sign_in_title else R.string.setup_ytm_sign_in)
        val cookies = CookieManager.getInstance()
        // A fresh sign-in each time: the session a sign-in came from must not be reused.
        if (savedInstanceState == null) cookies.removeAllCookies(null)
        web = WebView(this)
        web.settings.javaScriptEnabled = true
        web.settings.domStorageEnabled = true
        web.settings.userAgentString = web.settings.userAgentString.replace("; wv", "")
        cookies.setAcceptThirdPartyCookies(web, true)
        web.webViewClient = object : WebViewClient() {
            override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest) = false
            override fun onPageFinished(view: WebView, url: String?) = check(url)
        }
        // Edge to edge: keep the page clear of the system bars.
        setContentView(android.widget.FrameLayout(this).apply { fitsSystemWindows = true; addView(web) })
        if (savedInstanceState == null) web.loadUrl(SIGN_IN_URL) else web.restoreState(savedInstanceState)
    }

    private fun check(url: String?) {
        if (url == null || Uri.parse(url).host != "music.youtube.com") return
        val header = CookieManager.getInstance().getCookie(YtmLogin.URL)
        if (!YtmLogin.hasSession(header)) return
        clearJar()
        setResult(Activity.RESULT_OK, Intent().putExtra(EXTRA_COOKIES, header))
        finish()
    }

    override fun onSaveInstanceState(outState: Bundle) {
        super.onSaveInstanceState(outState)
        web.saveState(outState)
    }

    override fun onDestroy() {
        web.destroy()
        // Not on a configuration change: the sign-in goes on in the next instance.
        if (isFinishing) clearJar()
        super.onDestroy()
    }

    /** Forgets every cookie and all web storage the sign-in left (see the class comment). */
    private fun clearJar() {
        val cookies = CookieManager.getInstance()
        cookies.removeAllCookies { cookies.flush() }
        WebStorage.getInstance().deleteAllData()
    }

    companion object {
        const val EXTRA_COOKIES = "cookies"
        const val EXTRA_FOR_COORDINATOR = "forCoordinator"

        /** A fresh sign-in whose session goes to the coordinator, never this device. */
        fun forCoordinator(context: Context): Intent = Intent(context, YtmSignInActivity::class.java).putExtra(EXTRA_FOR_COORDINATOR, true)
        const val SIGN_IN_URL = "https://accounts.google.com/ServiceLogin?ltmpl=music&service=youtube&passive=true&continue=https%3A%2F%2Fmusic.youtube.com%2F"
    }
}
