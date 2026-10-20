package app.hocket.diagnostics

import android.app.ActivityManager
import android.content.ClipData
import android.content.Context
import android.content.Intent
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.Uri
import android.os.Build
import android.os.PowerManager
import android.os.Process
import android.os.StatFs
import android.os.SystemClock
import androidx.core.content.FileProvider
import app.hocket.BuildConfig
import app.hocket.core.DevLog
import app.hocket.core.NativeCore
import app.hocket.core.api.Query
import app.hocket.core.api.QueryResult
import app.hocket.core.client.CoreClient
import app.hocket.share.ShareImages
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.concurrent.TimeUnit

/**
 * What the developer diagnostics page saves and shares: [head] (the device and app, then the core's
 * own diagnostics), the core's log of the last hour ([coreLog]: Rust tracing plus what [DevLog]
 * recorded, crashes included) and this app's logcat for the same hour ([logcat]: Kotlin, Media3,
 * the system's lines about us).
 */
data class DiagnosticsReport(
    val generatedAt: Long,
    val head: String,
    val coreLog: String,
    val logcat: String,
    val windowMs: Long,
) {
    val fileName: String get() = "hocket-diagnostics-" + SimpleDateFormat("yyyyMMdd-HHmmss", Locale.US).format(Date(generatedAt)) + ".txt"
    val coreLines: Int get() = coreLog.lineSequence().count { it.isNotEmpty() }
    val logcatLines: Int get() = logcat.lineSequence().count { it.isNotEmpty() }
    val bytes: Long get() = (coreLog.length + logcat.length).toLong()
    private val minutes: Long get() = windowMs / 60_000

    val text: String
        get() = buildString {
            append(head.trimEnd()).append("\n\n")
            append("Core log (last $minutes minutes)\n-----------------------------\n")
            append(coreLog.trimEnd().ifEmpty { "(empty)" }).append("\n\n")
            append("Logcat (last $minutes minutes)\n---------------------------\n")
            append(logcat.trimEnd().ifEmpty { "(empty)" }).append('\n')
        }

    /** The newest [n] lines of the core log, for the page's preview. */
    fun coreTail(n: Int): String = coreLog.trimEnd().lines().takeLast(n).joinToString("\n")
}

object DiagnosticsBundle {
    private const val SHARE_DIR = "shared/diagnostics"
    /** Bundles in the share folder older than this are removed when a new one is written. */
    private const val KEEP_MS = 24 * 60 * 60 * 1000L

    /** Collects a fresh report. [logcat] is swappable for tests (Robolectric has no logcat). */
    suspend fun collect(
        context: Context,
        client: CoreClient,
        windowMs: Long = DevLog.WINDOW_MS,
        logcat: suspend (Long) -> String = { Logcat.recent(it) },
    ): DiagnosticsReport = withContext(Dispatchers.IO) {
        val now = System.currentTimeMillis()
        val core = withTimeoutOrNull(5_000) { (client.query(Query.Diagnostics) as? QueryResult.Text)?.data } ?: "(the core didn't answer)"
        val coreLog = DevLog.recent(windowMs)
        val cat = logcat(windowMs)
        val facts = facts(context, client, now)
        val width = facts.maxOf { it.first.length }
        val head = buildString {
            append("Hocket diagnostics\n==================\n")
            facts.forEach { (k, v) -> append(k.padEnd(width)).append("  ").append(v).append('\n') }
            append("\nCore\n----\n").append(core.trimEnd()).append('\n')
        }
        DiagnosticsReport(now, head, coreLog, cat, windowMs)
    }

