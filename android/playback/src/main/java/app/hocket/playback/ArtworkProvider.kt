package app.hocket.playback

import android.content.ContentProvider
import android.content.ContentValues
import android.content.Context
import android.database.Cursor
import android.net.Uri
import android.os.ParcelFileDescriptor
import app.hocket.core.ArtworkSizes
import app.hocket.core.Queries
import app.hocket.core.api.QueryResult
import java.io.File
import java.io.FileNotFoundException
import java.util.concurrent.ConcurrentHashMap
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeoutOrNull

/**
 * Serves cover art to the apps browsing the library ([LibraryBrowser]): Android Auto, Wear and
 * other `MediaBrowser`s get `content://<package>.artwork/<size>/<coverArt>` URIs, since they cannot
 * read the core's cache files. Opening one resolves it through `Query.Artwork` (fetching it into the
 * cache if needed) and hands back the cached file read-only.
 *
 * Exported, because a controller reads the URI with its own identity, but only callers the session
 * has accepted as controllers ([allow], from `onConnect`) and this app itself may open anything;
 * everyone else gets a `SecurityException`. Sizes are the core's fixed cache sizes only.
 */
class ArtworkProvider : ContentProvider() {
    companion object {
        private const val TIMEOUT_MS = 10_000L
        private val SIZES = setOf(ArtworkSizes.THUMB, ArtworkSizes.LIST, ArtworkSizes.GRID, ArtworkSizes.FULL)
        private val allowed: MutableSet<String> = ConcurrentHashMap.newKeySet()

        fun authority(context: Context): String = "${context.packageName}.artwork"

        fun uri(context: Context, coverArt: String, size: Int = ArtworkSizes.GRID): Uri =
            Uri.Builder().scheme("content").authority(authority(context)).appendPath(size.toString()).appendPath(coverArt).build()

        /** Lets [packageName] (a connected media controller) open artwork URIs until [revokeAll]. */
        fun allow(packageName: String) { allowed += packageName }

        internal fun isAllowed(packageName: String?, own: String): Boolean = packageName != null && (packageName == own || packageName in allowed)

        /** Nobody but this app may open artwork again until controllers reconnect. */
        fun revokeAll() = allowed.clear()
    }

    override fun onCreate(): Boolean = true

    override fun openFile(uri: Uri, mode: String): ParcelFileDescriptor {
        val ctx = context ?: throw FileNotFoundException(uri.toString())
        if (mode != "r") throw SecurityException("artwork is read-only")
        if (!isAllowed(callingPackage, ctx.packageName)) throw SecurityException("not a connected media controller")
        val segments = uri.pathSegments
        val size = segments.getOrNull(0)?.toIntOrNull()?.takeIf { it in SIZES } ?: throw FileNotFoundException(uri.toString())
        val coverArt = segments.getOrNull(1)?.takeIf { segments.size == 2 && it.isNotEmpty() } ?: throw FileNotFoundException(uri.toString())
        val core = CoreHost.current ?: throw FileNotFoundException("no core running")
        // A binder thread: blocking on the core's (possibly fetching) answer is fine, bounded.
        val path = runBlocking { withTimeoutOrNull(TIMEOUT_MS) { runCatching { (core.query(Queries.artwork(coverArt, size)) as? QueryResult.Path)?.data }.getOrNull() } }
            ?: throw FileNotFoundException(uri.toString())
        val file = File(path.removePrefix("file://"))
        return ParcelFileDescriptor.open(file, ParcelFileDescriptor.MODE_READ_ONLY)
    }

    override fun getType(uri: Uri): String = "image/*"
    override fun query(uri: Uri, projection: Array<out String>?, selection: String?, selectionArgs: Array<out String>?, sortOrder: String?): Cursor? = null
    override fun insert(uri: Uri, values: ContentValues?): Uri? = null
    override fun delete(uri: Uri, selection: String?, selectionArgs: Array<out String>?): Int = 0
    override fun update(uri: Uri, values: ContentValues?, selection: String?, selectionArgs: Array<out String>?): Int = 0
}
