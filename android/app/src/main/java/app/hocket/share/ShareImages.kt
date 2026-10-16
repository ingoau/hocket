package app.hocket.share

import android.content.ClipData
import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import androidx.core.content.FileProvider
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.File

/** Images the app hands to other apps (a lyrics card): never exported, a read grant per intent only. */
class ShareFileProvider : FileProvider()

/**
 * Writing a rendered image out: to the share sheet (a PNG in `cache/shared/`, through
 * [ShareFileProvider]), to Pictures/Hocket (MediaStore, Android 10+, no permission), or to a
 * document the user picked (older Android, where MediaStore writes need the storage permission).
 */
object ShareImages {
    private const val DIR = "shared"
    /** Older shared images are dropped when a new one is written (the target app has long read them). */
    private const val KEEP_MS = 60 * 60 * 1000L

    fun authority(context: Context) = "${context.packageName}.share"

    /** A file name for [title]: no path separators or characters storage providers refuse. */
    fun fileName(title: String): String = "Hocket - " + title.replace(Regex("[\\\\/:*?\"<>|\\p{Cntrl}]"), "_").trim().take(60).ifEmpty { "lyrics" } + ".png"

    /** Writes [bitmap] to the share cache and opens the share sheet for it. */
    suspend fun share(context: Context, bitmap: Bitmap, name: String, chooserTitle: String) {
        val uri = withContext(Dispatchers.IO) {
            val dir = File(context.cacheDir, DIR).apply { mkdirs() }
            val now = System.currentTimeMillis()
            dir.listFiles()?.filter { now - it.lastModified() > KEEP_MS }?.forEach { it.deleteRecursively() }
            // Its own folder per share, so the name the target app shows is the plain one.
            val file = File(File(dir, now.toString()).apply { mkdirs() }, name)
            file.outputStream().use { png(bitmap).compress(Bitmap.CompressFormat.PNG, 100, it) }
            FileProvider.getUriForFile(context, authority(context), file)
        }
        val send = Intent(Intent.ACTION_SEND).apply {
            type = "image/png"
            putExtra(Intent.EXTRA_STREAM, uri)
            // The ClipData carries the grant to the chooser's target (and its preview).
            clipData = ClipData.newRawUri(name, uri)
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        context.startActivity(Intent.createChooser(send, chooserTitle).apply { if (context !is android.app.Activity) addFlags(Intent.FLAG_ACTIVITY_NEW_TASK) })
    }

    /** Whether [saveToPictures] can be used (else [writeTo] a document the user picked). */
    val canSaveToPictures: Boolean get() = Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q

    /** Saves [bitmap] as a PNG in Pictures/Hocket (Android 10+). False when it could not be written. */
    suspend fun saveToPictures(context: Context, bitmap: Bitmap, name: String): Boolean = withContext(Dispatchers.IO) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) return@withContext false
        val resolver = context.contentResolver
        val values = ContentValues().apply {
            put(MediaStore.Images.Media.DISPLAY_NAME, name)
            put(MediaStore.Images.Media.MIME_TYPE, "image/png")
            put(MediaStore.Images.Media.RELATIVE_PATH, Environment.DIRECTORY_PICTURES + "/Hocket")
            put(MediaStore.Images.Media.IS_PENDING, 1)
        }
        val uri = runCatching { resolver.insert(MediaStore.Images.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY), values) }.getOrNull()
            ?: return@withContext false
        val written = runCatching {
            resolver.openOutputStream(uri)?.use { png(bitmap).compress(Bitmap.CompressFormat.PNG, 100, it) } ?: false
        }.getOrDefault(false)
        if (written) {
            resolver.update(uri, ContentValues().apply { put(MediaStore.Images.Media.IS_PENDING, 0) }, null, null)
        } else {
            runCatching { resolver.delete(uri, null, null) }
        }
        written
    }

    /** Writes [bitmap] as a PNG to [uri] (a document from `ACTION_CREATE_DOCUMENT`). */
    suspend fun writeTo(context: Context, uri: Uri, bitmap: Bitmap): Boolean = withContext(Dispatchers.IO) {
        runCatching { context.contentResolver.openOutputStream(uri)?.use { png(bitmap).compress(Bitmap.CompressFormat.PNG, 100, it) } ?: false }.getOrDefault(false)
    }

    /** A software copy of a hardware bitmap (what a captured graphics layer may be) for encoding. */
    private fun png(bitmap: Bitmap): Bitmap =
        if (bitmap.config == Bitmap.Config.HARDWARE) bitmap.copy(Bitmap.Config.ARGB_8888, false) else bitmap
}
