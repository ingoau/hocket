package app.hocket.playback

import android.content.ContentProvider
import android.content.ContentValues
import android.content.Context
import android.database.Cursor
import android.net.Uri
import android.os.CancellationSignal
import android.os.ParcelFileDescriptor
import app.hocket.core.ArtworkSizes
import app.hocket.core.Queries
import app.hocket.core.api.QueryResult
import java.io.File
import java.io.FileNotFoundException
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.Semaphore
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Deferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.async
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeoutOrNull

/**
 * Serves cover art to the apps browsing the library ([LibraryBrowser]): Android Auto, Wear and
 * other `MediaBrowser`s get `content://<package>.artwork/<size>/<coverArt>` URIs, since they cannot
 * read the core's cache files. Opening one resolves it through `Query.Artwork` (fetching it into the
 * cache if needed) and hands back the cached file read-only. The now-playing cover is a
 * `content://<package>.artwork/file/<key>` URI instead ([fileUri]): the very file the core resolved
 * for the media session, at the size it chose, with no lookup.
 *
 * Exported, because a controller reads the URI with its own identity, but only callers the session
 * has accepted as controllers ([allow], from `onConnect`) and this app itself may open anything;
 * everyone else gets a `SecurityException`. Sizes are the core's fixed cache sizes only.
 *
 * Each open holds one of the process's few binder threads, which Auto's play/pause and browse
 * calls need too, so waits are bounded ([ArtworkWaits]): a cached cover (or the core's at-once
 * "nothing" while offline) answers within the quick wait; a fetch is shared by everyone asking for
 * the same cover and size, only a few opens may wait on fetches at a time (the rest get
 * `FileNotFoundException` at once and the controller shows its placeholder), and a controller that
 * cancels (scrolled away) is let go.
 */
class ArtworkProvider : ContentProvider() {
    companion object {
        private val SIZES = setOf(ArtworkSizes.THUMB, ArtworkSizes.LIST, ArtworkSizes.GRID, ArtworkSizes.FULL)
        private val allowed: MutableSet<String> = ConcurrentHashMap.newKeySet()

        private const val FILE = "file"
        /** Files [fileUri] handed out, by key; cleared when it grows past [FILES_KEPT]. */
        private val files = ConcurrentHashMap<String, String>()
        private const val FILES_KEPT = 16

        fun authority(context: Context): String = "${context.packageName}.artwork"

        /**
         * A URI other apps can open for the local cover file at [path] (`file://` or a bare path),
         * such as the media session's now-playing artwork: a file URI is unreadable to them.
         */
        fun fileUri(context: Context, path: String): Uri {
            val file = path.removePrefix("file://")
            val key = java.security.MessageDigest.getInstance("SHA-256").digest(file.toByteArray()).take(12).joinToString("") { "%02x".format(it) }
            if (files.size >= FILES_KEPT && !files.containsKey(key)) files.clear()
            files[key] = file
            return Uri.Builder().scheme("content").authority(authority(context)).appendPath(FILE).appendPath(key).build()
        }

        /** The file behind a [fileUri] key; null when unknown (or forgotten since). */
        internal fun fileFor(key: String): String? = files[key]

        fun uri(context: Context, coverArt: String, size: Int = ArtworkSizes.GRID): Uri =
            Uri.Builder().scheme("content").authority(authority(context)).appendPath(size.toString()).appendPath(coverArt).build()

        /** Lets [packageName] (a connected media controller) open artwork URIs until [revokeAll]. */
        fun allow(packageName: String) { allowed += packageName }

        internal fun isAllowed(packageName: String?, own: String): Boolean = packageName != null && (packageName == own || packageName in allowed)

        /** Nobody but this app may open artwork again until controllers reconnect. */
        fun revokeAll() = allowed.clear()

        private val waits = ArtworkWaits(CoroutineScope(SupervisorJob() + Dispatchers.IO))
    }

    override fun onCreate(): Boolean = true

    override fun openFile(uri: Uri, mode: String): ParcelFileDescriptor = openFile(uri, mode, null)

