package app.hocket.playback

import android.content.Context
import android.os.Build
import android.util.Log
import app.hocket.core.CoreHandle
import app.hocket.core.CoreKind
import app.hocket.core.NativeCore
import app.hocket.core.api.AudioMode
import app.hocket.core.Commands
import app.hocket.core.api.Command
import app.hocket.core.api.CoreConfig
import app.hocket.core.api.Event
import app.hocket.core.api.Platform
import app.hocket.core.fake.FakeCore
import java.io.File
import java.util.UUID
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

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

    /** Where passwords live; installed before [acquire] (tests may set an in-memory one). */
    @Volatile
    var credentials: ServerCredentialStore? = null

    /**
     * True once credentials have been replayed for this core start (or there were none to replay).
     * The UI waits for this before deciding between the setup screen and the shell.
     */
    private val _credentialsReplayed = MutableStateFlow(false)
    val credentialsReplayed: StateFlow<Boolean> = _credentialsReplayed.asStateFlow()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private var replayJob: Job? = null

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
        val store = credentials ?: KeystoreCredentialStore(app).also { credentials = it }
        _credentialsReplayed.value = false
        replayJob?.cancel()
        replayJob = scope.launch { replayCredentials(core, store) }
        core.dispatch(Command.Start)
        handle = core
        return core
    }

    /**
     * The core never persists passwords: on every start, once `Started` arrives, replay `AddServer`
     * for each stored login that matches a persisted server (or, when the core has no server yet,
     * every stored login). `ServersChanged` prunes logins for servers that were removed.
     */
    suspend fun replayCredentials(core: CoreHandle, store: ServerCredentialStore) {
        core.events.collect { event ->
            when (event) {
                is Event.Started -> {
                    val known = event.data.snapshot.servers
                    val stored = store.all()
                    val toReplay = if (known.isEmpty()) stored else stored.filter { c -> known.any { it.url.trimEnd('/') == c.url.trimEnd('/') && it.username == c.username } }
                    toReplay.forEach { c ->
                        Log.i(TAG, "replaying credentials for ${c.username}@${c.url}")
                        core.dispatch(Commands.addServer(c.url, c.username, c.password, c.name))
                    }
                    _credentialsReplayed.value = true
                }
                is Event.ServersChanged -> {
                    val keep = event.data.servers.map { it.url.trimEnd('/') to it.username }
                    if (_credentialsReplayed.value && keep.isNotEmpty()) store.retainOnly(keep)
                    // An emptied server list (RemoveServer) drops everything.
                    if (_credentialsReplayed.value && keep.isEmpty()) store.retainOnly(emptyList())
                }
                else -> Unit
            }
        }
    }

    /** Replaces the running core (tests). Shuts down the previous one. */
    @Synchronized
    fun install(core: CoreHandle) {
        handle?.close()
        handle = core
    }

    @Synchronized
    fun shutdown() {
        replayJob?.cancel()
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
