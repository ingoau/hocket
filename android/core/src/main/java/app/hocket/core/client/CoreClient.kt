package app.hocket.core.client

import app.hocket.core.Commands
import app.hocket.core.CoreHandle
import app.hocket.core.CoreKind
import app.hocket.core.Queries
import app.hocket.core.api.*
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** Identifies a track list for the page cache. */
data class TrackListKey(val serverId: ServerId, val sort: SortOrder, val descending: Boolean, val filter: FilterNode? = null)
data class AlbumListKey(val serverId: ServerId, val sort: SortOrder, val descending: Boolean, val artistId: ArtistId? = null, val genre: String? = null)
data class ArtistListKey(val serverId: ServerId)
data class PlaylistTracksKey(val playlistId: PlaylistId)

/** The handoff picker as the UI sees it. */
data class HandoffPicker(val open: Boolean = false, val targets: List<DeviceInfo> = emptyList())

/** What kind of ids a selection holds, so toolbar actions can be queried for the right target. */
enum class SelectionKind { Tracks, Albums, Artists, Playlists, QueueItems }

/**
 * The app-side view of the core: forwards commands and queries, folds the event stream into
 * StateFlows for each snapshot piece, extrapolates position, caches list pages and holds the
 * multi-selection. No business logic lives here — every rule is the core's; this only shapes state
 * for Compose.
 *
 * One instance per bound core, owned by the Application. Screens read the StateFlows with
 * `collectAsStateWithLifecycle` so the position ticker (a `WhileSubscribed` flow) runs only while a
 * screen that shows position is resumed.
 */
