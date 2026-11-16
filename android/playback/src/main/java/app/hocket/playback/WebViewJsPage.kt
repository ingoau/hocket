package app.hocket.playback

import android.annotation.SuppressLint
import android.content.Context
import android.os.Handler
import android.os.Looper
import android.webkit.JavascriptInterface
import android.webkit.WebView
import android.webkit.WebViewClient
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeout
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicLong

/**
 * The YouTube Music JS host's page: a WebView that is never shown, on https://www.youtube.com (a
 * blank document with that base URL, as NewPipe's PoTokenWebView does), so the host script's fetches
 * are same-origin. Results come back through a JavaScript interface, since `evaluateJavascript`
 * can't await a promise. Main thread inside; callable from any.
 */
class WebViewJsPage private constructor(private val web: WebView) : JsPage {
    private val pending = ConcurrentHashMap<Long, CompletableDeferred<String>>()
    private val ids = AtomicLong()
    @Volatile override var closed = false
        private set

    private inner class Bridge {
        @JavascriptInterface
        fun done(id: String, json: String) {
            pending.remove(id.toLongOrNull())?.complete(json)
        }

        @JavascriptInterface
        fun fail(id: String, message: String) {
            pending.remove(id.toLongOrNull())?.completeExceptionally(IllegalStateException(message))
        }
    }

    override suspend fun load(script: String) {
        call { id -> "$script\n;$BRIDGE.done('$id', 'null');" }
    }

    override suspend fun run(code: String): String = call { id ->
        "(async () => { try { const v = await ($code); $BRIDGE.done('$id', JSON.stringify(v === undefined ? null : v)); }" +
            " catch (e) { $BRIDGE.fail('$id', String(e && e.message ? e.message : e)); } })();"
    }

    private suspend fun call(wrap: (Long) -> String): String {
        check(!closed) { "the JS host page is gone" }
        val id = ids.incrementAndGet()
        val answer = CompletableDeferred<String>()
        pending[id] = answer
        try {
            withContext(Dispatchers.Main) { web.evaluateJavascript(wrap(id), null) }
            return withTimeout(TIMEOUT_MS) { answer.await() }
        } finally {
            pending.remove(id)
        }
    }

    override fun close() {
        if (closed) return
        closed = true
        pending.values.forEach { it.completeExceptionally(IllegalStateException("the JS host page closed")) }
        pending.clear()
        // Not `web.post`: a WebView that was never attached to a window queues its posts until it
        // is, so destroy would never run and every closed page would leak its renderer.
        Handler(Looper.getMainLooper()).post { web.destroy() }
    }

    companion object {
        private const val BRIDGE = "HocketJs"
        private const val TIMEOUT_MS = 18_000L
        private const val ORIGIN = "https://www.youtube.com/"

        @SuppressLint("SetJavaScriptEnabled")
        suspend fun open(context: Context): JsPage = withContext(Dispatchers.Main) {
            val web = WebView(context.applicationContext)
            web.settings.javaScriptEnabled = true
            web.settings.domStorageEnabled = true
            val loaded = CompletableDeferred<Unit>()
            web.webViewClient = object : WebViewClient() {
                override fun onPageFinished(view: WebView, url: String?) {
                    loaded.complete(Unit)
                }
            }
            val page = WebViewJsPage(web)
            web.addJavascriptInterface(page.Bridge(), BRIDGE)
            web.loadDataWithBaseURL(ORIGIN, "<!DOCTYPE html><html><head></head><body></body></html>", "text/html", "utf-8", null)
            try {
                withTimeout(TIMEOUT_MS) { loaded.await() }
            } catch (e: Throwable) {
                // A page that never loaded (or an open that was cancelled) is never handed out.
                page.closed = true
                web.destroy()
                throw e
            }
            page
        }
    }
}
