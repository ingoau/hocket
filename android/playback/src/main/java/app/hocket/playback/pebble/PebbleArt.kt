package app.hocket.playback.pebble

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Rect

/** The current cover, from the core's image cache, cut to what the watch asked for. */
internal object PebbleArt {
    /** [path] (a local path or `file://` URI) centre-cropped and scaled to [width] × [height]; null when unreadable. */
    fun load(path: String, width: Int, height: Int): PebbleProtocol.Art? {
        if (width <= 0 || height <= 0 || width > 400 || height > 400) return null
        val file = path.removePrefix("file://")
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeFile(file, bounds)
        if (bounds.outWidth <= 0 || bounds.outHeight <= 0) return null
        var sample = 1
        while (bounds.outWidth / (sample * 2) >= width && bounds.outHeight / (sample * 2) >= height) sample *= 2
        val source = BitmapFactory.decodeFile(file, BitmapFactory.Options().apply { inSampleSize = sample }) ?: return null
        // The largest centred region with the target's aspect ratio.
        val scale = minOf(source.width.toFloat() / width, source.height.toFloat() / height)
        val cw = (width * scale).toInt().coerceIn(1, source.width)
        val ch = (height * scale).toInt().coerceIn(1, source.height)
        val crop = Rect((source.width - cw) / 2, (source.height - ch) / 2, (source.width + cw) / 2, (source.height + ch) / 2)
        val out = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
        android.graphics.Canvas(out).drawBitmap(source, crop, Rect(0, 0, width, height), android.graphics.Paint(android.graphics.Paint.FILTER_BITMAP_FLAG))
        source.recycle()
        val pixels = IntArray(width * height)
        out.getPixels(pixels, 0, width, 0, 0, width, height)
        out.recycle()
        return PebbleProtocol.encodeArt(pixels, width, height)
    }
}