    override fun openFile(uri: Uri, mode: String, signal: CancellationSignal?): ParcelFileDescriptor {
        val ctx = context ?: throw FileNotFoundException(uri.toString())
        if (mode != "r") throw SecurityException("artwork is read-only")
        if (!isAllowed(callingPackage, ctx.packageName)) throw SecurityException("not a connected media controller")
        val segments = uri.pathSegments
        if (segments.size == 2 && segments[0] == FILE) {
            val path = fileFor(segments[1]) ?: throw FileNotFoundException(uri.toString())
            return ParcelFileDescriptor.open(File(path), ParcelFileDescriptor.MODE_READ_ONLY)
        }
        val size = segments.getOrNull(0)?.toIntOrNull()?.takeIf { it in SIZES } ?: throw FileNotFoundException(uri.toString())
        val coverArt = segments.getOrNull(1)?.takeIf { segments.size == 2 && it.isNotEmpty() } ?: throw FileNotFoundException(uri.toString())
        val core = CoreHost.current ?: throw FileNotFoundException("no core running")
        // A binder thread: blocking on the core's (possibly fetching) answer, bounded.
        val path = waits.await("$size/$coverArt", signal) { (core.query(Queries.artwork(coverArt, size)) as? QueryResult.Path)?.data }
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

/**
 * Bounded, shared waits for [ArtworkProvider]'s lookups, keyed by size and cover. A lookup runs
 * once in [scope] however many opens want it, and outlives the opens that gave up on it, so its
 * fetch still lands in the cache for the next ask.
 *
 * [await] waits [quickMs] for the answer (a cache hit, or the core's immediate "nothing" while
 * offline) with no limit; past that it needs one of [maxWaiting] slots to keep waiting, up to
 * [timeoutMs] in all, and without a slot answers null at once. At most [maxLookups] lookups run at
 * once (every open starts one for its cover, kept or not): the rest wait their turn. A cancelled [CancellationSignal]
 * ends the wait with null.
 */
internal class ArtworkWaits(
    private val scope: CoroutineScope,
    private val quickMs: Long = QUICK_MS,
    private val timeoutMs: Long = TIMEOUT_MS,
    maxWaiting: Int = MAX_WAITING,
    maxLookups: Int = MAX_LOOKUPS,
) {
    companion object {
        const val QUICK_MS = 300L
        const val TIMEOUT_MS = 5_000L
        /** Of the process's ~15 binder threads. */
        const val MAX_WAITING = 4
        /** Core lookups (fetches) running at once, whoever waits for them. */
        const val MAX_LOOKUPS = 6
    }

    private val slots = Semaphore(maxWaiting)
    private val lookups = kotlinx.coroutines.sync.Semaphore(maxLookups)
    private val inFlight = ConcurrentHashMap<String, Deferred<String?>>()

    fun await(key: String, signal: CancellationSignal?, lookup: suspend () -> String?): String? {
        if (signal?.isCanceled == true) return null
        val lookupJob = inFlight.computeIfAbsent(key) {
            scope.async(start = CoroutineStart.LAZY) {
                // Only [maxLookups] run at once; the rest queue for a turn, but not past [timeoutMs]
                // (by then the opens that wanted them have long given up).
                if (withTimeoutOrNull(timeoutMs) { lookups.acquire() } == null) return@async null
                try { runCatching { lookup() }.getOrNull() } finally { lookups.release() }
            }
                .also { d -> d.invokeOnCompletion { inFlight.remove(key, d) } }
        }
        lookupJob.start()
        return try {
            runBlocking {
                val waiter = coroutineContext[Job]
                signal?.setOnCancelListener { waiter?.cancel() }
                try {
                    withTimeoutOrNull(quickMs) { lookupJob.await() }?.let { return@runBlocking it }
                    if (lookupJob.isCompleted) return@runBlocking lookupJob.await()
                    if (!slots.tryAcquire()) return@runBlocking null
                    try {
                        withTimeoutOrNull((timeoutMs - quickMs).coerceAtLeast(0)) { lookupJob.await() }
                    } finally {
                        slots.release()
                    }
                } finally {
                    signal?.setOnCancelListener(null)
                }
            }
        } catch (e: CancellationException) {
            // The asker cancelled (or the lookup was): nothing to hand over.
            null
        }
    }
}
