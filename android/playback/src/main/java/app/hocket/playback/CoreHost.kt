package app.hocket.playback

import android.content.Context
import android.content.pm.ApplicationInfo
import android.os.Build
import android.util.Log
import app.hocket.core.Commands
import app.hocket.core.CoreHandle
import app.hocket.core.CoreKind
import app.hocket.core.NativeCore
import app.hocket.core.api.AudioMode
import app.hocket.core.api.Command
import app.hocket.core.api.CoreConfig
import app.hocket.core.api.ErrorKind
import app.hocket.core.api.Event
import app.hocket.core.api.Platform
import app.hocket.core.api.Query
import app.hocket.core.api.QueryResult
import app.hocket.core.api.ServerInfo
import app.hocket.core.fake.FakeCore
import java.io.File
import java.util.UUID
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlin.system.exitProcess

/**
 * The single core instance for the process.
 *
 * [PlaybackService] owns its lifecycle (it is created on the service's first `onCreate` and shut down
 * when the service is destroyed), but because the activity, the service and the media session all
 * run in one process, the handle itself is a process-wide singleton that the bound
 * [PlaybackServiceConnection] hands to the UI. The real [NativeCore] is used when the native library
 * loads. In a debuggable build a [FakeCore] stands in when it does not (and [kind] tells the UI to
 * show its debug banner); in a release build the failure is fatal: [fatalError] is set, an inert
 * [DeadCore] keeps the service alive and the UI shows the error with a "reset data" action instead
 * of silently running against fake data.
 */
object CoreHost {
    private const val TAG = "CoreHost"
    private const val PREFS = "hocket-core"
    private const val KEY_DEVICE_ID = "deviceId"

    private val _handle = MutableStateFlow<CoreHandle?>(null)

    /** The running core, or null once it has been detached for shutdown. */
    val handle: StateFlow<CoreHandle?> = _handle.asStateFlow()

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

    /** `url|username` keys with a readable stored login. The UI shows the shell only for a server in here. */
    private val _logins = MutableStateFlow<Set<String>>(emptySet())
    val logins: StateFlow<Set<String>> = _logins.asStateFlow()

    /** Keys whose login is stored but unreadable right now (keystore unavailable): re-login needed, nothing deleted. */
    private val _unreadableLogins = MutableStateFlow<Set<String>>(emptySet())
    val unreadableLogins: StateFlow<Set<String>> = _unreadableLogins.asStateFlow()

    /** Why the native core could not be created (release builds only); the UI shows the fatal screen. */
    private val _fatalError = MutableStateFlow<String?>(null)
    val fatalError: StateFlow<String?> = _fatalError.asStateFlow()

    /**
     * Identifies this process's own bind to [PlaybackService]: the service is exported (Media3 needs
     * that) and `onBind` runs outside any binder transaction, so the calling uid is not available
     * there; a random per-process token that only in-process callers can know does the job.
     */
    val bindToken: String = UUID.randomUUID().toString()

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private var replayJob: Job? = null
    private var pendingShutdown: Job? = null

    /** A login entered in setup, held until the core confirms it (see [replayCredentials]). */
    @Volatile
    private var pendingLogin: ServerCredential? = null

    @Volatile
    private var lastServers: List<ServerInfo> = emptyList()

    val current: CoreHandle? get() = _handle.value
    val kind: CoreKind? get() = _handle.value?.kind

    /** Returns the running core, creating and starting it if needed. Safe to call from any thread. */
    @Synchronized
    fun acquire(context: Context): CoreHandle {
        _handle.value?.let { return it }
        val app = context.applicationContext
        // One core per data dir: a previous instance may still be flushing when the service is
        // re-created quickly after being destroyed.
        pendingShutdown?.let { job -> runBlocking { job.join() }; pendingShutdown = null }
        val core = createCore(app)
        val store = credentials ?: KeystoreCredentialStore(app).also { credentials = it }
        _credentialsReplayed.value = false
        replayJob?.cancel()
        // UNDISPATCHED: the collector is subscribed before Start is dispatched, so the first
        // `Started` cannot slip past it.
        replayJob = scope.launch(start = CoroutineStart.UNDISPATCHED) { replayCredentials(core, store) }
        core.dispatch(Command.Start)
        _handle.value = core
        return core
    }

