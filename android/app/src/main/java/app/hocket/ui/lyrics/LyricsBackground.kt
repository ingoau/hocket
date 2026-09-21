package app.hocket.ui.lyrics

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.BitmapShader
import android.graphics.RuntimeShader
import android.graphics.Shader
import android.os.Build
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Paint
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.drawscope.drawIntoCanvas
import androidx.compose.ui.graphics.nativeCanvas
import androidx.compose.ui.graphics.toArgb
import app.hocket.core.ArtworkSizes
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.theme.ArtworkColors
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * The Kawarp equivalent: a small artwork texture (Kawase-blurred once per track) fed to an AGSL
 * shader that warps it slowly. Falls back to the static blurred texture below Android 13 and
 * whenever battery saver or "animated background off" applies. Stops animating when not visible.
 */
@Composable
fun LyricsBackground(coverArt: String?, animated: Boolean, visible: Boolean, modifier: Modifier = Modifier) {
    val client = LocalCoreClient.current
    val texture by produceState<Bitmap?>(initialValue = null, coverArt) {
        value = withContext(Dispatchers.Default) {
            val path = coverArt?.let { client.artworkPath(it, ArtworkSizes.LIST) }
            val src = path?.let { BitmapFactory.decodeFile(it.removePrefix("file://"), BitmapFactory.Options().apply { inSampleSize = 2 }) }
                ?: placeholderTexture(coverArt)
            kawaseBlur(Bitmap.createScaledBitmap(src, 48, 48, true), passes = 4)
        }
    }
    val tex = texture ?: return
    var time by remember { mutableFloatStateOf(0f) }
    val shouldAnimate = animated && visible && Build.VERSION.SDK_INT >= 33
    LaunchedEffect(shouldAnimate) {
        if (!shouldAnimate) return@LaunchedEffect
        val start = System.nanoTime()
        while (true) withFrameNanos { now -> time = (now - start) / 1_000_000_000f }
    }
    if (Build.VERSION.SDK_INT >= 33) {
        val shader = remember(tex) {
            RuntimeShader(WARP_SHADER).also {
                it.setInputShader("image", BitmapShader(tex, Shader.TileMode.MIRROR, Shader.TileMode.MIRROR))
                it.setFloatUniform("texSize", tex.width.toFloat(), tex.height.toFloat())
            }
        }
        Canvas(modifier.fillMaxSize()) {
            shader.setFloatUniform("size", size.width, size.height)
            shader.setFloatUniform("time", if (shouldAnimate) time else 0f)
            drawIntoCanvas { c ->
                val p = android.graphics.Paint().apply { setShader(shader) }
                c.nativeCanvas.drawRect(0f, 0f, size.width, size.height, p)
            }
            drawRect(Color.Black.copy(alpha = 0.35f))
        }
    } else {
        val image = remember(tex) { tex.asImageBitmap() }
        Canvas(modifier.fillMaxSize()) {
            drawIntoCanvas { c ->
                val p = android.graphics.Paint().apply { setShader(BitmapShader(tex, Shader.TileMode.MIRROR, Shader.TileMode.MIRROR).also { s ->
                    val m = android.graphics.Matrix(); m.setScale(size.width / tex.width, size.height / tex.height); s.setLocalMatrix(m)
                }); isFilterBitmap = true }
                c.nativeCanvas.drawRect(0f, 0f, size.width, size.height, p)
            }
            drawRect(Color.Black.copy(alpha = 0.35f))
        }
    }
}

private const val WARP_SHADER = """
uniform shader image;
uniform float2 size;
uniform float2 texSize;
uniform float time;
half4 main(float2 p) {
    float2 uv = p / size;
    float t = time * 0.12;
    float2 w = float2(sin(uv.y * 3.1 + t) * 0.09 + sin(uv.x * 2.3 - t * 0.7) * 0.05,
                      cos(uv.x * 2.7 - t) * 0.09 + sin(uv.y * 2.1 + t * 0.6) * 0.05);
    float2 q = 0.5 + (uv + w - 0.5) * 0.82;
    return image.eval(q * texSize);
}
"""

/** A few passes of a 5-tap Kawase-style blur on a tiny bitmap; cheap and smooth enough at 48 px. */
fun kawaseBlur(src: Bitmap, passes: Int): Bitmap {
    var current = src.copy(Bitmap.Config.ARGB_8888, true)
    val w = current.width; val h = current.height
    val a = IntArray(w * h); val b = IntArray(w * h)
    current.getPixels(a, 0, w, 0, 0, w, h)
    for (pass in 0 until passes) {
        val r = pass + 1
        for (y in 0 until h) for (x in 0 until w) {
            var rr = 0; var gg = 0; var bb = 0
            val taps = intArrayOf(x, y, x - r, y - r, x + r, y - r, x - r, y + r, x + r, y + r)
            for (i in 0 until 5) {
                val tx = taps[i * 2].coerceIn(0, w - 1); val ty = taps[i * 2 + 1].coerceIn(0, h - 1)
                val c = a[ty * w + tx]
                rr += (c shr 16) and 0xff; gg += (c shr 8) and 0xff; bb += c and 0xff
            }
            b[y * w + x] = (0xff shl 24) or ((rr / 5) shl 16) or ((gg / 5) shl 8) or (bb / 5)
        }
        System.arraycopy(b, 0, a, 0, a.size)
    }
    current.setPixels(a, 0, w, 0, 0, w, h)
    return current
}

private fun placeholderTexture(id: String?): Bitmap {
    val bmp = Bitmap.createBitmap(48, 48, Bitmap.Config.ARGB_8888)
    val c = android.graphics.Canvas(bmp)
    val p = android.graphics.Paint()
    val c1 = ArtworkColors.seedFor(id).toArgb(); val c2 = ArtworkColors.seedFor((id ?: "") + "~").toArgb()
    p.shader = android.graphics.LinearGradient(0f, 0f, 48f, 48f, c1, c2, Shader.TileMode.CLAMP)
    c.drawRect(0f, 0f, 48f, 48f, p)
    return bmp
}
