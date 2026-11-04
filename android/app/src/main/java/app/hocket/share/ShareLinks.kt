package app.hocket.share

import android.content.ClipData
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.widget.Toast
import androidx.core.content.FileProvider
import app.hocket.R
import app.hocket.core.Queries
import app.hocket.core.api.ActionTarget
import app.hocket.core.api.Query
import app.hocket.core.api.QueryResult
import app.hocket.core.client.CoreClient
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.BasicAlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.components.MenuKind
import app.hocket.ui.components.MenuSubject
import app.hocket.ui.components.resolveTrackIds
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.delay
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.job
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File
import java.net.URLEncoder

/** Which share a menu row runs. */
enum class ShareChoice { Navidrome, SongLink, Genius, File }

/** What Share can offer for this kind of item. */
data class ShareChoices(val navidrome: Boolean, val songLink: Boolean, val file: Boolean) {
    val any: Boolean get() = navidrome || songLink || file
}

/**
 * One song has every option. Several songs, an album and a playlist can make a link
 * (an album also Squigly). An artist has none: createShare has no artist id and
 * Squigly gets no artist pages from us. Open in Genius is a row of the song menu, not one of these.
 */
fun shareChoices(kind: MenuKind, count: Int = 1): ShareChoices = when (kind) {
    MenuKind.Song -> ShareChoices(navidrome = true, songLink = count <= 1, file = count <= 1)
    MenuKind.Songs -> ShareChoices(navidrome = true, songLink = false, file = false)
    MenuKind.Album -> ShareChoices(navidrome = true, songLink = true, file = false)
    MenuKind.Artist -> ShareChoices(navidrome = false, songLink = false, file = false)
    MenuKind.Playlist -> ShareChoices(navidrome = true, songLink = false, file = false)
    MenuKind.Other -> ShareChoices(navidrome = false, songLink = false, file = false)
}

/** Genius search for `"title" "artist"`. Null when there is no title. */
fun geniusSearch(title: String, artist: String?): String? {
    val song = title.trim()
    if (song.isEmpty()) return null
    val who = artist?.trim().orEmpty()
    val q = if (who.isEmpty()) "\"$song\"" else "\"$song\" \"$who\""
    return "https://genius.com/search?q=" + URLEncoder.encode(q, Charsets.UTF_8.name()).replace("+", "%20")
}

/**
 * What the core's songLink query looks up: one song or one album. Squigly takes another
 * service's link, not a search, so the core finds the item on Deezer first.
 */
fun songLinkQuery(subject: MenuSubject, target: ActionTarget): Query? = when (subject.kind) {
    MenuKind.Song -> (subject.trackId ?: (target as? ActionTarget.Tracks)?.data?.ids?.singleOrNull())
        ?.takeIf { it.isNotEmpty() }?.let { Queries.songLink(trackId = it) }
    MenuKind.Album -> ((target as? ActionTarget.Albums)?.data?.ids?.singleOrNull() ?: subject.albumId)
        ?.takeIf { it.isNotEmpty() }?.let { Queries.songLink(albumId = it) }
    else -> null
}

/** One share the menu asked for, kept by the host so the work outlives the sheet. */
data class ShareRequest(val choice: ShareChoice, val subject: MenuSubject, val target: ActionTarget)

/** Opens the system share sheet for a Navidrome link, a Squigly link, or the audio file. */
object ShareActions {
    suspend fun perform(context: Context, client: CoreClient, request: ShareRequest) {
        when (request.choice) {
            ShareChoice.Navidrome -> {
                val ids = navidromeIds(client, request.target, request.subject.kind)
                val url = if (ids.isEmpty()) null else (client.query(Queries.createShare(ids)) as? QueryResult.Share)?.data?.url
                currentCoroutineContext().ensureActive()
                if (url.isNullOrBlank()) toast(context, R.string.toast_share_failed) else sendText(context, url)
            }
            ShareChoice.SongLink -> {
                val url = songLinkQuery(request.subject, request.target)
                    ?.let { (client.query(it) as? QueryResult.SongLink)?.data }
                currentCoroutineContext().ensureActive()
                if (url.isNullOrBlank()) toast(context, R.string.toast_share_failed) else sendText(context, url)
            }
            ShareChoice.Genius -> {
                val url = geniusSearch(request.subject.title, request.subject.artist)
                if (url == null) toast(context, R.string.toast_share_failed) else openUrl(context, url)
            }
            ShareChoice.File -> shareFile(context, client, request)
        }
    }

    private suspend fun navidromeIds(client: CoreClient, target: ActionTarget, kind: MenuKind): List<String> = when (kind) {
        MenuKind.Album -> (target as? ActionTarget.Albums)?.data?.ids.orEmpty()
        MenuKind.Playlist -> (target as? ActionTarget.Playlists)?.data?.ids.orEmpty()
        MenuKind.Song, MenuKind.Songs -> when (target) {
            is ActionTarget.Tracks -> target.data.ids
            else -> resolveTrackIds(client, target)
        }
        else -> emptyList()
    }

