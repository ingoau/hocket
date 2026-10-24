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
import app.hocket.core.api.QueryResult
import app.hocket.core.client.CoreClient
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.platform.LocalContext
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.components.MenuKind
import app.hocket.ui.components.MenuSubject
import app.hocket.ui.components.resolveTrackIds
import kotlinx.coroutines.Dispatchers
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
 * (an album also Song.link). An artist is Song.link only: createShare has no artist id.
 * Open in Genius is a row of the song menu, not one of these.
 */
fun shareChoices(kind: MenuKind, count: Int = 1): ShareChoices = when (kind) {
    MenuKind.Song -> ShareChoices(navidrome = true, songLink = count <= 1, file = count <= 1)
    MenuKind.Songs -> ShareChoices(navidrome = true, songLink = false, file = false)
    MenuKind.Album -> ShareChoices(navidrome = true, songLink = true, file = false)
    MenuKind.Artist -> ShareChoices(navidrome = false, songLink = true, file = false)
    MenuKind.Playlist -> ShareChoices(navidrome = true, songLink = false, file = false)
    MenuKind.Other -> ShareChoices(navidrome = false, songLink = false, file = false)
}

/** Song.link's search page. `URLEncoder` writes spaces as `+`; Song.link wants `%20`. */
fun songLinkSearch(query: String): String =
    "https://song.link/s/" + URLEncoder.encode(query.trim(), Charsets.UTF_8.name()).replace("+", "%20")

/** Genius search for `"title" "artist"`. Null when there is no title. */
fun geniusSearch(title: String, artist: String?): String? {
    val song = title.trim()
    if (song.isEmpty()) return null
    val who = artist?.trim().orEmpty()
    val q = if (who.isEmpty()) "\"$song\"" else "\"$song\" \"$who\""
    return "https://genius.com/search?q=" + URLEncoder.encode(q, Charsets.UTF_8.name()).replace("+", "%20")
}

/** "Artist Title" / "Artist Album" / the artist's name. */
fun songLinkQuery(subject: MenuSubject): String? = when (subject.kind) {
    MenuKind.Song -> listOfNotNull(subject.artist, subject.title).joinToString(" ").ifBlank { null }
    MenuKind.Album -> listOfNotNull(subject.artist, subject.album ?: subject.title).joinToString(" ").ifBlank { null }
    MenuKind.Artist -> (subject.artist ?: subject.title).ifBlank { null }
    else -> null
}

/** One share the menu asked for, kept by the host so the work outlives the sheet. */
data class ShareRequest(val choice: ShareChoice, val subject: MenuSubject, val target: ActionTarget)

/** Opens the system share sheet for a Navidrome link, a Song.link, or the audio file. */
object ShareActions {
    suspend fun perform(context: Context, client: CoreClient, request: ShareRequest) {
        when (request.choice) {
            ShareChoice.Navidrome -> {
                val ids = navidromeIds(client, request.target, request.subject.kind)
                val url = if (ids.isEmpty()) null else (client.query(Queries.createShare(ids)) as? QueryResult.Share)?.data?.url
                if (url.isNullOrBlank()) toast(context, R.string.toast_share_failed) else sendText(context, url)
            }
            ShareChoice.SongLink -> {
                val query = songLinkQuery(request.subject)
                if (query == null) toast(context, R.string.toast_share_failed) else sendText(context, songLinkSearch(query))
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
        val chooser = Intent.createChooser(send, context.getString(R.string.action_share))
        if (context !is android.app.Activity) chooser.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        context.startActivity(chooser)
    }

    private fun toast(context: Context, text: Int) {
        Toast.makeText(context, text, Toast.LENGTH_SHORT).show()
    }
}

/** Runs [request] on a scope that outlives the menu sheet, then [onDone]. */
@Composable
fun ShareRequestEffect(request: ShareRequest?, onDone: () -> Unit) {
    val context = LocalContext.current
    val client = LocalCoreClient.current
    LaunchedEffect(request) {
        val req = request ?: return@LaunchedEffect
        ShareActions.perform(context, client, req)
        onDone()
    }
}