class CoreClient(
    val core: CoreHandle,
    private val scope: CoroutineScope,
    private val now: () -> Double = { System.currentTimeMillis().toDouble() },
) {
    val kind: CoreKind get() = core.kind

    // -- snapshot pieces ---------------------------------------------------------------------------
    private val _servers = MutableStateFlow<List<ServerInfo>>(emptyList())
    val servers: StateFlow<List<ServerInfo>> = _servers.asStateFlow()
    private val _session = MutableStateFlow<SessionDocument?>(null)
    val session: StateFlow<SessionDocument?> = _session.asStateFlow()
    private val _queue = MutableStateFlow(emptyQueue())
    val queue: StateFlow<QueueView> = _queue.asStateFlow()
    private val _transport = MutableStateFlow(emptyTransport())
    val transport: StateFlow<TransportState> = _transport.asStateFlow()
    private val _nowPlaying = MutableStateFlow<QueueEntry?>(null)
    val nowPlaying: StateFlow<QueueEntry?> = _nowPlaying.asStateFlow()
    private val _connection = MutableStateFlow(emptyConnection())
    val connection: StateFlow<ConnectionState> = _connection.asStateFlow()
    private val _devices = MutableStateFlow<List<DeviceInfo>>(emptyList())
    val devices: StateFlow<List<DeviceInfo>> = _devices.asStateFlow()
    private val _handoff = MutableStateFlow(HandoffPicker())
    val handoffPicker: StateFlow<HandoffPicker> = _handoff.asStateFlow()
    private val _jobs = MutableStateFlow<List<Job>>(emptyList())
    val jobs: StateFlow<List<Job>> = _jobs.asStateFlow()
    private val _problems = MutableStateFlow<List<Problem>>(emptyList())
    val problems: StateFlow<List<Problem>> = _problems.asStateFlow()
    private val _undo = MutableStateFlow(emptyUndo())
    val undo: StateFlow<UndoState> = _undo.asStateFlow()
    private val _settings = MutableStateFlow<Map<String, Setting>>(emptyMap())
    val settings: StateFlow<Map<String, Setting>> = _settings.asStateFlow()
    private val _audio = MutableStateFlow(defaultAudioSettings())
    val audio: StateFlow<AudioSettings> = _audio.asStateFlow()
    private val _mediaSession = MutableStateFlow(emptyMediaSession())
    val mediaSession: StateFlow<MediaSessionState> = _mediaSession.asStateFlow()
    private val _resumeOffer = MutableStateFlow<ResumeOffer?>(null)
    val resumeOffer: StateFlow<ResumeOffer?> = _resumeOffer.asStateFlow()
    private val _sleepTimer = MutableStateFlow<SleepTimer?>(null)
    val sleepTimer: StateFlow<SleepTimer?> = _sleepTimer.asStateFlow()
    private val _network = MutableStateFlow<NetworkState?>(null)
    val network: StateFlow<NetworkState?> = _network.asStateFlow()
    private val _batterySaver = MutableStateFlow(false)
    val batterySaver: StateFlow<Boolean> = _batterySaver.asStateFlow()
    private val _syncProgress = MutableStateFlow<SyncProgress?>(null)
    val syncProgress: StateFlow<SyncProgress?> = _syncProgress.asStateFlow()
    private val _savedQueues = MutableStateFlow<List<SavedQueue>>(emptyList())
    val savedQueues: StateFlow<List<SavedQueue>> = _savedQueues.asStateFlow()
    private val _pins = MutableStateFlow<List<Pin>>(emptyList())
    val pins: StateFlow<List<Pin>> = _pins.asStateFlow()
    private val _storage = MutableStateFlow(StorageSummary(0.0, 0.0, 0.0))
    val storage: StateFlow<StorageSummary> = _storage.asStateFlow()
    private val _filters = MutableStateFlow<List<Filter>>(emptyList())
    val filters: StateFlow<List<Filter>> = _filters.asStateFlow()
    private val _outputDevices = MutableStateFlow<List<OutputDevice>>(emptyList())
    val outputDevices: StateFlow<List<OutputDevice>> = _outputDevices.asStateFlow()
    private val _playerNotice = MutableStateFlow<String?>(null)
    val playerNotice: StateFlow<String?> = _playerNotice.asStateFlow()
    private val _lyrics = MutableStateFlow<Map<TrackId, Lyrics?>>(emptyMap())
    /** Lyrics per track id as they arrive (`null` = fetched, none). */
    val lyrics: StateFlow<Map<TrackId, Lyrics?>> = _lyrics.asStateFlow()
    private val _started = MutableStateFlow(false)
    /** True once the first snapshot has arrived; screens show a loading indicator before that. */
    val started: StateFlow<Boolean> = _started.asStateFlow()

    // -- one-shot streams --------------------------------------------------------------------------
    private val _toasts = MutableSharedFlow<Toast>(extraBufferCapacity = 16)
    val toasts: SharedFlow<Toast> = _toasts.asSharedFlow()
    private val _searchResults = MutableSharedFlow<SearchResults>(extraBufferCapacity = 16)
    val searchResults: SharedFlow<SearchResults> = _searchResults.asSharedFlow()
    private val _errors = MutableSharedFlow<EventErrorInner>(extraBufferCapacity = 16)
    val errors: SharedFlow<EventErrorInner> = _errors.asSharedFlow()
    private val _libraryChanged = MutableSharedFlow<EventLibraryChangedInner>(extraBufferCapacity = 16)
    val libraryChanged: SharedFlow<EventLibraryChangedInner> = _libraryChanged.asSharedFlow()
    private val _exports = MutableSharedFlow<Event>(extraBufferCapacity = 4)
    /** `NspExported` and `ConfigExported`, for the share sheet. */
    val exports: SharedFlow<Event> = _exports.asSharedFlow()
    private val _actionsChanged = MutableSharedFlow<String>(extraBufferCapacity = 4)
    val actionsChanged: SharedFlow<String> = _actionsChanged.asSharedFlow()

    // -- derived -----------------------------------------------------------------------------------
    /** The single configured server (multi-server has no UI yet). */
    val server: StateFlow<ServerInfo?> = _servers.mapState(scope) { it.firstOrNull() }
    val isPlaying: StateFlow<Boolean> = _transport.mapState(scope) { it.position.isPlaying }
    val ownsTransport: StateFlow<Boolean> = _mediaSession.mapState(scope) { it.ownsTransport }
    /** Jobs that are still doing something. */
    val activeJobs: StateFlow<List<Job>> = _jobs.mapState(scope) { jobs -> jobs.filter { it.state == JobState.Queued || it.state == JobState.Running || it.state == JobState.Paused } }
    /** "Finished with problems" for the indicator. */
    val finishedWithProblems: StateFlow<Boolean> = _jobs.mapState(scope) { jobs -> jobs.any { it.state == JobState.Failed } }

    // -- position ----------------------------------------------------------------------------------
    /** Extrapolated position right now, for frame-driven consumers (lyrics via `withFrameNanos`). */
    fun positionNow(): Long {
        val duration = _nowPlaying.value?.track?.durationMs?.toLong()
        return PositionClock.extrapolate(_transport.value, now(), _connection.value.clockOffsetMs, duration)
    }

    /** ~60 Hz position while subscribed; nothing runs when no screen collects it. */
    val position: StateFlow<Long> = flow {
        while (true) {
            emit(positionNow())
            delay(16)
        }
    }.stateIn(scope, SharingStarted.WhileSubscribed(stopTimeoutMillis = 500), 0L)

    // -- page caches -------------------------------------------------------------------------------
    val trackPages = PageCache<TrackListKey, Track>(scope) { key, offset, limit ->
        when (val r = query(Queries.tracks(key.serverId, Page(offset.toUInt(), limit.toUInt()), key.sort, key.descending, key.filter))) {
            is QueryResult.Tracks -> PageCache.Page(r.data.items, r.data.offset.toInt(), r.data.total.toInt())
            else -> PageCache.Page(emptyList(), offset, 0)
        }
    }
    val albumPages = PageCache<AlbumListKey, Album>(scope) { key, offset, limit ->
        when (val r = query(Queries.albums(key.serverId, Page(offset.toUInt(), limit.toUInt()), key.sort, key.descending, key.artistId, key.genre))) {
            is QueryResult.Albums -> PageCache.Page(r.data.items, r.data.offset.toInt(), r.data.total.toInt())
            else -> PageCache.Page(emptyList(), offset, 0)
        }
    }
    val artistPages = PageCache<ArtistListKey, Artist>(scope, pageSize = 100) { key, offset, limit ->
        when (val r = query(Queries.artists(key.serverId, Page(offset.toUInt(), limit.toUInt())))) {
            is QueryResult.Artists -> PageCache.Page(r.data.items, r.data.offset.toInt(), r.data.total.toInt())
            else -> PageCache.Page(emptyList(), offset, 0)
        }
    }
    val playlistTrackPages = PageCache<PlaylistTracksKey, Track>(scope) { key, offset, limit ->
        when (val r = query(Queries.playlistTracks(key.playlistId, Page(offset.toUInt(), limit.toUInt())))) {
            is QueryResult.Tracks -> PageCache.Page(r.data.items, r.data.offset.toInt(), r.data.total.toInt())
            else -> PageCache.Page(emptyList(), offset, 0)
        }
    }

    // -- selection ---------------------------------------------------------------------------------
    private val _selection = MutableStateFlow(Selection())
    val selection: StateFlow<Selection> = _selection.asStateFlow()
    private val _selectionKind = MutableStateFlow(SelectionKind.Tracks)
    val selectionKind: StateFlow<SelectionKind> = _selectionKind.asStateFlow()

    fun toggleSelected(kind: SelectionKind, id: String) {
        if (_selectionKind.value != kind && _selection.value.active) clearSelection()
        _selectionKind.value = kind
        _selection.update { it.toggle(id) }
        publishSelection()
    }

    fun selectAll(kind: SelectionKind, total: Int) {
        _selectionKind.value = kind
        _selection.update { it.selectAll(total) }
        publishSelection()
    }

    fun setSelectionTotal(total: Int) = _selection.update { it.withTotal(total) }

    fun clearSelection() {
        if (_selection.value.active) {
            _selection.update { it.clear() }
            publishSelection()
        }
    }

    /** The core snapshots the selection alongside commands so undo can restore it. */
    private fun publishSelection() {
        val sel = _selection.value
        val ids = sel.ids.toList()
        val target = if (!sel.active) ActionTarget.None else when (_selectionKind.value) {
            SelectionKind.Tracks -> Commands.tracks(ids)
            SelectionKind.Albums -> Commands.albums(ids)
            SelectionKind.Artists -> Commands.artists(ids)
            SelectionKind.Playlists -> Commands.playlists(ids)
            SelectionKind.QueueItems -> Commands.queueItems(ids)
        }
        dispatch(Commands.setSelection(target))
    }

    /** The selection as an action target for `Query.Actions`. Inverted selections send the known ids. */
    fun selectionTarget(): ActionTarget {
        val ids = _selection.value.ids.toList()
        return when (_selectionKind.value) {
            SelectionKind.Tracks -> Commands.tracks(ids)
            SelectionKind.Albums -> Commands.albums(ids)
            SelectionKind.Artists -> Commands.artists(ids)
            SelectionKind.Playlists -> Commands.playlists(ids)
            SelectionKind.QueueItems -> Commands.queueItems(ids)
        }
    }

    // -- plumbing ----------------------------------------------------------------------------------
    private val collector: kotlinx.coroutines.Job = scope.launch { core.events.collect(::onEvent) }

    fun dispatch(command: Command) = core.dispatch(command)
    suspend fun query(query: Query): QueryResult = core.query(query)

    /** Ask the core to re-emit everything. Call on (re)attach. */
    fun requestSnapshot() = dispatch(Command.RequestSnapshot)

    /** Resolves artwork at one of the fixed sizes to a local path, or null. */
    suspend fun artworkPath(id: String?, size: Int): String? {
        if (id.isNullOrEmpty()) return null
        return (query(Queries.artwork(id, size)) as? QueryResult.Path)?.data
    }

    suspend fun actions(surface: String, target: ActionTarget): List<ActionDescriptor> =
        (query(Queries.actions(surface, target)) as? QueryResult.Actions)?.data ?: emptyList()

    fun applySnapshot(s: Snapshot) {
        _servers.value = s.servers
        _session.value = s.session
        _queue.value = s.queue
        _transport.value = s.transport
        _nowPlaying.value = s.queue.current
        _connection.value = s.connection
        _devices.value = s.devices
        _jobs.value = s.jobs
        _problems.value = s.problems
        _undo.value = s.undo
        _settings.value = s.settings.associateBy { it.key }
        _audio.value = s.audio
        _mediaSession.value = s.mediaSession
        _resumeOffer.value = s.resumeOffer
        _sleepTimer.value = s.sleepTimer
        _network.value = s.network
        _batterySaver.value = s.batterySaver
        _syncProgress.value = s.syncProgress
        s.session?.let { _savedQueues.value = it.savedQueues }
        _started.value = true
    }

    fun onEvent(event: Event) {
        when (event) {
            is Event.Started -> applySnapshot(event.data.snapshot)
            is Event.ServersChanged -> _servers.value = event.data.servers
            is Event.SyncProgress -> _syncProgress.value = event.data.progress
            is Event.LibraryChanged -> {
                val sid = event.data.server_id
                trackPages.invalidate { it.serverId == sid }
                albumPages.invalidate { it.serverId == sid }
                artistPages.invalidate { it.serverId == sid }
                playlistTrackPages.invalidate()
                _libraryChanged.tryEmit(event.data)
            }
            is Event.SearchResults -> _searchResults.tryEmit(event.data.results)
            is Event.SessionChanged -> {
                _session.value = event.data.document
                _savedQueues.value = event.data.document.savedQueues
            }
            is Event.QueueChanged -> {
                _queue.value = event.data.queue
                _nowPlaying.value = event.data.queue.current
            }
            is Event.TransportChanged -> _transport.value = event.data.transport
            is Event.NowPlayingChanged -> _nowPlaying.value = event.data.entry
            is Event.SavedQueuesChanged -> _savedQueues.value = event.data.queues
            is Event.UndoChanged -> _undo.value = event.data.state
            is Event.Toast -> _toasts.tryEmit(event.data.toast)
            is Event.PlayerNotice -> _playerNotice.value = event.data.message
            is Event.JobsChanged -> _jobs.value = event.data.jobs
            is Event.ProblemsChanged -> _problems.value = event.data.problems
            is Event.ConnectionChanged -> _connection.value = event.data.state
            is Event.DevicesChanged -> _devices.value = event.data.devices
            is Event.HandoffPickerChanged -> _handoff.value = HandoffPicker(event.data.open, event.data.targets)
            is Event.ResumeOfferChanged -> _resumeOffer.value = event.data.offer
            is Event.LyricsChanged -> _lyrics.update { it + (event.data.track_id to event.data.lyrics) }
            is Event.PinsChanged -> _pins.value = event.data.pins
            is Event.StorageChanged -> _storage.value = event.data.storage
            is Event.FiltersChanged -> _filters.value = event.data.filters
            is Event.SettingChanged -> _settings.update { it + (event.data.setting.key to event.data.setting) }
            is Event.AudioSettingsChanged -> _audio.value = event.data.settings
            is Event.OutputDevicesChanged -> _outputDevices.value = event.data.devices
            is Event.SleepTimerChanged -> _sleepTimer.value = event.data.timer
            is Event.ShortcutsChanged -> Unit // desktop only
            is Event.ActionsChanged -> _actionsChanged.tryEmit(event.data.surface)
            is Event.NspExported, is Event.ConfigExported -> _exports.tryEmit(event)
            is Event.Backend, is Event.MediaSession -> Unit // consumed by the playback service
            is Event.Error -> _errors.tryEmit(event.data)
            is Event.Log -> Unit
        }
    }

    fun close() {
        collector.cancel()
    }

    companion object {
        fun emptyQueue() = QueueView(null, emptyList(), null, emptyList(), emptyList(), false, RepeatMode.Off, false, QueueMode.Apple, 0u)
        fun emptyTransport() = TransportState(TransportLease(null, 0u, 0.0), PositionStamp(0u, 0.0, 1.0, false), false, 0u, 1.0)
        fun emptyConnection() = ConnectionState(ConnectionTier.Local, false, null, 0.0, null, 0u, null)
        fun emptyUndo() = UndoState(false, null, false, null, emptyList())
        fun emptyMediaSession() = MediaSessionState(null, false, PositionStamp(0u, 0.0, 1.0, false), false, RepeatMode.Off, 1.0, emptyList(), true)
        fun defaultAudioSettings() = AudioSettings(ReplayGainMode.Auto, 0.0, false, EqSettings(false, 0.0, emptyList(), null), true, null, false)
    }
}

private fun <T, R> StateFlow<T>.mapState(scope: CoroutineScope, transform: (T) -> R): StateFlow<R> {
    val out = MutableStateFlow(transform(value))
    scope.launch { collect { out.value = transform(it) } }
    return out.asStateFlow()
}
