package app.hocket.playback

import android.content.Context
import android.os.Build
import android.util.Log
import app.hocket.core.CoreHandle
import app.hocket.core.CoreKind
import app.hocket.core.NativeCore
import app.hocket.core.api.AudioMode
import app.hocket.core.api.Command
import app.hocket.core.api.CoreConfig
import app.hocket.core.api.Platform
import app.hocket.core.fake.FakeCore
import java.io.File
import java.util.UUID

/**
 * The single core instance for the process.
 *
 * [PlaybackService] owns its lifecycle (it is created on the service's first `onCreate` and shut down
 * when the service is destroyed), but because the activity, the service and the media session all
 * run in one process, the handle itself is a process-wide singleton that the bound
 * [PlaybackServiceConnection] hands to the UI. The real [NativeCore] is used when the native library
 * loads; otherwise a [FakeCore] stands in and [kind] tells the UI to show its debug banner.
 */
object CoreHost {
    private const val TAG = "CoreHost"
    private const val PREFS = "hocket-core"
    private const val KEY_DEVICE_ID = "deviceId"

    @Volatile
    private var handle: CoreHandle? = null

    /** Forces the fake core even when the native library is present (UI tests, previews). */
    @Volatile
    var forceFake: Boolean = false

    val current: CoreHandle? get() = handle
    val kind: CoreKind? get() = handle?.kind

    /** Returns the running core, creating and starting it if needed. Safe to call from any thread. */
    @Synchronized
    fun acquire(context: Context): CoreHandle {
        handle?.let { return it }
        val app = context.applicationContext
        val core: CoreHandle = if (!forceFake && NativeCore.isAvailable()) {
            try {
                NativeCore.create(config(app))
            } catch (e: Exception) {
                Log.e(TAG, "Native core failed to start; using the fake core", e)
                FakeCore(startWithServer = false, startPlaying = false)
            }
        } else {
            Log.w(TAG, "libhocket_android not built for this ABI; using the fake core")
            FakeCore(startWithServer = false, startPlaying = false)
        }
        core.dispatch(Command.Start)
        handle = core
        return core
    }

    /** Replaces the running core (tests). Shuts down the previous one. */
    @Synchronized
    fun install(core: CoreHandle) {
        handle?.close()
        handle = core
    }

    @Synchronized
    fun shutdown() {
        handle?.close()
        handle = null
    }

    fun config(context: Context): CoreConfig {
        val version = try {
            context.packageManager.getPackageInfo(context.packageName, 0).versionName ?: "0.0.0"
        } catch (e: Exception) {
            "0.0.0"
        }
        return CoreConfig(
            dataDir = File(context.filesDir, "hocket").apply { mkdirs() }.absolutePath,
            cacheDir = File(context.cacheDir, "stream").apply { mkdirs() }.absolutePath,
            deviceId = deviceId(context),
            deviceName = Build.MODEL ?: "Android",
            platform = Platform.Android,
            appVersion = version,
            audio = AudioMode.External,
            coordinatorListen = null,
        )
    }

    /** A stable per-install id, generated once and persisted. */
    fun deviceId(context: Context): String {
        val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        prefs.getString(KEY_DEVICE_ID, null)?.let { return it }
        val id = UUID.randomUUID().toString().replace("-", "")
        prefs.edit().putString(KEY_DEVICE_ID, id).apply()
        return id
    }
}