    private fun openUrl(context: Context, url: String) {
        val view = Intent(Intent.ACTION_VIEW, Uri.parse(url))
        if (context !is android.app.Activity) view.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        context.startActivity(view)
    }

    private fun sendText(context: Context, text: String) {
        val send = Intent(Intent.ACTION_SEND).apply {
            type = "text/plain"
            putExtra(Intent.EXTRA_TEXT, text)
        }
        val chooser = Intent.createChooser(send, context.getString(R.string.action_share))
        if (context !is android.app.Activity) chooser.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        context.startActivity(chooser)
    }

    private suspend fun shareFile(context: Context, client: CoreClient, request: ShareRequest) {
        val trackId = request.subject.trackId
            ?: (request.target as? ActionTarget.Tracks)?.data?.ids?.singleOrNull()
        val exported = trackId?.let { (client.query(Queries.exportAudio(it)) as? QueryResult.AudioFile)?.data }
        val src = exported?.path?.let(::File)?.takeIf { it.isFile }
        currentCoroutineContext().ensureActive()
        if (exported == null || src == null) {
            toast(context, R.string.toast_share_failed)
            return
        }
        val uri = withContext(Dispatchers.IO) {
            val root = File(context.cacheDir, "shared").apply { mkdirs() }
            val now = System.currentTimeMillis()
            // Same hour as a lyrics card: the other app has read the previous file by then.
            root.listFiles()?.filter { now - it.lastModified() > 60 * 60 * 1000L }?.forEach { it.deleteRecursively() }
            val dir = File(root, now.toString()).apply { mkdirs() }
            val dest = File(dir, exported.fileName.ifBlank { src.name })
            src.copyTo(dest, overwrite = true)
            FileProvider.getUriForFile(context, ShareImages.authority(context), dest)
        }
        val send = Intent(Intent.ACTION_SEND).apply {
            type = exported.contentType.ifBlank { "application/octet-stream" }
            putExtra(Intent.EXTRA_STREAM, uri)
            clipData = ClipData.newRawUri(exported.fileName, uri)
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        currentCoroutineContext().ensureActive()
        val chooser = Intent.createChooser(send, context.getString(R.string.action_share))
        if (context !is android.app.Activity) chooser.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        context.startActivity(chooser)
    }

    private fun toast(context: Context, text: Int) {
        Toast.makeText(context, text, Toast.LENGTH_SHORT).show()
    }
}

/**
 * Runs [request] on a scope that outlives the menu sheet, then [onDone]. A link or file the core has to
 * fetch can take a moment, so after a beat a small dialog says so; dismissing it gives up on the share.
 */
@Composable
fun ShareRequestEffect(request: ShareRequest?, onDone: () -> Unit) {
    val context = LocalContext.current
    val client = LocalCoreClient.current
    val finish by rememberUpdatedState(onDone)
    var busy by remember(request) { mutableStateOf(false) }
    val run = remember(request) { ShareRun() }
    LaunchedEffect(request) {
        val req = request ?: return@LaunchedEffect
        run.job = coroutineContext.job
        // Genius only opens a search page: nothing to wait for.
        val beat = if (req.choice == ShareChoice.Genius) null else launch { delay(BUSY_DELAY_MS); busy = true }
        try {
            ShareActions.perform(context, client, req)
        } finally {
            beat?.cancel()
            busy = false
        }
        run.finish(finish)
    }
    // Cancel stops the work at once, not a frame later when the request is cleared: a link landing in
    // between must not open the share sheet.
    if (busy && request != null) ShareBusyDialog(request.choice, onCancel = { run.job?.cancel(); run.finish(finish) })
}

/** One share's work, finished once whether it ends by itself or by Cancel. */
private class ShareRun {
    var job: Job? = null
    private var done = false

    fun finish(onDone: () -> Unit) {
        if (done) return
        done = true
        onDone()
    }
}

/** Long enough that a cached link never flashes the dialog. */
private const val BUSY_DELAY_MS = 300L

@OptIn(ExperimentalMaterial3Api::class, ExperimentalMaterial3ExpressiveApi::class)
@Composable
private fun ShareBusyDialog(choice: ShareChoice, onCancel: () -> Unit) {
    val label = stringResource(if (choice == ShareChoice.File) R.string.share_busy_file else R.string.share_busy_link)
    BasicAlertDialog(onDismissRequest = onCancel, modifier = Modifier.testTag("share.busy")) {
        Surface(shape = RoundedCornerShape(28.dp), color = MaterialTheme.colorScheme.surfaceContainerHigh) {
            Column(Modifier.padding(start = 24.dp, end = 24.dp, top = 24.dp, bottom = 12.dp)) {
                Row(
                    Modifier.fillMaxWidth().semantics(mergeDescendants = true) { liveRegion = LiveRegionMode.Polite },
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(16.dp),
                ) {
                    LoadingIndicator(Modifier.size(40.dp))
                    Text(label, style = MaterialTheme.typography.titleMedium)
                }
                TextButton(onClick = onCancel, modifier = Modifier.align(Alignment.End).padding(top = 8.dp)) {
                    Text(stringResource(R.string.action_cancel))
                }
            }
        }
    }
}
