package app.hocket.diagnostics

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.ImageDecoder
import android.net.Uri
import android.os.Build
import android.provider.OpenableColumns
import android.webkit.MimeTypeMap
import app.hocket.core.api.ReportAttachment
import app.hocket.core.api.ReportAttachmentKind
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.File
import java.io.IOException

/** Something the user picked for a report: a screenshot or the screen recording, as the picker handed it over. */
data class ReportPick(val uri: Uri, val kind: ReportAttachmentKind)

/**
 * Prepares a "Save to Coordinator" report's attachments for the core, which uploads files by path
 * (`docs/reports.md`): screenshots are decoded (subsampled), scaled so the longest side is at most
 * [MAX_SIDE] px and re-encoded as JPEG, which drops their metadata; the recording's original bytes
 * are copied as they are. Everything goes to `cacheDir/report-staging/` with `deleteAfter`, so the
 * core removes it once the upload is done or dismissed; [clearStale] catches what is left behind.
 */
object ReportStaging {
    const val DIR = "report-staging"
    const val MAX_SIDE = 2000
    const val JPEG_QUALITY = 85
    const val MAX_SCREENSHOTS = 5

    fun dir(context: Context): File = File(context.cacheDir, DIR)

    /** Stages [picks] (off the main thread) in a folder of their own; throws when one cannot be read or written. */
    suspend fun stage(context: Context, picks: List<ReportPick>): List<ReportAttachment> = withContext(Dispatchers.IO) {
        if (picks.isEmpty()) return@withContext emptyList()
        val folder = File(dir(context), System.currentTimeMillis().toString()).apply { mkdirs() }
        var screenshots = 0
        val out = ArrayList<ReportAttachment>(picks.size)
        try {
            for (pick in picks) {
                val file = when (pick.kind) {
                    ReportAttachmentKind.Screenshot -> stageScreenshot(context, pick.uri, File(folder, "screenshot-${++screenshots}.jpg"))
                    ReportAttachmentKind.Recording -> stageRecording(context, pick.uri, folder)
                }
                out += ReportAttachment(file.absolutePath, pick.kind, deleteAfter = true)
            }
        } catch (e: Throwable) {
            folder.deleteRecursively()
            throw e
        }
        out
    }

    /** Decodes [uri] no larger than it needs to be, scales it to [MAX_SIDE] and writes it to [file] as JPEG. */
    fun stageScreenshot(context: Context, uri: Uri, file: File, maxSide: Int = MAX_SIDE): File {
        val bitmap = decode(context, uri, maxSide)
        try {
            val scaled = downscale(bitmap, maxSide)
            try { writeJpeg(scaled, file) } finally { if (scaled !== bitmap) scaled.recycle() }
        } finally {
            bitmap.recycle()
        }
        return file
    }

    private fun decode(context: Context, uri: Uri, maxSide: Int): Bitmap {
        val resolver = context.contentResolver
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            // ImageDecoder applies the EXIF orientation, which the JPEG we write no longer carries.
            runCatching {
                return ImageDecoder.decodeBitmap(ImageDecoder.createSource(resolver, uri)) { decoder, info, _ ->
                    val (w, h) = scaledSize(info.size.width, info.size.height, maxSide)
                    decoder.setTargetSize(w, h)
                    decoder.allocator = ImageDecoder.ALLOCATOR_SOFTWARE
                }
            }
        }
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        // A bounds-only decode returns no bitmap: only a stream that won't open is an error here.
        (resolver.openInputStream(uri) ?: throw IOException("can't open the screenshot")).use { BitmapFactory.decodeStream(it, null, bounds) }
        if (bounds.outWidth <= 0 || bounds.outHeight <= 0) throw IOException("not an image")
        val options = BitmapFactory.Options().apply { inSampleSize = sampleSize(bounds.outWidth, bounds.outHeight, maxSide) }
        return resolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it, null, options) } ?: throw IOException("not an image")
    }

    /** [width]×[height] scaled (never up) so the longer side is at most [maxSide], keeping the aspect ratio. */
    fun scaledSize(width: Int, height: Int, maxSide: Int = MAX_SIDE): Pair<Int, Int> {
        val longest = maxOf(width, height)
        if (longest <= maxSide) return width to height
        val scale = maxSide.toDouble() / longest
        return maxOf(1, Math.round(width * scale).toInt()) to maxOf(1, Math.round(height * scale).toInt())
    }

    /** The largest power-of-two subsample that still decodes the longer side at [maxSide] or more (scaled down exactly after). */
    fun sampleSize(width: Int, height: Int, maxSide: Int = MAX_SIDE): Int {
        val longest = maxOf(width, height)
        var sample = 1
        while (longest / (sample * 2) >= maxSide) sample *= 2
        return sample
    }

    /** [bitmap] scaled to [scaledSize] (the same bitmap when it already fits). */
    fun downscale(bitmap: Bitmap, maxSide: Int = MAX_SIDE): Bitmap {
        val (w, h) = scaledSize(bitmap.width, bitmap.height, maxSide)
        return if (w == bitmap.width && h == bitmap.height) bitmap else Bitmap.createScaledBitmap(bitmap, w, h, true)
    }

    fun writeJpeg(bitmap: Bitmap, file: File) {
        file.parentFile?.mkdirs()
        val ok = file.outputStream().use { bitmap.compress(Bitmap.CompressFormat.JPEG, JPEG_QUALITY, it) }
        if (!ok) throw IOException("couldn't encode ${file.name}")
    }

    /** Copies the recording's original bytes into [folder], under its own (sanitised) name. */
    fun stageRecording(context: Context, uri: Uri, folder: File): File {
        val resolver = context.contentResolver
        val display = runCatching {
            resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
                if (c.moveToFirst()) c.getString(0) else null
            }
        }.getOrNull()
        val extension = resolver.getType(uri)?.let { MimeTypeMap.getSingleton().getExtensionFromMimeType(it) }
        val file = File(folder, recordingName(display, extension))
        val input = resolver.openInputStream(uri) ?: throw IOException("can't open the recording")
        input.use { src -> file.outputStream().use { src.copyTo(it, 256 * 1024) } }
        if (file.length() == 0L) throw IOException("the recording is empty")
        return file
    }

    /** A file name for the recording: the picker's display name made safe, with an extension (from the MIME type, else `.mp4`). */
    fun recordingName(display: String?, mimeExtension: String?): String {
        val base = display?.substringAfterLast('/')?.replace(Regex("[^A-Za-z0-9._-]"), "_")?.trim('.', '_')?.take(120)
            ?.takeIf { it.isNotEmpty() } ?: "recording"
        return if (base.contains('.')) base else "$base.${mimeExtension?.takeIf { it.isNotBlank() } ?: "mp4"}"
    }

    /** Deletes staged files left behind (no upload is pending), except those made after [before]. */
    fun clearStale(context: Context, before: Long) {
        dir(context).listFiles()?.forEach { if (it.lastModified() < before) it.deleteRecursively() }
    }
}