    private fun createCore(app: Context): CoreHandle {
        if (forceFake) return FakeCore(startWithServer = false, startPlaying = false)
        val debuggable = (app.applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE) != 0
        if (!NativeCore.isAvailable()) {
            if (debuggable) {
                Log.w(TAG, "libhocket_android not built for this ABI; using the fake core")
                return FakeCore(startWithServer = false, startPlaying = false)
            }
            _fatalError.value = "The native core is not built for this device (${Build.SUPPORTED_ABIS.joinToString()})."
            return DeadCore
        }
        return try {
            NativeCore.create(config(app))
        } catch (e: Exception) {
            Log.e(TAG, "Native core failed to start", e)
            if (debuggable) {
                Log.e(TAG, "using the fake core (debug build)")
                FakeCore(startWithServer = false, startPlaying = false)
            } else {
                _fatalError.value = e.message?.takeIf { it.isNotBlank() } ?: e.javaClass.simpleName
                DeadCore
            }
        }
    }

    /**
     * The core never persists passwords. Once per core instance, when the first `Started` arrives,
     * replay `AddServer` for each stored login that matches a persisted server (or, when the core has
     * no server yet, every stored login). The core emits `Started` once per instance (`RequestSnapshot`
     * and `RemoveServer` re-emit everything as `Snapshot`); should a second `Started` ever arrive it
     * is not replayed either. `ServersChanged` prunes logins for servers that were
     * removed and confirms a pending setup login (saved only once the server is reachable with it);
     * `Error{auth}` drops a pending login, or removes the stored one the server now refuses.
     */
    suspend fun replayCredentials(core: CoreHandle, store: ServerCredentialStore) {
        var replayed = false
        core.events.collect { event ->
            when (event) {
                is Event.Started -> {
                    val known = event.data.snapshot.servers
                    lastServers = known
                    if (!replayed) {
                        replayed = true
                        val stored = store.all()
                        val toReplay = if (known.isEmpty()) stored else stored.filter { c -> known.any { matches(it, c) } }
                        toReplay.forEach { c ->
                            Log.i(TAG, "replaying credentials for ${c.username}@${c.url}")
                            core.dispatch(Commands.addServer(c.url, c.username, c.password, c.name))
                        }
                        refreshLogins(store)
                        _credentialsReplayed.value = true
                    }
                }
                // RequestSnapshot (every UI attach, every service reconnect) and RemoveServer re-emit
                // the state as `Snapshot`, never as a second `Started`: nothing to replay here.
                is Event.Snapshot -> lastServers = event.data.snapshot.servers
                is Event.ServersChanged -> {
                    val servers = event.data.servers
                    lastServers = servers
                    pendingLogin?.let { p ->
                        if (servers.any { it.reachable && matches(it, p) }) {
                            store.save(p)
                            pendingLogin = null
                        }
                    }
                    // Only after the replay: the start-up list is what the replay is keyed on. An
                    // emptied list (RemoveServer) drops everything.
                    if (replayed) store.retainOnly(servers.map { it.url.trimEnd('/') to it.username })
                    refreshLogins(store)
                }
                is Event.Error -> if (event.data.kind == ErrorKind.Auth) {
                    if (pendingLogin != null) {
                        pendingLogin = null // refused: never saved
                    } else {
                        lastServers.firstOrNull()?.let { s ->
                            Log.i(TAG, "stored login for ${s.username}@${s.url} was refused; removing it")
                            store.remove(s.url, s.username)
                            refreshLogins(store)
                        }
                    }
                }
                else -> Unit
            }
        }
    }

    private fun matches(server: ServerInfo, c: ServerCredential) =
        server.url.trimEnd('/') == c.url.trimEnd('/') && server.username == c.username

    private fun refreshLogins(store: ServerCredentialStore) {
        _logins.value = store.all().map { ServerCredentialStore.key(it.url, it.username) }.toSet()
        _unreadableLogins.value = store.unavailable().map { (url, user) -> ServerCredentialStore.key(url, user) }.toSet()
    }