    /** Writes the report to the share cache and opens the share sheet for it. */
    suspend fun share(context: Context, report: DiagnosticsReport, chooserTitle: String) {
        val uri = withContext(Dispatchers.IO) {
            val dir = File(context.cacheDir, SHARE_DIR).apply { mkdirs() }
            val now = System.currentTimeMillis()
            dir.listFiles()?.filter { now - it.lastModified() > KEEP_MS }?.forEach { it.deleteRecursively() }
            val file = File(dir, report.fileName)
            file.writeText(report.text)
            FileProvider.getUriForFile(context, ShareImages.authority(context), file)
        }
        val send = Intent(Intent.ACTION_SEND).apply {
            type = "text/plain"
            putExtra(Intent.EXTRA_STREAM, uri)
            putExtra(Intent.EXTRA_SUBJECT, report.fileName)
            clipData = ClipData.newRawUri(report.fileName, uri)
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        context.startActivity(Intent.createChooser(send, chooserTitle).apply { if (context !is android.app.Activity) addFlags(Intent.FLAG_ACTIVITY_NEW_TASK) })
    }

    /** Writes the report to a document the user picked. False when it could not be written. */
    suspend fun writeTo(context: Context, uri: Uri, report: DiagnosticsReport): Boolean = withContext(Dispatchers.IO) {
        runCatching { context.contentResolver.openOutputStream(uri)?.use { it.write(report.text.toByteArray()) } != null }.getOrDefault(false)
    }

    private fun facts(context: Context, client: CoreClient, now: Long): List<Pair<String, String>> {
        val out = mutableListOf<Pair<String, String>>()
        fun fact(key: String, value: () -> String) = out.add(key to runCatching(value).getOrElse { "(unavailable: ${it.message})" })
        fact("generated") { SimpleDateFormat("yyyy-MM-dd'T'HH:mm:ssXXX", Locale.US).format(Date(now)) }
        fact("app") { "${BuildConfig.VERSION_NAME} (${BuildConfig.VERSION_CODE}) ${BuildConfig.BUILD_TYPE} · ${context.packageName}" }
        fact("core") { if (NativeCore.isAvailable()) "native ${NativeCore.version()} (${client.kind})" else "fake (${client.kind})" }
        fact("device") { "${Build.MANUFACTURER} ${Build.MODEL} (${Build.DEVICE}, ${Build.HARDWARE})" }
        fact("android") { "${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT}, patch ${Build.VERSION.SECURITY_PATCH}) ${Build.FINGERPRINT}" }
        fact("abis") { Build.SUPPORTED_ABIS.joinToString() }
        fact("locale") { "${Locale.getDefault().toLanguageTag()} · ${java.util.TimeZone.getDefault().id}" }
        fact("display") { context.resources.displayMetrics.let { "${it.widthPixels}x${it.heightPixels} px, ${it.densityDpi} dpi, font scale ${context.resources.configuration.fontScale}" } }
        fact("memory") {
            val am = context.getSystemService(ActivityManager::class.java)
            val info = ActivityManager.MemoryInfo().also { am.getMemoryInfo(it) }
            val rt = Runtime.getRuntime()
            "${mib(info.availMem)} free of ${mib(info.totalMem)}${if (info.lowMemory) " (low)" else ""}; heap ${mib(rt.totalMemory() - rt.freeMemory())} of ${mib(rt.maxMemory())}; class ${am.memoryClass} MiB"
        }
        fact("storage") { StatFs(context.filesDir.path).let { "${mib(it.availableBytes)} free of ${mib(it.totalBytes)}" } }
        fact("power") {
            val pm = context.getSystemService(PowerManager::class.java)
            "saver ${pm.isPowerSaveMode}, ignoring battery optimisations ${pm.isIgnoringBatteryOptimizations(context.packageName)}, idle ${pm.isDeviceIdleMode}"
        }
        fact("network") {
            val cm = context.getSystemService(ConnectivityManager::class.java)
            val caps = cm.activeNetwork?.let { cm.getNetworkCapabilities(it) }
            if (caps == null) "none" else listOfNotNull(
                "wifi".takeIf { caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) },
                "cellular".takeIf { caps.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR) },
                "ethernet".takeIf { caps.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET) },
                "vpn".takeIf { caps.hasTransport(NetworkCapabilities.TRANSPORT_VPN) },
                "validated".takeIf { caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED) },
                "metered".takeIf { !caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED) },
            ).joinToString(" ")
        }
        fact("process") {
            val up = (SystemClock.elapsedRealtime() - Process.getStartElapsedRealtime()) / 1000
            "pid ${Process.myPid()}, up $up s, device up ${SystemClock.elapsedRealtime() / 1000} s"
        }
        fact("developer mode") { if (DevLog.detailed()) "on (detailed logging)" else "detailed logging off" }
        fact("log") { "${DevLog.WINDOW_MS / 60_000} min window; ${if (DevLog.usesCore) "core buffer" else "app buffer (no native core)"}" }
        return out
    }

    private fun mib(bytes: Long): String = String.format(Locale.US, "%.1f MiB", bytes / (1024.0 * 1024.0))
}

/** This app's own logcat (apps may read only their own uid's lines, which includes a crashed earlier process). */
object Logcat {
    private const val MAX_CHARS = 4 * 1024 * 1024

    fun recent(windowMs: Long): String = try {
        val since = SimpleDateFormat("yyyy-MM-dd HH:mm:ss.SSS", Locale.US).format(Date(System.currentTimeMillis() - windowMs))
        val process = ProcessBuilder("logcat", "-d", "-v", "threadtime,year", "-T", since).redirectErrorStream(true).start()
        // Read while it runs (a full pipe would stall it); keep the newest MAX_CHARS.
        val text = StringBuilder()
        process.inputStream.bufferedReader().useLines { lines ->
            for (line in lines) {
                text.append(line).append('\n')
                if (text.length > MAX_CHARS * 2) text.delete(0, text.length - MAX_CHARS)
            }
        }
        if (!process.waitFor(5, TimeUnit.SECONDS)) process.destroy()
        if (text.length > MAX_CHARS) text.delete(0, text.length - MAX_CHARS)
        text.toString()
    } catch (e: Exception) {
        "(logcat unavailable: ${e.message})\n"
    }
}
