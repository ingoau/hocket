package app.hocket

import android.app.SearchManager
import android.content.Intent
import android.os.Bundle
import android.provider.MediaStore
import androidx.activity.ComponentActivity
import androidx.lifecycle.lifecycleScope
import app.hocket.playback.ExternalControl
import app.hocket.playback.LibraryBrowser
import kotlinx.coroutines.flow.filterNotNull
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull

/**
 * The Assistant discovery intent, `MEDIA_PLAY_FROM_SEARCH` ("play Blue Train on Hocket", also what
 * makes Hocket a choice among the phone's music apps). No UI of its own: it resolves the request the
 * way the media session resolves a "play …" from Android Auto ([LibraryBrowser.resolveSearch]: the
 * focus extras pick an artist, album, playlist or genre; an empty query resumes), plays it, and
 * finishes.
 *
 * The activity has to be exported, so any app can send this intent, and the sender cannot be told
 * apart. Playing straight away therefore follows "Allow control by other apps" ([ExternalControl]):
 * with it off (the default), when nothing matches, or when the core is not ready soon enough (signed
 * out, a slow start), the app opens on search with the query typed in, and the user decides.
 *
 * Its window is transparent and lets touches through (it is never in the way while it waits), and
 * it stays until playback has started, briefly: Media3 then promotes the service to the foreground
 * while the app is still in front, which a background app may not do.
 */
class MediaSearchActivity : ComponentActivity() {
    companion object {
        /** For the core and its server (a cold start). */
        private const val READY_MS = 5_000L
        /** For the played item to start, before finishing anyway. */
        private const val PLAYING_MS = 5_000L
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.addFlags(android.view.WindowManager.LayoutParams.FLAG_NOT_TOUCHABLE)
        if (savedInstanceState != null) { finish(); return }
        if (intent?.action != MediaStore.INTENT_ACTION_MEDIA_PLAY_FROM_SEARCH) { finish(); return }
        val query = intent.getStringExtra(SearchManager.QUERY).orEmpty()
        if (!ExternalControl(this).allowed) { openSearch(query); return }
        val app = application as HocketApp
        val extras = intent.extras
        lifecycleScope.launch {
            val client = withTimeoutOrNull(READY_MS) {
                app.client.filterNotNull().first().also { c -> c.servers.first { it.isNotEmpty() } }
            }
            val server = client?.servers?.value?.firstOrNull()?.id
            if (client == null || server == null) { openSearch(query); return@launch }
            val browser = LibraryBrowser(this@MediaSearchActivity, { client.core }, { server }, app.scope)
            val item = browser.resolveSearch(query, extras)
            if (item == null || !browser.playNow(listOf(item.mediaId), 0)) { openSearch(query); return@launch }
            withTimeoutOrNull(PLAYING_MS) { client.mediaSession.first { it.isPlaying } }
            finish()
        }
    }

    private fun openSearch(query: String) {
        AppLinks.openSearch(query)
        startActivity(Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP))
        finish()
    }
}