    /** True when [logins] (a collected value) holds a login for [server]. */
    fun hasLogin(server: ServerInfo, logins: Set<String>): Boolean = ServerCredentialStore.key(server.url, server.username) in logins

    fun needsRelogin(server: ServerInfo, unreadable: Set<String>): Boolean = ServerCredentialStore.key(server.url, server.username) in unreadable

    /**
     * Setup: dispatch `AddServer` and hold the login until the core confirms it. It is saved on the
     * first `ServersChanged` listing the server as reachable and dropped on `Error{auth}`, so a
     * mistyped password or url is never stored and replayed on every later launch.
     */
    fun login(dispatch: (Command) -> Unit, credential: ServerCredential) {
        pendingLogin = credential
        dispatch(Commands.addServer(credential.url, credential.username, credential.password, credential.name))
    }

    /**
     * Sign out, for every screen that offers it: the stored login goes first, then `RemoveServer`.
     * The core re-emits `Started` while clearing the server; with the login already gone (and the
     * replay done once per core anyway) nothing can resurrect the server.
     */
    fun removeServer(dispatch: (Command) -> Unit, server: ServerInfo) {
        pendingLogin = null
        credentials?.let { it.remove(server.url, server.username); refreshLogins(it) }
        dispatch(Commands.removeServer(server.id))
    }

    /** Clears the login bookkeeping between tests (the object is process-global). */
    internal fun resetForTests() {
        pendingLogin = null
        lastServers = emptyList()
        _logins.value = emptySet()
        _unreadableLogins.value = emptySet()
        _credentialsReplayed.value = false
        _fatalError.value = null
    }

    /** Replaces the running core (tests). Shuts down the previous one. */
    @Synchronized
    fun install(core: CoreHandle) {
        _handle.value?.close()
        _handle.value = core
    }

    /**
     * Detaches the running core and flushes it on a worker thread: [NativeCore.close] blocks until
     * the actor has written the session document, position and settings (bounded by its timeout),
     * which must not happen on the main thread in `Service.onDestroy`.
     */
    @Synchronized
    fun shutdown() {
        replayJob?.cancel()
        replayJob = null
        pendingLogin = null
        val core = _handle.value ?: return
        _handle.value = null
        pendingShutdown = scope.launch(Dispatchers.IO) {
            try {
                core.close()
            } catch (t: Throwable) {
                Log.e(TAG, "core shutdown failed", t)
            }
        }
    }

    /**
     * The fatal screen's "reset data": stops whatever core there is, deletes the core's data dir
     * (library mirror, downloads, session) and the stream cache, keeps the stored logins, then ends
     * the process so the next launch starts clean. [exit] is a seam for tests.
     */
    fun resetData(context: Context, exit: () -> Unit = { exitProcess(0) }) {
        shutdown()
        synchronized(this) { pendingShutdown }?.let { job -> runBlocking { job.join() } }
        val app = context.applicationContext
        File(app.filesDir, "hocket").deleteRecursively()
        File(app.cacheDir, "stream").deleteRecursively()
        _fatalError.value = null
        exit()
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

    /**
     * A stable per-install id, generated once and persisted in a prefs file that is excluded from
     * backup and device transfer (`backup_rules.xml`), so a restored install gets a fresh id and two
     * devices never share one in Connect leases and elections.
     */
    fun deviceId(context: Context): String {
        val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        prefs.getString(KEY_DEVICE_ID, null)?.let { return it }
        val id = UUID.randomUUID().toString().replace("-", "")
        prefs.edit().putString(KEY_DEVICE_ID, id).apply()
        return id
    }
}

/**
 * Stands in when the native core cannot be created in a release build: emits nothing, drops every
 * command, refuses queries. The service and the UI still come up, and the UI shows
 * [CoreHost.fatalError] instead of a shell.
 */
object DeadCore : CoreHandle {
    override val kind: CoreKind = CoreKind.Fake
    override val events: SharedFlow<Event> = MutableSharedFlow()
    override fun dispatch(command: Command) = Unit
    override suspend fun query(query: Query): QueryResult = throw IllegalStateException("the core is unavailable")
    override fun close() = Unit
}
