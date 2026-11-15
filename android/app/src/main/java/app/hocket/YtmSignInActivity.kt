package app.hocket

import android.annotation.SuppressLint
import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.webkit.CookieManager
import android.webkit.WebResourceRequest
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
 */
class YtmSignInActivity : ComponentActivity() {
    private lateinit var web: WebView

    @SuppressLint("SetJavaScriptEnabled")
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        title = getString(R.string.setup_ytm_sign_in)
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
        CookieManager.getInstance().flush()
        setResult(Activity.RESULT_OK, Intent().putExtra(EXTRA_COOKIES, header))
        finish()
    }

    override fun onSaveInstanceState(outState: Bundle) {
        super.onSaveInstanceState(outState)
        web.saveState(outState)
    }

    override fun onDestroy() {
        web.destroy()
        super.onDestroy()
    }

    companion object {
        const val EXTRA_COOKIES = "cookies"
        const val SIGN_IN_URL = "https://accounts.google.com/ServiceLogin?ltmpl=music&service=youtube&passive=true&continue=https%3A%2F%2Fmusic.youtube.com%2F"
    }
}
