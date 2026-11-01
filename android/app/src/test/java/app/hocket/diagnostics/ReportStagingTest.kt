package app.hocket.diagnostics

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Color
import android.net.Uri
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.api.ReportAttachmentKind
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.File

/** Report attachments as the core gets them: screenshots at most 2000 px on their longer side, as JPEG; recordings as they are. */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class ReportStagingTest {
    private val context: Context = ApplicationProvider.getApplicationContext()

    @Test
    fun theLongerSideIsScaledToTheLimitKeepingTheShape() {
        assertEquals(2000 to 1000, ReportStaging.scaledSize(4000, 2000))
        assertEquals(923 to 2000, ReportStaging.scaledSize(1080, 2340))
        assertEquals(1080 to 1920, ReportStaging.scaledSize(1080, 1920)) // already fits: never scaled up
        assertEquals(2000 to 1, ReportStaging.scaledSize(9000, 2)) // never below a pixel
    }

    @Test
    fun decodingSubsamplesNoFurtherThanTheLimit() {
        assertEquals(1, ReportStaging.sampleSize(1080, 2340))
        assertEquals(1, ReportStaging.sampleSize(3999, 3000))
        assertEquals(2, ReportStaging.sampleSize(4000, 3000))
        assertEquals(4, ReportStaging.sampleSize(8064, 6048))
    }

    @Test
    fun aBigScreenshotIsWrittenAsASmallerJpeg() {
        val source = File(context.cacheDir, "big.png")
        Bitmap.createBitmap(300, 120, Bitmap.Config.ARGB_8888).apply { eraseColor(Color.RED) }.let { b ->
            source.outputStream().use { b.compress(Bitmap.CompressFormat.PNG, 100, it) }
        }
        val out = File(context.cacheDir, "staged/screenshot-1.jpg")
        ReportStaging.stageScreenshot(context, Uri.fromFile(source), out, maxSide = 100)
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeFile(out.path, bounds)
        assertEquals(100, bounds.outWidth)
        assertEquals(40, bounds.outHeight)
        assertEquals("image/jpeg", bounds.outMimeType)
        // JPEG from the start: FF D8.
        assertArrayEquals(byteArrayOf(0xFF.toByte(), 0xD8.toByte()), out.readBytes().copyOf(2))
    }

    @Test
    fun aSmallBitmapIsLeftAsItIs() {
        val small = Bitmap.createBitmap(50, 20, Bitmap.Config.ARGB_8888)
        assertSame(small, ReportStaging.downscale(small, 100))
        val scaled = ReportStaging.downscale(Bitmap.createBitmap(400, 100, Bitmap.Config.ARGB_8888), 100)
        assertEquals(100, scaled.width)
        assertEquals(25, scaled.height)
    }

    @Test
    fun theRecordingIsCopiedByteForByteUnderASafeName() = runBlocking {
        val source = File(context.cacheDir, "screen-20261101-140211.mp4").apply { writeBytes(ByteArray(300_000) { (it % 251).toByte() }) }
        val staged = ReportStaging.stage(context, listOf(ReportPick(Uri.fromFile(source), ReportAttachmentKind.Recording)))
        assertEquals(1, staged.size)
        val file = File(staged[0].path)
        assertTrue(file.path.startsWith(ReportStaging.dir(context).path))
        assertEquals(ReportAttachmentKind.Recording, staged[0].kind)
        assertEquals(true, staged[0].deleteAfter)
        assertArrayEquals(source.readBytes(), file.readBytes())
        // Stale staging goes, newer files stay.
        ReportStaging.clearStale(context, before = file.parentFile!!.lastModified())
        assertTrue(file.exists())
        ReportStaging.clearStale(context, before = Long.MAX_VALUE)
        assertFalse(file.exists())
    }

    @Test
    fun recordingNamesAreSafeAndKeepAnExtension() {
        assertEquals("screen-20261101-140211.mp4", ReportStaging.recordingName("screen-20261101-140211.mp4", "mp4"))
        assertEquals("My_clip__1_.webm", ReportStaging.recordingName("My clip (1).webm", null))
        assertEquals("passwd.mp4", ReportStaging.recordingName("../../etc/passwd", "mp4"))
        assertEquals("recording.mkv", ReportStaging.recordingName(null, "mkv"))
        assertEquals("recording.mp4", ReportStaging.recordingName("...", null))
    }
}
