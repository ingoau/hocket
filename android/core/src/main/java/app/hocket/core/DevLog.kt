package app.hocket.core

import android.util.Log
import java.time.Instant
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter

/**
 * The log capture behind developer diagnostics. With the native core, lines go into the core's own
 * buffer (`hocket_core::devlog`): one timeline with the Rust side's tracing, the last hour, mirrored
 * to a file in the core's data dir so a crash or restart doesn't lose it. Developer mode
 * (`developer.mode`) switches that buffer to debug; the core applies the setting itself.
 *
 * Without the library (the fake core, JVM tests) a small ring here stands in and keeps every line.
 * Kotlin's own `Log` calls are not routed through here: the diagnostics bundle reads them from
 * logcat. This is for what logcat would lose: uncaught exceptions ([installCrashHandler]) and
 * events worth having next to the core's lines.
 */
object DevLog {
    const val WINDOW_MS: Long = 60 * 60 * 1000L
    private const val MAX_LINE = 16 * 1024
    private const val FALLBACK_MAX_BYTES = 4 * 1024 * 1024

    private val TIMESTAMP: DateTimeFormatter = DateTimeFormatter.ofPattern("yyyy-MM-dd'T'HH:mm:ss.SSS'Z'").withZone(ZoneOffset.UTC)

    private val ring = ArrayDeque<Pair<Long, String>>()
    private var ringBytes = 0

    /** Whether lines go to the core's buffer (the library is loadable). */
    val usesCore: Boolean get() = NativeCore.isAvailable()

    /** [level] is `debug`, `info`, `warn` or `error`; [tag] names the source (`kotlin/PlaybackService`). */
    fun append(level: String, tag: String, message: String) {
        val text = if (message.length > MAX_LINE) message.take(MAX_LINE) + "… (${message.length} chars)" else message
        if (usesCore) {
            runCatching { app.hocket.core.ffi.appendLog(level, tag, text) }
            return
        }
        val now = System.currentTimeMillis()
        val line = formatLine(now, level, tag, text)
        synchronized(ring) {
            ring.addLast(now to line)
            ringBytes += line.length
            while (ring.isNotEmpty() && (ring.first().first < now - WINDOW_MS || ringBytes > FALLBACK_MAX_BYTES)) {
                ringBytes -= ring.removeFirst().second.length
            }
        }
    }

    /** The captured lines of the last [windowMs], oldest first. */
    fun recent(windowMs: Long = WINDOW_MS): String {
        if (usesCore) return runCatching { app.hocket.core.ffi.recentLogs(windowMs.toULong()) }.getOrDefault("")
        val since = System.currentTimeMillis() - windowMs
        return synchronized(ring) { ring.filter { it.first >= since }.joinToString("") { it.second } }
    }

    /** Whether the core's detailed logging is on (false without the library). */
    fun detailed(): Boolean = usesCore && runCatching { app.hocket.core.ffi.detailedLogging() }.getOrDefault(false)

    /** Blocks until everything captured is on disk, at most [timeoutMs]. */
    fun flush(timeoutMs: Long = 500) {
        if (usesCore) runCatching { app.hocket.core.ffi.flushLogs(timeoutMs.toULong()) }
    }

    /**
     * Records an uncaught exception (with its stack) and flushes before the previous handler takes
     * the process down, so the next diagnostics bundle has it. Call once, from `Application.onCreate`.
     */
    fun installCrashHandler() {
        val previous = Thread.getDefaultUncaughtExceptionHandler()
        Thread.setDefaultUncaughtExceptionHandler { thread, error ->
            runCatching {
                append("error", "crash", "uncaught on thread '${thread.name}': ${Log.getStackTraceString(error)}")
                flush()
            }
            previous?.uncaughtException(thread, error)
        }
    }

    /** `2026-10-20T03:04:05.678Z  INFO tag: message`, the core's line shape. */
    fun formatLine(atMs: Long, level: String, tag: String, message: String): String =
        "${TIMESTAMP.format(Instant.ofEpochMilli(atMs))} ${level.uppercase().padStart(5)} $tag: $message\n"
}
