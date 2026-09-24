package app.hocket.core.fake

import app.hocket.core.CoreHandle
import app.hocket.core.CoreKind
import app.hocket.core.ActionIds
import app.hocket.core.HocketJson
import app.hocket.core.SettingKeys
import app.hocket.core.api.*
import app.hocket.core.toSummary
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.builtins.MapSerializer
import kotlinx.serialization.builtins.serializer
import kotlin.random.Random

/**
 * An in-process Kotlin stand-in for the Rust core.
 *
 * It keeps just enough of the session model to make every screen work against realistic state:
 * a context with cursor, history, insertions, shuffle, repeat and autoplay; transport with position
 * stamps (extrapolated by the client, exactly like the real core); saved queues; a snapshot undo
 * stack; jobs that progress; problems; pins; filters; scoped settings; lyrics; devices, the
 * handoff picker and a dormant resume offer. Every command is applied on a single dispatcher and
 * every change is announced with the same events the real core emits.
 *
 * It never emits [Event.Backend] — playback is simulated by advancing the stamp to the track end.
 * It is not the reducer; it is a plausible imitation for previews, UI tests and running without the
 * native library.
 */
class FakeCore(
    parentScope: CoroutineScope? = null,
    seed: Long = 42L,
    val library: FakeLibrary = FakeLibrary(seed),
    /** Start with a configured server (previews) or on the setup screen. */
    startWithServer: Boolean = true,
    /** Start with something playing. */
    startPlaying: Boolean = true,
    /** Disable timers (track-end advance, job progress, handoff readiness) for deterministic tests. */
    private val timers: Boolean = true,
    private val now: () -> Double = { System.currentTimeMillis().toDouble() },
    private val dispatcher: CoroutineDispatcher = Dispatchers.Default.limitedParallelism(1),
) : CoreHandle {
    override val kind: CoreKind = CoreKind.Fake
    private val scope = CoroutineScope((parentScope?.coroutineContext ?: SupervisorJob()) + dispatcher + SupervisorJob())
    private val _events = MutableSharedFlow<Event>(extraBufferCapacity = 1024)
    override val events: SharedFlow<Event> = _events.asSharedFlow()
    private val rng = Random(seed)
    private val serverId = library.serverId
    val deviceId = "fake-device"

    // -- state -------------------------------------------------------------------------------------
    private var servers = ArrayList<ServerInfo>()
    private var context: QueueContext? = null
    private var contextTracks: List<TrackId> = emptyList()
    private var order: List<Int> = emptyList() // permuted indices into contextTracks
    private var cursor = 0
    private var current: QueueItem? = null
    private val history = ArrayList<QueueItem>()
    private val insertions = ArrayList<QueueItem>()
    private var shuffle: ShuffleState? = null
    private var repeat = RepeatMode.Off
    private var autoplay = true
    private var mode = QueueMode.Apple
    private var revision = 0u
    private var stamp = PositionStamp(0u, now(), 1.0, false)
    private var volume = 1.0
    private var buffering = false
    private var transportOwner: DeviceId? = deviceId
    private var epoch = 1u
    private var playedMs = 0L
    private val savedQueues = ArrayList<SavedQueue>()
    private val undoStack = ArrayList<Pair<UndoEntry, () -> Unit>>()
    private val redoStack = ArrayList<Pair<UndoEntry, () -> Unit>>()
    private val jobs = ArrayList<Job>()
    private val problems = ArrayList<Problem>()
    private val pins = ArrayList<Pin>()
    private val filters = ArrayList<Filter>()
    private val settings = LinkedHashMap<String, Setting>()
    private val defaults = HashMap<String, String>()
    private var audio = AudioSettings(ReplayGainMode.Auto, 0.0, false, EqSettings(false, 0.0, defaultBands(), null), true, null, false)
    private var sleepTimer: SleepTimer? = null
    private var network: NetworkState? = null
    private var batterySaver = false
    private var started = false
    private var syncProgress: SyncProgress? = null
    private var connection = ConnectionState(ConnectionTier.Lan, true, null, 0.0, 12.0, 2u, null)
    private val devices = ArrayList<DeviceInfo>()
    private var pickerOpen = false
    private var resumeOffer: ResumeOffer? = null
    private var playerNotice: String? = null
    private var selection: ActionTarget = ActionTarget.None
    private val actionOrders = HashMap<String, List<String>>()
    private val lyricsOffsets = HashMap<TrackId, Int>()
    private var externalLyrics = false
    private var autoplaySettings = AutoplaySettings(listOf(AutoplayProvider.SonicSimilarity, AutoplayProvider.SimilarSongs, AutoplayProvider.TopSongs, AutoplayProvider.Random), 5u, 0.6, 50u, null)
    private var savedQueueCap = 10
    private var warnThreshold: Double? = 4.0 * 1024 * 1024 * 1024
    private val playHistory = ArrayList<PlayHistoryEntry>()
    private var keyCounter = 0
    private var idCounter = 0
    private var advanceJob: kotlinx.coroutines.Job? = null
    private var storage = StorageSummary(1.8e9, 6.4e8, 1.2e8, warnThreshold, 2.4e10)

    init {
        devices += DeviceInfo(deviceId, "This phone", Platform.Android, "0.1.0", true, false, now(), true)
        devices += DeviceInfo("laptop", "Laptop", Platform.Linux, "0.1.0", false, false, now() - 20_000, false)
        devices += DeviceInfo("living-room", "Living room", Platform.MacOs, "0.1.0", false, false, now() - 60_000, false)
        // The registry's keys, scopes and defaults (settings/registry.rs).
        fun def(key: String, value: String, scope: SettingScope) { settings[key] = Setting(key, value, scope, now()) }
        def(SettingKeys.QUEUE_MODE, "\"apple\"", SettingScope.AccountSynced)
        def(SettingKeys.QUEUE_SAVED_CAP, "10", SettingScope.AccountSynced)
        def(SettingKeys.QUEUE_HISTORY_CAP, "200", SettingScope.AccountSynced)
        def(SettingKeys.TRANSCODING_PROFILES, "{\"default\":{\"format\":null,\"maxBitRate\":null,\"cannotDecode\":[]},\"cellular\":{\"format\":\"opus\",\"maxBitRate\":128,\"cannotDecode\":[]}}", SettingScope.DeviceLocal)
        def(SettingKeys.LYRICS_EXTERNAL_ENABLED, "false", SettingScope.AccountSynced)
        def(SettingKeys.LYRICS_EXTERNAL_PROVIDER, "\"lrclib\"", SettingScope.AccountSynced)
        def(SettingKeys.LYRICS_DEFAULT_OFFSET_MS, "0", SettingScope.DeviceLocal)
        def(SettingKeys.LYRICS_SHOW_TRANSLATIONS, "true", SettingScope.AccountSynced)
        def(SettingKeys.RATINGS_LOVE_BRIDGE_ENABLED, "false", SettingScope.AccountSynced)
        def(SettingKeys.RATINGS_LOVE_BRIDGE_THRESHOLD, "4", SettingScope.AccountSynced)
        def(SettingKeys.BATTERY_AUTO_ENGAGE, "true", SettingScope.DeviceLocal)
        def(SettingKeys.BATTERY_LYRICS_FPS, "30", SettingScope.DeviceLocal)
        def(SettingKeys.BATTERY_SMALL_ARTWORK, "true", SettingScope.DeviceLocal)
        def(SettingKeys.BATTERY_PAUSE_PREFETCH, "true", SettingScope.DeviceLocal)
        def(SettingKeys.DISPLAY_ANIMATED_BACKGROUND, "true", SettingScope.DeviceLocal)
        def(SettingKeys.DISPLAY_LYRICS_FPS, "60", SettingScope.DeviceLocal)
        def(SettingKeys.DISPLAY_THEME, "\"system\"", SettingScope.DeviceLocal)
        def(SettingKeys.DISPLAY_ACCENT, "null", SettingScope.DeviceLocal)
        def(SettingKeys.DISPLAY_DYNAMIC_COLOUR, "true", SettingScope.DeviceLocal)
        def(SettingKeys.ACTIONS_ORDER_CONTEXT_MENU, "[]", SettingScope.AccountSynced)
        def(SettingKeys.ACTIONS_ORDER_SIDEBAR, "[]", SettingScope.AccountSynced)
        def(SettingKeys.ACTIONS_ORDER_MEDIA_SESSION, "[]", SettingScope.AccountSynced)
        def(SettingKeys.SYNC_ENABLED, "true", SettingScope.DeviceLocal)
        def(SettingKeys.CONNECT_COORDINATOR_URL, "null", SettingScope.DeviceLocal)
        def(SettingKeys.CONNECT_LAN_DISCOVERY, "true", SettingScope.DeviceLocal)
        def(SettingKeys.STORAGE_WARN_THRESHOLD_BYTES, "4294967296.0", SettingScope.DeviceLocal)
        def(SettingKeys.STORAGE_CACHE_MAX_BYTES, "2147483648.0", SettingScope.DeviceLocal)
        def(SettingKeys.DOWNLOADS_TRANSCODE, "false", SettingScope.DeviceLocal)
        def(SettingKeys.DOWNLOADS_WIFI_ONLY, "true", SettingScope.DeviceLocal)
        def(SettingKeys.SLEEP_DEFAULT_MINUTES, "30", SettingScope.AccountSynced)
        def(SettingKeys.SLEEP_STOP_AT_END_OF_TRACK, "true", SettingScope.AccountSynced)
        def(SettingKeys.SCROBBLE_ENABLED, "true", SettingScope.AccountSynced)
        def(SettingKeys.SCROBBLE_NOW_PLAYING, "true", SettingScope.AccountSynced)
        def(SettingKeys.LIBRARY_SYNC_INTERVAL_MINUTES, "60", SettingScope.DeviceLocal)
        def(SettingKeys.LIBRARY_FULL_RECONCILE_DAYS, "7", SettingScope.DeviceLocal)
        def(SettingKeys.SEARCH_INCLUDE_SERVER, "true", SettingScope.AccountSynced)
        settings.forEach { (k, v) -> defaults[k] = v.value }
        filters += Filter("f-loved", "Loved, not played lately", FilterNode.All(listOf(
            FilterNode.Rule(FilterRule(FilterField.Loved, FilterOp.IsTrue, FilterValue.Bool(true))),
            FilterNode.Rule(FilterRule(FilterField.LastPlayed, FilterOp.NotInTheLast, FilterValue.Days(30u))),
        )), SortOrder.Random, false, 100u)
        filters += Filter("f-offline", "Downloaded flac", FilterNode.All(listOf(
            FilterNode.Rule(FilterRule(FilterField.Downloaded, FilterOp.IsTrue, FilterValue.Bool(true))),
            FilterNode.Rule(FilterRule(FilterField.FileType, FilterOp.Is, FilterValue.Text("flac"))),
        )), SortOrder.Title, false, null)
        pins += Pin(PinTarget.Playlist(PinTargetPlaylistInner("pl0")), "Late night", library.playlist("pl0")?.coverArt, 18u, 18u, 4.1e8, now() - 5 * 86_400_000.0, false)
        pins += Pin(PinTarget.Album(PinTargetAlbumInner("al3")), library.album("al3")?.name ?: "Album", "al3", 9u, 6u, 2.2e8, now() - 3_600_000.0, false)
        problems += Problem("p1", "job-sync-old", "Couldn't fetch artwork for 3 albums", "HTTP 502 from the server while fetching cover art.", now() - 900_000.0, true)
        jobs += Job("job-sync-old", JobKind.ArtworkPrefetch, "Artwork prefetch", JobState.Failed, 412u, 415u, 3u, now() - 1_200_000.0, false)
        library.tracks.filter { it.playCount > 0u }.take(40).forEachIndexed { i, t ->
            playHistory += PlayHistoryEntry(t.toSummary(), now() - i * 3_600_000.0 * 5, t.durationMs, true, if (i % 3 == 0) "laptop" else deviceId)
        }
        if (startWithServer) {
            servers += ServerInfo("fake-server", "https://music.example.net", "ada", "music.example.net", capabilities(), now() - 3_600_000.0, true)
            if (startPlaying) {
                val album = library.albums[5]
                setContext(QueueContext(serverId, ContextKind.Album(ContextKindAlbumInner(album.id)), album.name, SortOrder.Default), library.albumTracks(album.id).map { it.id }, 2, false)
                // Some history, one insertion and an autoplay tail so the queue timeline has every kind of row.
                insertions += QueueItem(newKey(), library.tracks[300].id, QueueSource.Inserted, false)
                stamp = PositionStamp(42_000u, now(), 1.0, true)
                scheduleAdvance()
                savedQueues += savedQueueFor(library.playlists[1], pinned = true)
                savedQueues += savedQueueFor(library.playlists[3], pinned = false)
                resumeOffer = ResumeOffer("Laptop", library.tracks[77].toSummary(), 93_000u, now() - 1_800_000.0)
            }
        }
    }

    // -- CoreHandle --------------------------------------------------------------------------------
    override fun dispatch(command: Command) {
        scope.launch { handle(command) }
    }

    override suspend fun query(query: Query): QueryResult = withContext(dispatcher) { answer(query) }

    override fun close() {
        scope.cancel()
    }

    /**
     * Serves [trackId]'s lyrics from an OpenSubsonic `getLyricsBySongId` answer (e.g. Navidrome's
     * `enhanced=true` document), adapted the way the real core adapts a server answer
     * ([EnhancedLyrics]); announces them with `LyricsChanged`. Returns what is now served.
     */
    fun serveServerLyrics(trackId: TrackId, openSubsonicJson: String): Lyrics? {
        val lyrics = EnhancedLyrics.adaptList(trackId, EnhancedLyrics.parseOpenSubsonic(openSubsonicJson))
        scope.launch {
            library.lyricsByTrack[trackId] = lyrics
            emit(Event.LyricsChanged(EventLyricsChangedInner(trackId, lyricsFor(trackId))))
        }
        return lyrics
    }

    /** Test helper: apply a command and wait for it. */
    suspend fun dispatchAndWait(command: Command) = withContext(dispatcher) { handle(command) }

    private fun emit(event: Event) {
        _events.tryEmit(event)
    }

    // -- snapshot ----------------------------------------------------------------------------------
    private fun snapshot() = Snapshot(
        servers = servers.toList(), session = sessionDocument(), queue = queueView(), transport = transport(), connection = connection,
        devices = devices.toList(), jobs = jobs.toList(), problems = problems.toList(), undo = undoState(), settings = settings.values.toList(),
        audio = audio, mediaSession = mediaSessionState(), resumeOffer = resumeOffer, sleepTimer = sleepTimer, network = network,
        batterySaver = batterySaver, syncProgress = syncProgress,
    )

    private fun sessionDocument() = SessionDocument(1u, "fake-session", "$serverId:ada", revision, now(), context, mode, cursor.toUInt(), current,
        history.toList(), insertions.toList(), shuffle, repeat, autoplay, transport(), savedQueues.toList(), null)

    private fun transport() = TransportState(TransportLease(transportOwner, epoch, now() + 20_000), stamp, buffering, playedMs.toUInt(), volume)

    private fun entry(item: QueueItem): QueueEntry? = library.track(item.trackId)?.let { QueueEntry(item, it.toSummary()) }

    private fun queueView(): QueueView {
        val upcoming = ArrayList<QueueEntry>()
        if (context != null) {
            val from = if (current == null) cursor else cursor + 1
            for (i in from until order.size) {
                val idx = order[i]
                entry(QueueItem("ctx-$i-${contextTracks[idx]}", contextTracks[idx], QueueSource.Context(QueueSourceContextInner(idx.toUInt())), false))?.let { upcoming += it }
                if (upcoming.size >= 200) break
            }
            if (autoplay && repeat == RepeatMode.Off && upcoming.size < 3) {
                autoplayTail().forEach { upcoming += it }
            }
        }
        return QueueView(context?.label, history.mapNotNull(::entry), current?.let(::entry), insertions.mapNotNull(::entry), upcoming,
            shuffle != null, repeat, autoplay, mode, (order.size - cursor - 1).coerceAtLeast(0).toUInt())
    }

    private var autoplayCache: List<QueueEntry>? = null
    private fun autoplayTail(): List<QueueEntry> = autoplayCache ?: (0 until 3).mapNotNull { i ->
        val seed = current?.let { library.track(it.trackId) } ?: library.tracks[0]
        val t = library.tracks[(seed.id.drop(1).toInt() * 31 + i * 97) % library.tracks.size]
        entry(QueueItem("auto-$i-${t.id}", t.id, QueueSource.Autoplay(QueueSourceAutoplayInner(AutoplayProvider.SonicSimilarity, "Similar to ${seed.title}", 0.93 - i * 0.07)), false))
    }.also { autoplayCache = it }

    private fun undoState() = UndoState(undoStack.isNotEmpty(), undoStack.lastOrNull()?.first?.label, redoStack.isNotEmpty(), redoStack.lastOrNull()?.first?.label, undoStack.asReversed().map { it.first })

    private fun mediaSessionState(): MediaSessionState {
        val t = current?.let { library.track(it.trackId) }
        return MediaSessionState(
            metadata = t?.let { MediaSessionMetadata(it.title, it.artist, it.album, it.durationMs, null, it.id, it.loved, it.rating) },
            isPlaying = stamp.isPlaying, position = stamp, shuffle = shuffle != null, repeat = repeat, volume = volume,
            actions = orderedMediaActions(), ownsTransport = transportOwner == deviceId,
        )
    }

    private fun orderedMediaActions(): List<MediaSessionAction> {
        val base = listOf(MediaSessionAction.Play, MediaSessionAction.Pause, MediaSessionAction.Next, MediaSessionAction.Previous, MediaSessionAction.Seek, MediaSessionAction.Stop)
        val custom = (actionOrders["mediaSession"]?.takeIf { it.isNotEmpty() } ?: ActionIds.MEDIA_SESSION).mapNotNull {
            when (it) { ActionIds.LOVE -> MediaSessionAction.Love; ActionIds.SHUFFLE -> MediaSessionAction.Shuffle; ActionIds.REPEAT -> MediaSessionAction.Repeat; ActionIds.RATE -> MediaSessionAction.Rate; else -> null }
        }
        return base + custom
    }

    private fun emitQueue() {
        autoplayCache = null
        revision++
        emit(Event.QueueChanged(EventQueueChangedInner(queueView())))
        emit(Event.NowPlayingChanged(EventNowPlayingChangedInner(current?.let(::entry))))
        emit(Event.SessionChanged(EventSessionChangedInner(sessionDocument())))
        emitMedia()
    }

    private fun emitTransport() {
        emit(Event.TransportChanged(EventTransportChangedInner(transport())))
        emitMedia()
    }

    private fun emitMedia() = emit(Event.MediaSession(EventMediaSessionInner(mediaSessionState())))
    private fun emitUndo() = emit(Event.UndoChanged(EventUndoChangedInner(undoState())))
    private fun emitJobs() = emit(Event.JobsChanged(EventJobsChangedInner(jobs.toList())))
    private fun emitProblems() = emit(Event.ProblemsChanged(EventProblemsChangedInner(problems.toList())))
    private fun emitSaved() = emit(Event.SavedQueuesChanged(EventSavedQueuesChangedInner(savedQueues.toList())))
    private fun emitPins() {
        emit(Event.PinsChanged(EventPinsChangedInner(pins.toList())))
        storage = storage.copy(downloadsBytes = pins.sumOf { it.bytes }, warnThresholdBytes = warnThreshold)
        emit(Event.StorageChanged(EventStorageChangedInner(storage)))
    }
    private fun emitDevices() = emit(Event.DevicesChanged(EventDevicesChangedInner(devices.toList())))
    private fun emitLibrary(ids: List<String>, table: String = "tracks") = emit(Event.LibraryChanged(EventLibraryChangedInner(serverId, listOf(table), ids)))
    private fun toast(message: String, actionLabel: String? = null, action: Command? = null, duration: Int = 5000) =
        emit(Event.Toast(EventToastInner(Toast(newId("toast"), message, actionLabel, action?.let { HocketJson.encodeCommand(it) }, duration.toUInt()))))

    // -- helpers -----------------------------------------------------------------------------------
    private fun newKey() = "k${keyCounter++}"
    private fun newId(prefix: String) = "$prefix-${idCounter++}"
    private fun currentTrack(): Track? = current?.let { library.track(it.trackId) }
    private fun positionNow(): Long {
        val base = stamp.positionMs.toLong()
        if (!stamp.isPlaying) return base
        return base + ((now() - stamp.takenAt) * stamp.rate).toLong().coerceAtLeast(0)
    }

    private fun capabilities(meetsFloor: Boolean = true) = ServerCapabilities(if (meetsFloor) "0.63.1" else "0.58.0", true,
        listOf("transcodeOffset", "formPost", "songLyrics", "sonicSimilarity"), true, true, true, true, false, true, true, meetsFloor)

    private fun pushUndo(label: String, inverse: () -> Unit) {
        undoStack += UndoEntry(newId("undo"), label, deviceId, now(), null) to inverse
        if (undoStack.size > 50) undoStack.removeAt(0)
        redoStack.clear()
        emitUndo()
    }

    private fun setContext(ctx: QueueContext, tracks: List<TrackId>, startIndex: Int, shuffled: Boolean, keepHistory: Boolean = false) {
        context = ctx.copy(tracks = tracks)
        contextTracks = tracks
        if (!keepHistory) history.clear()
        insertions.clear()
        shuffle = if (shuffled) ShuffleState(rng.nextInt(1, 1_000_000).toUInt(), startIndex.toUInt(), null) else null
        rebuildOrder(anchor = startIndex)
        cursor = if (shuffled) 0 else startIndex.coerceIn(0, (tracks.size - 1).coerceAtLeast(0))
        current = order.getOrNull(cursor)?.let { idx -> QueueItem(newKey(), tracks[idx], QueueSource.Context(QueueSourceContextInner(idx.toUInt())), false) }
        playedMs = 0
    }

    private fun rebuildOrder(anchor: Int? = null) {
        val n = contextTracks.size
        order = if (shuffle == null) (0 until n).toList() else {
            val perm = (0 until n).shuffled(Random(shuffle!!.seed.toInt())).toMutableList()
            val a = anchor ?: shuffle!!.anchor?.toInt()
            if (a != null && a in 0 until n) { perm.remove(a); perm.add(0, a) }
            perm
        }
    }

    private fun startPlayback(positionMs: Long = 0, play: Boolean = true) {
        stamp = PositionStamp(positionMs.toUInt(), now(), 1.0, play)
        buffering = false
        scheduleAdvance()
    }

    private fun scheduleAdvance() {
        advanceJob?.cancel()
        if (!timers || !stamp.isPlaying) return
        val t = currentTrack() ?: return
        val remaining = (t.durationMs.toLong() - positionNow()).coerceAtLeast(50)
        advanceJob = scope.launch {
            delay(remaining)
            recordPlay()
            if (repeat == RepeatMode.One) startPlayback(0) else advance(natural = true)
            emitQueue(); emitTransport()
        }
    }

    private fun recordPlay() {
        val t = currentTrack() ?: return
        library.updateTrack(t.id) { it.copy(playCount = it.playCount + 1u, lastPlayed = now()) }
        playHistory.add(0, PlayHistoryEntry(t.toSummary(), now(), t.durationMs, true, deviceId))
    }

    /** Next: current goes to history, the next item (insertions first in Apple mode) becomes current. */
    private fun advance(natural: Boolean) {
        val wasPlaying = stamp.isPlaying || natural
        current?.let { history += it; if (history.size > 200) history.removeAt(0) }
        val next: QueueItem? = when {
            insertions.isNotEmpty() -> insertions.removeAt(0)
            cursor + 1 < order.size -> { cursor++; contextItem(cursor) }
            repeat == RepeatMode.All && order.isNotEmpty() -> { cursor = 0; contextItem(0) }
            autoplay -> autoplayTail().firstOrNull()?.item?.copy(key = newKey())?.also { item ->
                // Autoplay extends the context as an ad-hoc tail so the timeline keeps flowing.
                contextTracks = contextTracks + item.trackId
                order = order + (contextTracks.size - 1)
                cursor = order.size - 1
            }
            else -> null
        }
        current = next
        playedMs = 0
        if (next == null) { stamp = PositionStamp(0u, now(), 1.0, false); advanceJob?.cancel() } else startPlayback(0, wasPlaying)
    }

    private fun contextItem(pos: Int): QueueItem? = order.getOrNull(pos)?.let { idx -> QueueItem(newKey(), contextTracks[idx], QueueSource.Context(QueueSourceContextInner(idx.toUInt())), false) }

    private fun previous() {
        if (positionNow() > 3_000 && history.isEmpty()) { startPlayback(0, stamp.isPlaying); return }
        val prev = history.removeLastOrNull() ?: run { startPlayback(0, stamp.isPlaying); return }
        current?.let { cur ->
            when (cur.source) {
                is QueueSource.Inserted -> insertions.add(0, cur)
                is QueueSource.Context -> if (cursor > 0 && (prev.source is QueueSource.Context)) cursor--
                is QueueSource.Autoplay -> Unit
            }
        }
        current = prev
        startPlayback(0, stamp.isPlaying)
    }

    private fun savedQueueFor(p: Playlist, pinned: Boolean): SavedQueue {
        val ctx = QueueContext(serverId, ContextKind.Playlist(ContextKindPlaylistInner(p.id)), p.name, SortOrder.Default)
        return SavedQueue(newId("sq"), ctx, p.name, 3u, null, emptyList(), emptyList(), null, RepeatMode.Off, 12_000u, pinned, now() - 86_400_000.0, now() - 3_600_000.0, now(), p.songCount, p.coverArt)
    }

    private fun saveOutgoingQueue() {
        val ctx = context ?: return
        if (contextTracks.size < 2 || (history.isEmpty() && playedMs == 0L && positionNow() < 5_000)) return
        val key = ctx.kind
        savedQueues.removeAll { it.context.kind == key && !it.pinned }
        val summaryCtx = if (ctx.kind is ContextKind.AdHoc) ctx else ctx.copy(tracks = emptyList())
        savedQueues.add(0, SavedQueue(newId("sq"), summaryCtx, ctx.label, cursor.toUInt(), current, history.toList(), insertions.toList(), shuffle, repeat,
            positionNow().toUInt(), false, now(), now(), now(), contextTracks.size.toUInt(), currentTrack()?.coverArt))
        val unpinned = savedQueues.filter { !it.pinned }
        if (unpinned.size > savedQueueCap) unpinned.sortedBy { it.lastInteractedAt }.take(unpinned.size - savedQueueCap).forEach { savedQueues.remove(it) }
        emitSaved()
    }

    private fun resolveContext(ctx: QueueContext): List<TrackId> = when (val k = ctx.kind) {
        is ContextKind.Album -> library.albumTracks(k.data.id).map { it.id }
        is ContextKind.Artist -> library.artistAlbums(k.data.id).flatMap { library.albumTracks(it.id) }.map { it.id }
        is ContextKind.Playlist -> library.playlistTracks[k.data.id]?.toList() ?: emptyList()
        is ContextKind.Genre -> library.sorted(library.genreTracks(k.data.name), ctx.sort, false).map { it.id }
        is ContextKind.Filter -> evaluateFilter(k.data.filter).map { it.id }
        is ContextKind.AdHoc -> ctx.tracks ?: emptyList()
        ContextKind.Autoplay -> emptyList()
    }

    // -- filters -----------------------------------------------------------------------------------
    private val localOnly = setOf(FilterField.Downloaded, FilterField.Cached, FilterField.LocalPlayCount, FilterField.LocalLastPlayed, FilterField.InPlaylist)

    private fun fields(node: FilterNode): List<FilterField> = when (node) {
        is FilterNode.Rule -> listOf(node.data.field)
        is FilterNode.All -> node.data.flatMap(::fields)
        is FilterNode.Any -> node.data.flatMap(::fields)
    }

    private fun evaluateFilter(f: Filter): List<Track> {
        val matched = library.tracks.filter { matches(it, f.root) }
        val sorted = library.sorted(matched, f.sort, f.descending)
        return f.limit?.let { sorted.take(it.toInt()) } ?: sorted
    }

    private fun matches(t: Track, node: FilterNode): Boolean = when (node) {
        is FilterNode.All -> node.data.all { matches(t, it) }
        is FilterNode.Any -> node.data.any { matches(t, it) }
        is FilterNode.Rule -> ruleMatches(t, node.data)
    }

    private fun ruleMatches(t: Track, r: FilterRule): Boolean {
        val text: String? = when (r.field) {
            FilterField.Title -> t.title; FilterField.Album -> t.album; FilterField.Artist -> t.artist; FilterField.AlbumArtist -> t.albumArtist
            FilterField.Genre -> t.genre; FilterField.FileType -> t.suffix; FilterField.Comment -> t.comment; FilterField.Key -> t.sonic?.key
            FilterField.Mood -> t.sonic?.mood; FilterField.FilePath -> t.path; else -> null
        }
        val number: Double? = when (r.field) {
            FilterField.Year -> t.year?.toDouble(); FilterField.PlayCount, FilterField.LocalPlayCount -> t.playCount.toDouble(); FilterField.Rating -> t.rating.toDouble()
            FilterField.Duration -> t.durationMs.toDouble() / 1000; FilterField.BitRate -> t.bitRate?.toDouble(); FilterField.DiscNumber -> t.discNumber?.toDouble()
            FilterField.TrackNumber -> t.trackNumber?.toDouble(); FilterField.Bpm -> t.sonic?.bpm; FilterField.Energy -> t.sonic?.energy; else -> null
        }
        val bool: Boolean? = when (r.field) {
            FilterField.Loved -> t.loved; FilterField.HasCoverArt -> t.coverArt != null; FilterField.Compilation -> false
            FilterField.Downloaded -> t.offline == OfflineState.Downloaded; FilterField.Cached -> t.offline == OfflineState.Cached
            FilterField.Lyrics -> library.lyricsByTrack[t.id] != null; else -> null
        }
        val date: Double? = when (r.field) {
            FilterField.DateAdded, FilterField.DateModified -> t.created; FilterField.LastPlayed, FilterField.LocalLastPlayed -> t.lastPlayed; else -> null
        }
        val v = r.value
        return when (r.op) {
            FilterOp.Is -> when (v) { is FilterValue.Text -> text.equals(v.data, true); is FilterValue.Number -> number == v.data; is FilterValue.List -> text != null && v.data.any { it.equals(text, true) }; else -> false }
            FilterOp.IsNot -> !ruleMatches(t, r.copy(op = FilterOp.Is))
            FilterOp.Contains -> v is FilterValue.Text && text?.contains(v.data, true) == true
            FilterOp.NotContains -> v is FilterValue.Text && text?.contains(v.data, true) != true
            FilterOp.StartsWith -> v is FilterValue.Text && text?.startsWith(v.data, true) == true
            FilterOp.EndsWith -> v is FilterValue.Text && text?.endsWith(v.data, true) == true
            FilterOp.Gt -> v is FilterValue.Number && number != null && number > v.data
            FilterOp.Lt -> v is FilterValue.Number && number != null && number < v.data
            FilterOp.InTheRange -> v is FilterValue.Range && number != null && number >= v.data.low && number <= v.data.high
            FilterOp.Before -> v is FilterValue.Date && date != null && date < parseDate(v.data)
            FilterOp.After -> v is FilterValue.Date && date != null && date > parseDate(v.data)
            FilterOp.InTheLast -> v is FilterValue.Days && date != null && date > now() - v.data.toDouble() * 86_400_000
            FilterOp.NotInTheLast -> v is FilterValue.Days && (date == null || date <= now() - v.data.toDouble() * 86_400_000)
            FilterOp.IsTrue -> bool == true
            FilterOp.IsFalse -> bool == false
        }
    }

    private fun parseDate(s: String): Double = runCatching {
        val (y, m, d) = s.split("-").map { it.toInt() }
        // Days since epoch, good enough for a fake.
        ((y - 1970) * 365.25 + (m - 1) * 30.44 + d) * 86_400_000.0
    }.getOrDefault(0.0)

    private fun nsp(f: Filter): String {
        fun node(n: FilterNode): String = when (n) {
            is FilterNode.All -> "{\"all\":[${n.data.joinToString(",", transform = ::node)}]}"
            is FilterNode.Any -> "{\"any\":[${n.data.joinToString(",", transform = ::node)}]}"
            is FilterNode.Rule -> "{\"${n.data.op.string}\":{\"${n.data.field.string}\":${value(n.data.value)}}}"
        }
        return "{\"name\":\"${f.name}\",${node(f.root).drop(1).dropLast(1)},\"sort\":\"${f.sort.string}\",\"order\":\"${if (f.descending) "desc" else "asc"}\"${f.limit?.let { ",\"limit\":$it" } ?: ""}}"
    }

    private fun value(v: FilterValue): String = when (v) {
        is FilterValue.Text -> "\"${v.data}\""; is FilterValue.Number -> v.data.toString(); is FilterValue.Range -> "[${v.data.low},${v.data.high}]"
        is FilterValue.Date -> "\"${v.data}\""; is FilterValue.Bool -> v.data.toString(); is FilterValue.Days -> v.data.toString(); is FilterValue.List -> v.data.joinToString(",", "[", "]") { "\"$it\"" }
    }

    // -- commands ----------------------------------------------------------------------------------
    private fun handle(c: Command) {
        when (c) {
            Command.Start, Command.RequestSnapshot -> {
                // Like the real core: `Started` exactly once per instance, `Snapshot` for every later
                // re-emit (RequestSnapshot, or a repeated Start), so one-time work keyed on `Started`
                // (the credential replay) never runs twice.
                if (c == Command.Start && !started) {
                    started = true
                    emit(Event.Started(EventStartedInner(snapshot())))
                } else {
                    emit(Event.Snapshot(EventSnapshotInner(snapshot())))
                }
                emit(Event.SavedQueuesChanged(EventSavedQueuesChangedInner(savedQueues.toList())))
                emit(Event.PinsChanged(EventPinsChangedInner(pins.toList())))
                emit(Event.StorageChanged(EventStorageChangedInner(storage)))
                emit(Event.FiltersChanged(EventFiltersChangedInner(filters.toList())))
                emit(Event.PlayerNotice(EventPlayerNoticeInner(playerNotice)))
                emit(Event.HandoffPickerChanged(EventHandoffPickerChangedInner(pickerOpen, devices.filter { !it.isSelf })))
            }
            Command.Shutdown -> advanceJob?.cancel()
            is Command.SetNetworkState -> { network = c.data.state }
            is Command.SetVisibility -> Unit
            is Command.SetBatterySaver -> { batterySaver = c.data.enabled; emit(Event.Snapshot(EventSnapshotInner(snapshot()))) }

            is Command.AddServer -> addServer(c.data)
            is Command.RemoveServer -> { servers.clear(); emit(Event.ServersChanged(EventServersChangedInner(emptyList()))) }
            is Command.ProbeServer -> emit(Event.ServersChanged(EventServersChangedInner(servers.toList())))
            is Command.SyncLibrary -> startSync(c.data.full)
            is Command.SetTranscodingProfile -> {
                // Like the core: one JSON map keyed by network id, "default" when none.
                val map = settings[SettingKeys.TRANSCODING_PROFILES]?.value?.let { runCatching { HocketJson.json.decodeFromString(MapSerializer(String.serializer(), TranscodingProfile.serializer()), it) }.getOrNull() }?.toMutableMap() ?: mutableMapOf()
                map[c.data.network_id ?: "default"] = c.data.profile
                setSetting(SettingKeys.TRANSCODING_PROFILES, HocketJson.json.encodeToString(MapSerializer(String.serializer(), TranscodingProfile.serializer()), map))
            }

            Command.Play -> if (current != null && !stamp.isPlaying) { startPlayback(positionNow(), true); emitTransport() }
            Command.Pause -> if (stamp.isPlaying) { playedMs += now().toLong() - stamp.takenAt.toLong(); startPlayback(positionNow(), false); emitTransport() }
            Command.TogglePlay -> handle(if (stamp.isPlaying) Command.Pause else Command.Play)
            Command.Stop -> { stamp = PositionStamp(0u, now(), 1.0, false); advanceJob?.cancel(); emitTransport() }
            Command.Next -> { advance(natural = false); emitQueue(); emitTransport() }
            Command.Previous -> { previous(); emitQueue(); emitTransport() }
            is Command.SeekTo -> { startPlayback(c.data.position_ms.toLong(), stamp.isPlaying); emitTransport() }
            is Command.SeekBy -> { startPlayback((positionNow() + c.data.delta_ms).coerceAtLeast(0), stamp.isPlaying); emitTransport() }
            is Command.SetVolume -> { volume = c.data.volume.coerceIn(0.0, 1.0); emitTransport() }

            is Command.PlayContext -> {
                val a = c.data.args
                val before = captureSession()
                if (a.saveOutgoing != false) saveOutgoingQueue()
                val tracks = resolveContext(a.context)
                setContext(a.context, tracks, a.startIndex?.toInt() ?: 0, a.shuffle)
                startPlayback(0, true)
                pushUndo("Play ${a.context.label}") { restoreSession(before) }
                emitQueue(); emitTransport()
            }
            is Command.PlayTracks -> {
                val before = captureSession()
                saveOutgoingQueue()
                setContext(QueueContext(serverId, ContextKind.AdHoc(ContextKindAdHocInner(c.data.label)), c.data.label, SortOrder.Default, c.data.track_ids), c.data.track_ids, c.data.start_index.toInt(), c.data.shuffle)
                startPlayback(0, true)
                pushUndo("Play ${c.data.label}") { restoreSession(before) }
                emitQueue(); emitTransport()
            }
            is Command.PlayNext -> {
                val before = captureSession()
                c.data.track_ids.reversed().forEach { insertions.add(0, QueueItem(newKey(), it, QueueSource.Inserted, false)) }
                if (current == null) { advance(false) }
                pushUndo("Play next") { restoreSession(before) }
                toast(if (c.data.track_ids.size == 1) "Playing next" else "${c.data.track_ids.size} tracks playing next", "Undo", Command.Undo)
                emitQueue()
            }
            is Command.PlayLater -> {
                val before = captureSession()
                c.data.track_ids.forEach { insertions += QueueItem(newKey(), it, QueueSource.Inserted, false) }
                if (current == null) { advance(false) }
                pushUndo("Play later") { restoreSession(before) }
                toast(if (c.data.track_ids.size == 1) "Added to queue" else "${c.data.track_ids.size} tracks added to queue", "Undo", Command.Undo)
                emitQueue()
            }
            is Command.JumpToQueueItem -> jumpTo(c.data.key)
            is Command.RemoveQueueItems -> {
                val before = captureSession()
                val keys = c.data.keys.toSet()
                insertions.removeAll { it.key in keys }
                history.removeAll { it.key in keys }
                val ctxRemovals = queueView().upcoming.filter { it.item.key in keys && it.item.source is QueueSource.Context }.map { (it.item.source as QueueSource.Context).data.index.toInt() }.toSet()
                if (ctxRemovals.isNotEmpty()) {
                    val newOrder = order.filter { it !in ctxRemovals }
                    val keep = contextTracks.indices.filter { it !in ctxRemovals }
                    val remap = keep.withIndex().associate { (n, old) -> old to n }
                    val curIdx = order.getOrNull(cursor)
                    contextTracks = keep.map { contextTracks[it] }
                    order = newOrder.map { remap[it]!! }
                    cursor = curIdx?.let { remap[it] }?.let { order.indexOf(it) } ?: cursor.coerceAtMost((order.size - 1).coerceAtLeast(0))
                    if (shuffle != null) shuffle = shuffle!!.copy(order = order.map { it.toUInt() })
                }
                if (current?.key in keys) advance(false)
                pushUndo(if (keys.size == 1) "Remove from queue" else "Remove ${keys.size} from queue") { restoreSession(before) }
                toast(if (keys.size == 1) "Removed from queue" else "Removed ${keys.size} tracks", "Undo", Command.Undo)
                emitQueue(); emitTransport()
            }
            is Command.MoveQueueItem -> moveItem(c.data.key, c.data.to_index.toInt())
            Command.ClearQueue -> { val before = captureSession(); insertions.clear(); order = order.take(cursor + 1); pushUndo("Clear queue") { restoreSession(before) }; toast("Queue cleared", "Undo", Command.Undo); emitQueue() }
            Command.ClearInsertions -> { val before = captureSession(); insertions.clear(); pushUndo("Clear playing next") { restoreSession(before) }; emitQueue() }
            is Command.SetShuffle -> {
                val before = captureSession()
                val curIdx = order.getOrNull(cursor)
                shuffle = if (c.data.enabled) ShuffleState(rng.nextInt(1, 1_000_000).toUInt(), curIdx?.toUInt(), null) else null
                rebuildOrder(curIdx)
                cursor = curIdx?.let { order.indexOf(it) }?.takeIf { it >= 0 } ?: 0
                pushUndo(if (c.data.enabled) "Shuffle on" else "Shuffle off") { restoreSession(before) }
                emitQueue()
            }
            is Command.SetRepeat -> { repeat = c.data.mode; emitQueue() }
            is Command.SetAutoplay -> { autoplay = c.data.enabled; emitQueue() }
            is Command.SetQueueMode -> { mode = c.data.mode; emitQueue() }
            is Command.SkipUnavailable -> {
                val t = currentTrack()
                playerNotice = t?.let { "Couldn't play ${it.title} here, skipped" }
                emit(Event.PlayerNotice(EventPlayerNoticeInner(playerNotice)))
                advance(false); emitQueue(); emitTransport()
            }

            is Command.RestoreSavedQueue -> restoreSaved(c.data.id)
            is Command.PinSavedQueue -> { val i = savedQueues.indexOfFirst { it.id == c.data.id }; if (i >= 0) { savedQueues[i] = savedQueues[i].copy(pinned = c.data.pinned, updatedAt = now()); emitSaved() } }
            is Command.DeleteSavedQueue -> { savedQueues.removeAll { it.id == c.data.id }; emitSaved() }
            is Command.SaveQueueAsPlaylist -> {
                val ids = c.data.saved_queue_id?.let { id -> savedQueues.firstOrNull { it.id == id }?.let { resolveContext(it.context) } } ?: (contextTracks + insertions.map { it.trackId })
                createPlaylist(c.data.name, ids)
                toast("Saved as playlist “${c.data.name}”")
            }
            is Command.SetSavedQueueCap -> { savedQueueCap = c.data.cap.toInt(); setSetting(SettingKeys.QUEUE_SAVED_CAP, savedQueueCap.toString()) }

            Command.Undo -> undo()
            Command.Redo -> redo()
            is Command.UndoEntry -> {
                val idx = undoStack.indexOfFirst { it.first.id == c.data.id }
                if (idx >= 0) { while (undoStack.size > idx) undo(emitToast = false); toast("Undone") }
            }
            Command.RestoreSelection -> toast("Selection restored")

            is Command.SetRating -> setRating(c.data.targets, c.data.rating.toInt())
            is Command.SetLoved -> setLoved(c.data.targets, c.data.loved)
            is Command.SetArtistLoved -> { library.updateArtist(c.data.artist_id) { it.copy(loved = c.data.loved) }; emitLibrary(listOf(c.data.artist_id), "artists") }
            is Command.CreatePlaylist -> { createPlaylist(c.data.name, c.data.track_ids); toast("Created “${c.data.name}”") }
            is Command.DeletePlaylist -> { library.playlists.removeAll { it.id == c.data.playlist_id }; library.playlistTracks.remove(c.data.playlist_id); emitLibrary(listOf(c.data.playlist_id), "playlists"); toast("Playlist deleted") }
            is Command.RenamePlaylist -> {
                val i = library.playlists.indexOfFirst { it.id == c.data.playlist_id }
                if (i >= 0) library.playlists[i] = library.playlists[i].copy(name = c.data.name, comment = c.data.comment ?: library.playlists[i].comment, public = c.data.public ?: library.playlists[i].public)
                emitLibrary(listOf(c.data.playlist_id), "playlists")
            }
            is Command.PlaylistAdd -> {
                val list = library.playlistTracks.getOrPut(c.data.playlist_id) { mutableListOf() }
                val prior = list.toList()
                val at = c.data.at_index?.toInt()?.coerceIn(0, list.size) ?: list.size
                list.addAll(at, c.data.track_ids)
                library.refreshPlaylist(c.data.playlist_id)
                pushUndo("Add to playlist") { list.clear(); list.addAll(prior); library.refreshPlaylist(c.data.playlist_id); emitLibrary(listOf(c.data.playlist_id), "playlists") }
                emitLibrary(listOf(c.data.playlist_id), "playlists")
                toast(if (c.data.track_ids.size == 1) "Added to ${library.playlist(c.data.playlist_id)?.name}" else "Added ${c.data.track_ids.size} tracks to ${library.playlist(c.data.playlist_id)?.name}", "Undo", Command.Undo)
            }
            is Command.PlaylistRemove -> {
                val list = library.playlistTracks[c.data.playlist_id] ?: return
                val prior = list.toList()
                c.data.indices.map { it.toInt() }.sortedDescending().forEach { if (it in list.indices) list.removeAt(it) }
                library.refreshPlaylist(c.data.playlist_id)
                pushUndo("Remove from playlist") { list.clear(); list.addAll(prior); library.refreshPlaylist(c.data.playlist_id); emitLibrary(listOf(c.data.playlist_id), "playlists") }
                emitLibrary(listOf(c.data.playlist_id), "playlists")
                toast("Removed from playlist", "Undo", Command.Undo)
            }
            is Command.PlaylistMove -> {
                val list = library.playlistTracks[c.data.playlist_id] ?: return
                val prior = list.toList()
                val from = c.data.from_index.toInt(); val to = c.data.to_index.toInt()
                if (from in list.indices && to in list.indices) { val item = list.removeAt(from); list.add(to, item) }
                pushUndo("Reorder playlist") { list.clear(); list.addAll(prior); emitLibrary(listOf(c.data.playlist_id), "playlists") }
                emitLibrary(listOf(c.data.playlist_id), "playlists")
            }
            is Command.Scrobble -> Unit

            is Command.Pin -> pin(c.data.target, c.data.transcode)
            is Command.Unpin -> { pins.removeAll { it.target == c.data.target }; emitPins(); toast("Download removed") }
            Command.ClearStreamCache -> { storage = storage.copy(cacheBytes = 0.0); emit(Event.StorageChanged(EventStorageChangedInner(storage))); toast("Stream cache cleared") }
            is Command.SetStorageWarnThreshold -> { warnThreshold = c.data.bytes; emitPins() }

            is Command.CancelJob -> updateJob(c.data.id) { it.copy(state = JobState.Cancelled) }
            is Command.RetryJob -> { updateJob(c.data.id) { it.copy(state = JobState.Running, failed = 0u) }; progressJob(c.data.id) }
            is Command.PauseJob -> updateJob(c.data.id) { it.copy(state = JobState.Paused) }
            is Command.ResumeJob -> { updateJob(c.data.id) { it.copy(state = JobState.Running) }; progressJob(c.data.id) }
            is Command.RetryProblem -> { problems.removeAll { it.id == c.data.id }; emitProblems(); toast("Retrying") }
            is Command.DismissProblem -> { problems.removeAll { it.id == c.data.id }; emitProblems() }
            Command.DismissAllProblems -> { problems.clear(); emitProblems() }

            is Command.SaveFilter -> { filters.removeAll { it.id == c.data.filter.id }; filters += c.data.filter; emit(Event.FiltersChanged(EventFiltersChangedInner(filters.toList()))); toast("Filter saved") }
            is Command.DeleteFilter -> { filters.removeAll { it.id == c.data.id }; emit(Event.FiltersChanged(EventFiltersChangedInner(filters.toList()))) }
            is Command.CreateSmartPlaylist -> { createPlaylist(c.data.name, evaluateFilter(c.data.filter).map { it.id }, smart = true); toast("Smart playlist “${c.data.name}” created") }
            is Command.CreateStaticPlaylistFromFilter -> { createPlaylist(c.data.name, evaluateFilter(c.data.filter).map { it.id }); toast("Playlist “${c.data.name}” created") }
            is Command.ExportNsp -> emit(Event.NspExported(EventNspExportedInner(c.data.filter.id, nsp(c.data.filter), c.data.path)))
            is Command.SetAutoplaySettings -> { autoplaySettings = c.data.settings }

            is Command.SetLyricsOffset -> {
                lyricsOffsets[c.data.track_id] = c.data.offset_ms
                emit(Event.LyricsChanged(EventLyricsChangedInner(c.data.track_id, lyricsFor(c.data.track_id))))
            }
            is Command.SetExternalLyricsEnabled -> { externalLyrics = c.data.enabled; setSetting(SettingKeys.LYRICS_EXTERNAL_ENABLED, c.data.enabled.toString()) }
            is Command.FetchLyrics -> emit(Event.LyricsChanged(EventLyricsChangedInner(c.data.track_id, lyricsFor(c.data.track_id))))

            is Command.SetSetting -> setSetting(c.data.key, c.data.value)
            is Command.ResetSetting -> settings[c.data.key]?.let { setSetting(c.data.key, defaults[c.data.key] ?: it.value) }
            is Command.SetSettingsSync -> setSetting(SettingKeys.SYNC_ENABLED, c.data.enabled.toString())
            is Command.ExportConfig -> emit(Event.ConfigExported(EventConfigExportedInner(configDocument(c.data.include_secrets))))
            is Command.ImportConfig -> toast("Configuration restored")
            is Command.SetAudioSettings -> { audio = c.data.settings; emit(Event.AudioSettingsChanged(EventAudioSettingsChangedInner(audio))) }
            is Command.SetOutputDevice -> { audio = audio.copy(outputDevice = c.data.id); emit(Event.AudioSettingsChanged(EventAudioSettingsChangedInner(audio))) }
            Command.RefreshOutputDevices -> emit(Event.OutputDevicesChanged(EventOutputDevicesChangedInner(listOf(OutputDevice("default", "Phone speaker", true), OutputDevice("bt", "Headphones", false)))))

            is Command.SetCoordinatorUrl -> {
                setSetting(SettingKeys.CONNECT_COORDINATOR_URL, c.data.url?.let { "\"$it\"" } ?: "null")
                connection = connection.copy(coordinatorUrl = c.data.url, tier = if (c.data.url != null) ConnectionTier.Coordinator else ConnectionTier.Lan)
                emit(Event.ConnectionChanged(EventConnectionChangedInner(connection)))
            }
            Command.ConnectCoordinator -> { connection = connection.copy(connected = true, tier = ConnectionTier.Coordinator, error = null, roundTripMs = 48.0); emit(Event.ConnectionChanged(EventConnectionChangedInner(connection))) }
            Command.DisconnectCoordinator -> { connection = connection.copy(connected = false, tier = ConnectionTier.Local, peerCount = 0u); emit(Event.ConnectionChanged(EventConnectionChangedInner(connection))) }
            is Command.SetLanDiscovery -> setSetting(SettingKeys.CONNECT_LAN_DISCOVERY, c.data.enabled.toString())
            Command.OpenHandoffPicker -> {
                pickerOpen = true
                emit(Event.HandoffPickerChanged(EventHandoffPickerChangedInner(true, devices.filter { !it.isSelf })))
                if (timers) scope.launch {
                    delay(900)
                    if (!pickerOpen) return@launch
                    for (i in devices.indices) if (!devices[i].isSelf) devices[i] = devices[i].copy(ready = true)
                    emitDevices()
                    emit(Event.HandoffPickerChanged(EventHandoffPickerChangedInner(true, devices.filter { !it.isSelf })))
                }
            }
            Command.CloseHandoffPicker -> {
                pickerOpen = false
                for (i in devices.indices) devices[i] = devices[i].copy(ready = false)
                emitDevices()
                emit(Event.HandoffPickerChanged(EventHandoffPickerChangedInner(false, emptyList())))
            }
            is Command.HandoffTo -> {
                pickerOpen = false
                transportOwner = c.data.device_id
                epoch++
                for (i in devices.indices) devices[i] = devices[i].copy(playing = devices[i].id == c.data.device_id, ready = false)
                emitDevices()
                emit(Event.HandoffPickerChanged(EventHandoffPickerChangedInner(false, emptyList())))
                emitTransport()
                toast("Now playing on ${devices.firstOrNull { it.id == c.data.device_id }?.name}")
            }
            Command.ResumeHere -> {
                val offer = resumeOffer ?: return
                val before = captureSession()
                saveOutgoingQueue()
                val album = library.track(offer.track.id)?.albumId?.let { library.album(it) }
                if (album != null) {
                    val tracks = library.albumTracks(album.id).map { it.id }
                    setContext(QueueContext(serverId, ContextKind.Album(ContextKindAlbumInner(album.id)), album.name, SortOrder.Default), tracks, tracks.indexOf(offer.track.id).coerceAtLeast(0), false)
                }
                transportOwner = deviceId; epoch++
                for (i in devices.indices) devices[i] = devices[i].copy(playing = devices[i].isSelf)
                startPlayback(offer.positionMs.toLong(), true)
                resumeOffer = null
                pushUndo("Resume here") { restoreSession(before) }
                emit(Event.ResumeOfferChanged(EventResumeOfferChangedInner(null)))
                emitDevices(); emitQueue(); emitTransport()
            }
            Command.DismissResumeOffer -> { resumeOffer = null; emit(Event.ResumeOfferChanged(EventResumeOfferChangedInner(null))) }
            is Command.SetSleepTimer -> { sleepTimer = c.data.timer; emit(Event.SleepTimerChanged(EventSleepTimerChangedInner(sleepTimer))) }

            is Command.BackendReport -> Unit
            is Command.MediaSessionCommand -> when (c.data.action) {
                MediaSessionAction.Play -> handle(Command.Play)
                MediaSessionAction.Pause -> handle(Command.Pause)
                MediaSessionAction.Next -> handle(Command.Next)
                MediaSessionAction.Previous -> handle(Command.Previous)
                MediaSessionAction.Seek -> c.data.value?.let { handle(Command.SeekTo(CommandSeekToInner(it.toLong().coerceAtLeast(0).toUInt()))) }
                MediaSessionAction.Stop -> handle(Command.Stop)
                MediaSessionAction.Shuffle -> handle(Command.SetShuffle(CommandSetShuffleInner(shuffle == null)))
                MediaSessionAction.Repeat -> handle(Command.SetRepeat(CommandSetRepeatInner(when (repeat) { RepeatMode.Off -> RepeatMode.All; RepeatMode.All -> RepeatMode.One; RepeatMode.One -> RepeatMode.Off })))
                MediaSessionAction.Love -> currentTrack()?.let { setLoved(listOf(RatingTarget.Track(RatingTargetTrackInner(it.id))), !it.loved) }
                MediaSessionAction.Rate -> currentTrack()?.let { t -> setRating(listOf(RatingTarget.Track(RatingTargetTrackInner(t.id))), c.data.value?.toInt() ?: 0) }
            }
            is Command.RunAction -> runAction(c.data.action_id, c.data.target)
            is Command.SetShortcut -> Unit
            is Command.SetActionOrder -> {
                actionOrders[c.data.surface] = c.data.action_ids
                setSetting(SettingKeys.actionOrder(c.data.surface), HocketJson.json.encodeToString(ListSerializer(String.serializer()), c.data.action_ids))
                emit(Event.ActionsChanged(EventActionsChangedInner(c.data.surface))); emitMedia()
            }
            is Command.SetSelection -> { selection = c.data.target }
            is Command.Touch -> Unit
        }
    }

    private data class SessionCapture(val context: QueueContext?, val tracks: List<TrackId>, val order: List<Int>, val cursor: Int, val current: QueueItem?, val history: List<QueueItem>, val insertions: List<QueueItem>, val shuffle: ShuffleState?, val repeat: RepeatMode, val position: Long, val playing: Boolean)

    private fun captureSession() = SessionCapture(context, contextTracks, order, cursor, current, history.toList(), insertions.toList(), shuffle, repeat, positionNow(), stamp.isPlaying)

    private fun restoreSession(s: SessionCapture) {
        context = s.context; contextTracks = s.tracks; order = s.order; cursor = s.cursor; current = s.current
        history.clear(); history += s.history; insertions.clear(); insertions += s.insertions; shuffle = s.shuffle; repeat = s.repeat
        startPlayback(s.position, s.playing)
        emitQueue(); emitTransport()
    }

    private fun undo(emitToast: Boolean = true) {
        val (entry, inverse) = undoStack.removeLastOrNull() ?: return
        val redoCapture = captureSession()
        inverse()
        redoStack += entry to { restoreSession(redoCapture) }
        emitUndo()
        if (emitToast) toast("Undid: ${entry.label}", "Redo", Command.Redo)
    }

    private fun redo() {
        val (entry, apply) = redoStack.removeLastOrNull() ?: return
        val undoCapture = captureSession()
        apply()
        undoStack += entry to { restoreSession(undoCapture) }
        emitUndo()
    }

    private fun jumpTo(key: QueueKey) {
        val before = captureSession()
        val view = queueView()
        view.history.indexOfFirst { it.item.key == key }.takeIf { it >= 0 }?.let { i ->
            // Everything after the target in history goes back to the front of the future.
            val target = history[i]
            val after = history.subList(i + 1, history.size).toList()
            current?.let { after + it }
            repeat(history.size - i) { history.removeAt(history.size - 1) }
            current?.let { cur -> if (cur.source is QueueSource.Inserted) insertions.add(0, cur) }
            after.reversed().forEach { if (it.source is QueueSource.Inserted) insertions.add(0, it) }
            current = target
            (target.source as? QueueSource.Context)?.let { cursor = order.indexOf(it.data.index.toInt()).coerceAtLeast(0) }
            startPlayback(0, true)
            pushUndo("Jump back") { restoreSession(before) }
            emitQueue(); emitTransport(); return
        }
        insertions.indexOfFirst { it.key == key }.takeIf { it >= 0 }?.let { i ->
            current?.let { history += it }
            val target = insertions.removeAt(i)
            // Skipped insertions before the target are played through: they go to history.
            repeat(i) { history += insertions.removeAt(0) }
            current = target
            startPlayback(0, true)
            pushUndo("Jump in queue") { restoreSession(before) }
            emitQueue(); emitTransport(); return
        }
        view.upcoming.firstOrNull { it.item.key == key }?.let { e ->
            when (val src = e.item.source) {
                is QueueSource.Context -> {
                    current?.let { history += it }
                    val pos = order.indexOf(src.data.index.toInt())
                    for (p in cursor + 1 until pos) contextItem(p)?.let { history += it }
                    cursor = pos
                    current = contextItem(pos)
                }
                is QueueSource.Autoplay -> { current?.let { history += it }; current = e.item.copy(key = newKey()) }
                QueueSource.Inserted -> Unit
            }
            startPlayback(0, true)
            pushUndo("Jump in queue") { restoreSession(before) }
            emitQueue(); emitTransport()
        }
    }

    private fun moveItem(key: QueueKey, toIndex: Int) {
        val before = captureSession()
        val combined = insertions.map { it.key } + queueView().upcoming.map { it.item.key }
        val from = combined.indexOf(key)
        if (from < 0) return
        val insIdx = insertions.indexOfFirst { it.key == key }
        if (insIdx >= 0 && toIndex < insertions.size) {
            val item = insertions.removeAt(insIdx); insertions.add(toIndex.coerceIn(0, insertions.size), item)
        } else if (insIdx >= 0) {
            // Insertion moved into the context tail: splice it into the order after the cursor.
            val item = insertions.removeAt(insIdx)
            contextTracks = contextTracks + item.trackId
            val newIdx = contextTracks.size - 1
            val pos = (cursor + 1 + (toIndex - insertions.size)).coerceIn(cursor + 1, order.size)
            order = order.toMutableList().also { it.add(pos, newIdx) }
            if (shuffle != null) shuffle = shuffle!!.copy(order = order.map { it.toUInt() })
        } else {
            val upIdx = from - insertions.size
            val o = order.toMutableList()
            val srcPos = cursor + 1 + upIdx
            if (srcPos !in o.indices) return
            val idx = o.removeAt(srcPos)
            if (toIndex < insertions.size) {
                insertions.add(toIndex, QueueItem(newKey(), contextTracks[idx], QueueSource.Inserted, false))
            } else {
                o.add((cursor + 1 + (toIndex - insertions.size)).coerceIn(cursor + 1, o.size), idx)
            }
            order = o
            if (shuffle != null) shuffle = shuffle!!.copy(order = order.map { it.toUInt() })
        }
        pushUndo("Reorder queue") { restoreSession(before) }
        emitQueue()
    }

    private fun restoreSaved(id: String) {
        val sq = savedQueues.firstOrNull { it.id == id } ?: return
        val before = captureSession()
        saveOutgoingQueue()
        val tracks = resolveContext(sq.context)
        setContext(sq.context, tracks, sq.cursor.toInt().coerceIn(0, (tracks.size - 1).coerceAtLeast(0)), sq.shuffle != null)
        history.clear(); history += sq.history
        insertions.clear(); insertions += sq.insertions
        sq.current?.let { current = it }
        repeat = sq.repeat
        startPlayback(sq.positionMs.toLong(), true)
        val i = savedQueues.indexOf(sq)
        savedQueues[i] = sq.copy(lastInteractedAt = now())
        pushUndo("Restore ${sq.label}") { restoreSession(before) }
        emitSaved(); emitQueue(); emitTransport()
    }

    private fun setRating(targets: List<RatingTarget>, rating: Int) {
        val prior = HashMap<RatingTarget, Int>()
        for (t in targets) when (t) {
            is RatingTarget.Track -> library.track(t.data.id)?.let { prior[t] = it.rating.toInt(); library.updateTrack(t.data.id) { tr -> tr.copy(rating = rating.toUInt()) } }
            is RatingTarget.Album -> library.album(t.data.id)?.let { prior[t] = it.rating.toInt(); library.updateAlbum(t.data.id) { al -> al.copy(rating = rating.toUInt()) } }
        }
        val label = if (targets.size == 1) "Rate ${rating}★" else "Rate ${targets.size} items"
        pushUndo(label) {
            var changed = 0
            prior.forEach { (t, r) -> when (t) { is RatingTarget.Track -> library.updateTrack(t.data.id) { tr -> if (tr.rating.toInt() == rating) tr.copy(rating = r.toUInt()) else { changed++; tr } }; is RatingTarget.Album -> library.updateAlbum(t.data.id) { al -> al.copy(rating = r.toUInt()) } } }
            emitLibrary(prior.keys.map { idOf(it) }); emitQueue()
        }
        if (targets.size > 20) startJob(JobKind.BulkRating, "Rating ${targets.size} tracks", targets.size)
        emitLibrary(targets.map { idOf(it) }); emitQueue()
        if (targets.size > 1) toast("Rated ${targets.size} items", "Undo", Command.Undo)
    }

    private fun setLoved(targets: List<RatingTarget>, loved: Boolean) {
        for (t in targets) when (t) {
            is RatingTarget.Track -> library.updateTrack(t.data.id) { it.copy(loved = loved) }
            is RatingTarget.Album -> library.updateAlbum(t.data.id) { it.copy(loved = loved) }
        }
        pushUndo(if (loved) "Love" else "Unlove") {
            for (t in targets) when (t) {
                is RatingTarget.Track -> library.updateTrack(t.data.id) { it.copy(loved = !loved) }
                is RatingTarget.Album -> library.updateAlbum(t.data.id) { it.copy(loved = !loved) }
            }
            emitLibrary(targets.map { idOf(it) }); emitQueue()
        }
        if (targets.size > 20) startJob(JobKind.BulkLove, "${if (loved) "Loving" else "Unloving"} ${targets.size} tracks", targets.size)
        emitLibrary(targets.map { idOf(it) }); emitQueue()
    }

    private fun idOf(t: RatingTarget) = when (t) { is RatingTarget.Track -> t.data.id; is RatingTarget.Album -> t.data.id }

    private fun createPlaylist(name: String, ids: List<TrackId>, smart: Boolean = false) {
        val id = newId("pl")
        library.playlistTracks[id] = ids.toMutableList()
        library.playlists += Playlist(id, serverId, name, null, "ada", false, ids.size.toUInt(), ids.sumOf { library.track(it)?.durationMs?.toLong() ?: 0L }.toUInt(), ids.firstOrNull()?.let { library.track(it)?.coverArt }, now(), now(), smart, true, OfflineState.None)
        emitLibrary(listOf(id), "playlists")
    }

    private fun pin(target: PinTarget, transcode: Boolean) {
        val (label, cover, count, bytes) = when (target) {
            is PinTarget.Track -> library.track(target.data.id)?.let { Quad(it.title, it.coverArt, 1, it.sizeBytes ?: 8e6) } ?: return
            is PinTarget.Album -> library.album(target.data.id)?.let { a -> Quad(a.name, a.coverArt, a.songCount.toInt(), library.albumTracks(a.id).sumOf { it.sizeBytes ?: 8e6 }) } ?: return
            is PinTarget.Playlist -> library.playlist(target.data.id)?.let { p -> Quad(p.name, p.coverArt, p.songCount.toInt(), (library.playlistTracks[p.id] ?: emptyList()).sumOf { library.track(it)?.sizeBytes ?: 8e6 }) } ?: return
        }
        if (pins.any { it.target == target }) return
        pins += Pin(target, label, cover, count.toUInt(), 0u, bytes, now(), transcode)
        emitPins()
        val jobId = startJob(JobKind.Download, "Downloading $label", count)
        toast("Downloading $label")
        if (timers) scope.launch {
            var done = 0
            while (done < count) {
                delay(400)
                done++
                val i = pins.indexOfFirst { it.target == target }
                if (i < 0) { updateJob(jobId) { it.copy(state = JobState.Cancelled) }; return@launch }
                pins[i] = pins[i].copy(downloadedCount = done.toUInt())
                emitPins()
            }
        }
    }

    private data class Quad(val label: String, val cover: String?, val count: Int, val bytes: Double)

    private fun startJob(kind: JobKind, label: String, total: Int): JobId {
        val id = newId("job")
        jobs.add(0, Job(id, kind, label, JobState.Running, 0u, total.toUInt(), 0u, now(), true))
        emitJobs()
        progressJob(id)
        return id
    }

    private fun updateJob(id: JobId, f: (Job) -> Job) {
        val i = jobs.indexOfFirst { it.id == id }
        if (i >= 0) { jobs[i] = f(jobs[i]); emitJobs() }
    }

    private fun progressJob(id: JobId) {
        if (!timers) return
        scope.launch {
            while (true) {
                delay(350)
                val i = jobs.indexOfFirst { it.id == id }
                if (i < 0) return@launch
                val j = jobs[i]
                if (j.state != JobState.Running) return@launch
                val total = j.total ?: 100u
                val step = (total.toInt() / 12).coerceAtLeast(1).toUInt()
                val done = (j.done + step).coerceAtMost(total)
                jobs[i] = if (done >= total) j.copy(done = total, state = if (j.failed > 0u) JobState.Failed else JobState.Done) else j.copy(done = done)
                emitJobs()
                if (jobs[i].kind == JobKind.LibrarySync) {
                    syncProgress = SyncProgress(serverId, if (done < total / 2u) "tracks" else "albums", done, total, if (done > total / 3u) listOf("artists", "albums") else listOf("artists"), done >= total)
                    emit(Event.SyncProgress(EventSyncProgressInner(syncProgress!!)))
                    if (done >= total) { servers[0] = servers[0].copy(lastSync = now()); emit(Event.ServersChanged(EventServersChangedInner(servers.toList()))); emitLibrary(emptyList()) }
                }
                if (done >= total) return@launch
            }
        }
    }

    private fun addServer(a: CommandAddServerInner) {
        when {
            a.password == "wrong" -> emit(Event.Error(EventErrorInner(ErrorKind.Auth, "Wrong username or password", "The server refused the credentials.")))
            a.url.contains("old") -> {
                servers.clear(); servers += ServerInfo("fake-server", a.url, a.username, a.name ?: a.url.removePrefix("https://"), capabilities(false), null, true)
                emit(Event.ServersChanged(EventServersChangedInner(servers.toList())))
                emit(Event.Error(EventErrorInner(ErrorKind.Server, "Navidrome 0.58.0 is too old", "Hocket needs Navidrome 0.63.0 or newer for synced lyrics and fast library sync.")))
            }
            a.url.contains("down") -> emit(Event.Error(EventErrorInner(ErrorKind.Network, "Couldn't reach ${a.url}", null)))
            else -> {
                servers.clear(); servers += ServerInfo("fake-server", a.url, a.username, a.name ?: a.url.removePrefix("https://").removePrefix("http://"), capabilities(), null, true)
                emit(Event.ServersChanged(EventServersChangedInner(servers.toList())))
                startSync(true)
            }
        }
    }

    private fun startSync(full: Boolean) {
        val total = library.tracks.size
        val id = startJob(JobKind.LibrarySync, if (full) "Full library sync" else "Library refresh", total)
        syncProgress = SyncProgress(serverId, "artists", 0u, total.toUInt(), emptyList(), false)
        emit(Event.SyncProgress(EventSyncProgressInner(syncProgress!!)))
        if (!timers) { updateJob(id) { it.copy(state = JobState.Done, done = total.toUInt()) }; syncProgress = syncProgress!!.copy(done = total.toUInt(), finished = true, readyTables = listOf("artists", "albums", "tracks")); emit(Event.SyncProgress(EventSyncProgressInner(syncProgress!!))) }
    }

    /** Like the core: unknown keys are refused (an Error event), known ones keep their registry scope. */
    private fun setSetting(key: String, value: String) {
        val existing = settings[key]
        if (existing == null) {
            emit(Event.Error(EventErrorInner(ErrorKind.Internal, "unknown setting '$key'", null)))
            return
        }
        settings[key] = Setting(key, value, existing.scope, now())
        emit(Event.SettingChanged(EventSettingChangedInner(settings[key]!!)))
        if (key == SettingKeys.QUEUE_SAVED_CAP) value.toIntOrNull()?.let { savedQueueCap = it }
    }

    private fun lyricsFor(id: TrackId): Lyrics? = library.lyricsByTrack[id]?.let { it.copy(offsetMs = lyricsOffsets[id] ?: 0) }

    private fun configDocument(secrets: Boolean) = HocketJson.json.encodeToString(ConfigDocument.serializer(), ConfigDocument(1u, now(), settings.values.toList(), filters.toList(), emptyList(), servers.toList(),
        if (secrets) hashMapOf("fake-server.password" to "••••") else null, audio, autoplaySettings, null))

    private fun runAction(actionId: String, target: ActionTarget) {
        val id = canonical(actionId)
        if (id in ActionIds.UI_HANDLED) return // the platform performs these; the core yields no command
        val trackIds: List<TrackId> = when (target) {
            is ActionTarget.Tracks -> target.data.ids
            is ActionTarget.Albums -> target.data.ids.flatMap { library.albumTracks(it) }.map { it.id }
            is ActionTarget.Artists -> target.data.ids.flatMap { a -> library.artistAlbums(a).flatMap { library.albumTracks(it.id) } }.map { it.id }
            is ActionTarget.Playlists -> target.data.ids.flatMap { library.playlistTracks[it] ?: emptyList() }
            is ActionTarget.QueueItems -> target.data.keys.mapNotNull { k -> (history + listOfNotNull(current) + insertions + queueView().upcoming.map { it.item }).firstOrNull { it.key == k }?.trackId }
            is ActionTarget.SavedQueue -> emptyList()
            ActionTarget.None -> emptyList()
        }
        val label = when (target) { is ActionTarget.Albums -> library.album(target.data.ids.first())?.name; is ActionTarget.Artists -> library.artist(target.data.ids.first())?.name; is ActionTarget.Playlists -> library.playlist(target.data.ids.first())?.name; else -> null } ?: "${trackIds.size} tracks"
        val rateMatch = Regex("rate([0-5])").matchEntire(id)
        when {
            rateMatch != null -> setRating(trackIds.map { RatingTarget.Track(RatingTargetTrackInner(it)) }, rateMatch.groupValues[1].toInt())
            id == ActionIds.PLAY -> handle(Command.PlayTracks(CommandPlayTracksInner(serverId, trackIds, 0u, label, false)))
            id == ActionIds.PLAY_SHUFFLED -> handle(Command.PlayTracks(CommandPlayTracksInner(serverId, trackIds, 0u, label, true)))
            id == ActionIds.PLAY_NEXT -> handle(Command.PlayNext(CommandPlayNextInner(serverId, trackIds)))
            id == ActionIds.PLAY_LATER -> handle(Command.PlayLater(CommandPlayLaterInner(serverId, trackIds)))
            id == ActionIds.LOVE -> setLoved(trackIds.map { RatingTarget.Track(RatingTargetTrackInner(it)) }, true)
            id == ActionIds.UNLOVE -> setLoved(trackIds.map { RatingTarget.Track(RatingTargetTrackInner(it)) }, false)
            id == ActionIds.DOWNLOAD -> when (target) {
                is ActionTarget.Albums -> target.data.ids.forEach { pin(PinTarget.Album(PinTargetAlbumInner(it)), false) }
                is ActionTarget.Playlists -> target.data.ids.forEach { pin(PinTarget.Playlist(PinTargetPlaylistInner(it)), false) }
                else -> trackIds.forEach { pin(PinTarget.Track(PinTargetTrackInner(it)), false) }
            }
            id == ActionIds.UNPIN -> when (target) {
                is ActionTarget.Albums -> target.data.ids.forEach { handle(Command.Unpin(CommandUnpinInner(PinTarget.Album(PinTargetAlbumInner(it))))) }
                is ActionTarget.Playlists -> target.data.ids.forEach { handle(Command.Unpin(CommandUnpinInner(PinTarget.Playlist(PinTargetPlaylistInner(it))))) }
                else -> trackIds.forEach { handle(Command.Unpin(CommandUnpinInner(PinTarget.Track(PinTargetTrackInner(it))))) }
            }
            id == ActionIds.REMOVE_FROM_QUEUE -> (target as? ActionTarget.QueueItems)?.let { handle(Command.RemoveQueueItems(CommandRemoveQueueItemsInner(it.data.keys))) }
            id == ActionIds.DELETE_PLAYLIST -> (target as? ActionTarget.Playlists)?.data?.ids?.forEach { handle(Command.DeletePlaylist(CommandDeletePlaylistInner(it))) }
            id == ActionIds.DELETE_SAVED_QUEUE -> (target as? ActionTarget.SavedQueue)?.let { handle(Command.DeleteSavedQueue(CommandDeleteSavedQueueInner(it.data.id))) }
            id == ActionIds.RESTORE_SAVED_QUEUE -> (target as? ActionTarget.SavedQueue)?.let { handle(Command.RestoreSavedQueue(CommandRestoreSavedQueueInner(it.data.id))) }
            id == ActionIds.PIN_SAVED_QUEUE -> (target as? ActionTarget.SavedQueue)?.let { handle(Command.PinSavedQueue(CommandPinSavedQueueInner(it.data.id, true))) }
            id == ActionIds.UNPIN_SAVED_QUEUE -> (target as? ActionTarget.SavedQueue)?.let { handle(Command.PinSavedQueue(CommandPinSavedQueueInner(it.data.id, false))) }
            id == ActionIds.SAVE_QUEUE_AS_PLAYLIST -> handle(Command.SaveQueueAsPlaylist(CommandSaveQueueAsPlaylistInner((target as? ActionTarget.SavedQueue)?.data?.id, context?.label ?: "Queue")))
            id == ActionIds.TOGGLE_PLAY -> handle(Command.TogglePlay)
            id == ActionIds.NEXT -> handle(Command.Next)
            id == ActionIds.PREVIOUS -> handle(Command.Previous)
            id == ActionIds.SHUFFLE -> handle(Command.SetShuffle(CommandSetShuffleInner(shuffle == null)))
            id == ActionIds.REPEAT -> handle(Command.MediaSessionCommand(CommandMediaSessionCommandInner(MediaSessionAction.Repeat, null)))
            id == ActionIds.AUTOPLAY -> handle(Command.SetAutoplay(CommandSetAutoplayInner(!autoplay)))
            id == ActionIds.HANDOFF -> handle(Command.OpenHandoffPicker)
            else -> emit(Event.Error(EventErrorInner(ErrorKind.Internal, "unknown action '$actionId'", null)))
        }
    }

    /** The core's alias table, so desktop-style ids still work on input. */
    private fun canonical(id: String): String = when {
        id.startsWith("rate.") -> "rate" + id.removePrefix("rate.")
        id == "track.love" || id == "ms.love" -> ActionIds.LOVE
        id == "track.unlove" -> ActionIds.UNLOVE
        id == "transport.play" || id == "transport.togglePlay" || id == "ms.play" -> ActionIds.TOGGLE_PLAY
        id.startsWith("transport.") -> id.removePrefix("transport.").let { if (it == "seekBack") "seekBackward" else if (it == "toggleAutoplay") "autoplay" else it }
        id.startsWith("ms.") -> id.removePrefix("ms.")
        id == "ui.removeDownload" -> ActionIds.UNPIN
        id == "ui.delete" -> "remove"
        id == "ui.playOn" -> ActionIds.HANDOFF
        id.startsWith("ui.") -> id.removePrefix("ui.")
        else -> id
    }

    private fun actionsFor(surface: String, target: ActionTarget): List<ActionDescriptor> {
        fun d(id: String, label: String, icon: String, category: String, undoable: Boolean = true, destructive: Boolean = false, enabled: Boolean = true) =
            ActionDescriptor(id, label, icon, category, enabled, null, undoable, destructive)
        val library = listOf(
            d(ActionIds.PLAY, "Play", "play_arrow", "playback"), d(ActionIds.PLAY_SHUFFLED, "Shuffle play", "shuffle", "playback"),
            d(ActionIds.PLAY_NEXT, "Play next", "playlist_play", "queue"), d(ActionIds.PLAY_LATER, "Play later", "playlist_add", "queue"),
            d(ActionIds.ADD_TO_PLAYLIST, "Add to playlist", "playlist_add", "library", undoable = false),
            d(ActionIds.LOVE, "Love", "favorite", "library"), d(ActionIds.UNLOVE, "Unlove", "heart_minus", "library"),
            d(ActionIds.rate(5), "Rate 5", "star", "library"), d(ActionIds.rate(4), "Rate 4", "star", "library"), d(ActionIds.rate(3), "Rate 3", "star", "library"),
            d(ActionIds.rate(2), "Rate 2", "star", "library"), d(ActionIds.rate(1), "Rate 1", "star", "library"), d(ActionIds.rate(0), "Clear rating", "star_outline", "library"),
            d(ActionIds.DOWNLOAD, "Download", "download", "offline", undoable = false), d(ActionIds.UNPIN, "Remove download", "download_done", "offline", undoable = false, destructive = true),
        )
        val base: List<ActionDescriptor> = when (target) {
            is ActionTarget.Tracks -> library + listOf(d(ActionIds.GO_TO_ALBUM, "Go to album", "album", "navigation", undoable = false), d(ActionIds.GO_TO_ARTIST, "Go to artist", "artist", "navigation", undoable = false))
            is ActionTarget.Albums -> library + listOf(d(ActionIds.GO_TO_ARTIST, "Go to artist", "artist", "navigation", undoable = false))
            is ActionTarget.Artists -> library
            is ActionTarget.Playlists -> library + listOf(d(ActionIds.DELETE_PLAYLIST, "Delete playlist", "delete_forever", "library", undoable = false, destructive = true))
            is ActionTarget.QueueItems -> listOf(d(ActionIds.REMOVE_FROM_QUEUE, "Remove from queue", "remove_from_queue", "queue"), d(ActionIds.PLAY_NEXT, "Play next", "playlist_play", "queue"),
                d(ActionIds.ADD_TO_PLAYLIST, "Add to playlist", "playlist_add", "library", undoable = false), d(ActionIds.LOVE, "Love", "favorite", "library"), d(ActionIds.DOWNLOAD, "Download", "download", "offline", undoable = false),
                d(ActionIds.GO_TO_ALBUM, "Go to album", "album", "navigation", undoable = false), d(ActionIds.GO_TO_ARTIST, "Go to artist", "artist", "navigation", undoable = false))
            is ActionTarget.SavedQueue -> listOf(d(ActionIds.RESTORE_SAVED_QUEUE, "Restore", "history", "queue"), d(ActionIds.PIN_SAVED_QUEUE, "Pin", "push_pin", "queue"), d(ActionIds.UNPIN_SAVED_QUEUE, "Unpin", "keep_off", "queue"),
                d(ActionIds.SAVE_QUEUE_AS_PLAYLIST, "Save as playlist", "playlist_add_check", "queue", undoable = false), d(ActionIds.DELETE_SAVED_QUEUE, "Delete", "delete_forever", "queue", undoable = false, destructive = true))
            ActionTarget.None -> emptyList()
        }
        val order = actionOrders[surface]?.takeIf { it.isNotEmpty() } ?: if (surface == "contextMenu") ActionIds.CONTEXT_MENU else return base
        return base.sortedBy { order.indexOf(it.id).let { i -> if (i < 0) Int.MAX_VALUE else i } }
    }

    // -- queries -----------------------------------------------------------------------------------
    private fun answer(q: Query): QueryResult = when (q) {
        Query.Snapshot -> QueryResult.SnapshotResult(snapshot())
        Query.Servers -> QueryResult.Servers(servers.toList())
        is Query.Tracks -> {
            val all = q.data.filter?.let { f -> library.tracks.filter { matches(it, f) } } ?: library.tracks
            val sorted = library.sorted(all, q.data.sort, q.data.descending)
            val off = q.data.page.offset.toInt()
            QueryResult.Tracks(TrackPage(sorted.drop(off).take(q.data.page.limit.toInt()), off.toUInt(), sorted.size.toUInt()))
        }
        is Query.TrackCount -> QueryResult.Count((q.data.filter?.let { f -> library.tracks.count { matches(it, f) } } ?: library.tracks.size).toUInt())
        is Query.Track -> QueryResult.TrackDetail(library.track(q.data.id))
        is Query.TracksByIds -> QueryResult.TrackList(q.data.ids.mapNotNull { library.track(it) })
        is Query.Albums -> {
            val all = library.albums.filter { (q.data.artist_id == null || it.artistId == q.data.artist_id) && (q.data.genre == null || it.genre == q.data.genre) }
            val sorted = library.sortedAlbums(all, q.data.sort, q.data.descending)
            val off = q.data.page.offset.toInt()
            QueryResult.Albums(AlbumPage(sorted.drop(off).take(q.data.page.limit.toInt()), off.toUInt(), sorted.size.toUInt()))
        }
        is Query.AlbumCount -> QueryResult.Count(library.albums.count { (q.data.artist_id == null || it.artistId == q.data.artist_id) && (q.data.genre == null || it.genre == q.data.genre) }.toUInt())
        is Query.Album -> QueryResult.AlbumDetail(library.album(q.data.id))
        is Query.AlbumTracks -> QueryResult.TrackList(library.albumTracks(q.data.id))
        is Query.Artists -> {
            val sorted = library.artists.sortedBy { it.name.lowercase() }
            val off = q.data.page.offset.toInt()
            QueryResult.Artists(ArtistPage(sorted.drop(off).take(q.data.page.limit.toInt()), off.toUInt(), sorted.size.toUInt()))
        }
        is Query.Artist -> QueryResult.ArtistDetail(library.artist(q.data.id))
        is Query.ArtistTopSongs -> QueryResult.TrackList(library.artistAlbums(q.data.id).flatMap { library.albumTracks(it.id) }.sortedByDescending { it.playCount }.take(q.data.count.toInt()))
        is Query.Genres -> QueryResult.Genres(library.genres.toList())
        is Query.Playlists -> QueryResult.Playlists(library.playlists.toList())
        is Query.Playlist -> QueryResult.PlaylistDetail(library.playlist(q.data.id))
        is Query.PlaylistTracks -> {
            val ids = library.playlistTracks[q.data.id] ?: emptyList()
            val off = q.data.page.offset.toInt()
            QueryResult.Tracks(TrackPage(ids.drop(off).take(q.data.page.limit.toInt()).mapNotNull { library.track(it) }, off.toUInt(), ids.size.toUInt()))
        }
        is Query.Search -> search(q.data)
        Query.Queue -> QueryResult.Queue(queueView())
        Query.SavedQueues -> QueryResult.SavedQueues(savedQueues.toList())
        is Query.Lyrics -> QueryResult.LyricsResult(lyricsFor(q.data.track_id))
        is Query.Related -> {
            val seed = library.track(q.data.track_id)
            QueryResult.Related((0 until q.data.count.toInt()).map { i ->
                val t = library.tracks[((seed?.id?.drop(1)?.toIntOrNull() ?: 0) * 13 + i * 41) % library.tracks.size]
                RelatedTrack(t.toSummary(), if (i % 3 == 0) AutoplayProvider.SimilarSongs else AutoplayProvider.SonicSimilarity, 0.95 - i * 0.05, "Similar to ${seed?.title}")
            })
        }
        is Query.Stats -> stats(q.data.period_days.toInt())
        is Query.RecentlyPlayed -> QueryResult.History(playHistory.take(q.data.limit.toInt()))
        Query.Jobs -> QueryResult.Jobs(jobs.toList())
        Query.Problems -> QueryResult.Problems(problems.toList())
        Query.Pins -> QueryResult.Pins(pins.toList())
        Query.Storage -> QueryResult.Storage(storage)
        Query.Filters -> QueryResult.Filters(filters.toList())
        is Query.FilterPreview -> {
            val matched = evaluateFilter(q.data.filter)
            val local = fields(q.data.filter.root).filter { it in localOnly }.distinct()
            QueryResult.Preview(FilterPreview(matched.size.toUInt(), FilterCapability(local.isEmpty(), local), matched.take(5).map { it.toSummary() }))
        }
        Query.Settings -> QueryResult.Settings(settings.values.toList())
        is Query.Setting -> QueryResult.SettingDetail(settings[q.data.key])
        Query.AudioSettings -> QueryResult.Audio(audio)
        Query.OutputDevices -> QueryResult.OutputDevices(listOf(OutputDevice("default", "Phone speaker", true)))
        Query.Connection -> QueryResult.Connection(connection)
        Query.Devices -> QueryResult.Devices(devices.toList())
        Query.UndoState -> QueryResult.Undo(undoState())
        is Query.Actions -> QueryResult.Actions(actionsFor(q.data.surface, q.data.target))
        Query.Shortcuts -> QueryResult.Shortcuts(emptyList())
        is Query.Artwork -> QueryResult.Path(null)
        is Query.MediaSource -> QueryResult.Source(library.track(q.data.track_id)?.let { MediaSource("k", it.toSummary(), "https://music.example.net/rest/stream?id=${it.id}", hashMapOf(), "audio/flac", -3.0, false) })
        Query.Diagnostics -> QueryResult.Text("Hocket fake core\ndevice=$deviceId\nserver=${servers.firstOrNull()?.url}\ntracks=${library.tracks.size}\n")
        is Query.ConfigDocument -> QueryResult.Text(configDocument(q.data.include_secrets))
    }

    private fun search(q: QuerySearchInner): QueryResult {
        val needle = q.query.trim().lowercase()
        val limit = q.limit.toInt()
        val tracks = if (needle.isEmpty()) emptyList() else library.tracks.filter { it.title.lowercase().contains(needle) || it.artist?.lowercase()?.contains(needle) == true }.take(limit)
        val albums = if (needle.isEmpty()) emptyList() else library.albums.filter { it.name.lowercase().contains(needle) || it.artist?.lowercase()?.contains(needle) == true }.take(limit)
        val artists = if (needle.isEmpty()) emptyList() else library.artists.filter { it.name.lowercase().contains(needle) }.take(limit)
        val playlists = if (needle.isEmpty()) emptyList() else library.playlists.filter { it.name.lowercase().contains(needle) }.take(limit)
        val local = SearchResults(q.request_id, q.query, tracks.map { it.toSummary() }, albums, artists, playlists, false)
        if (q.include_server && needle.isNotEmpty() && timers) scope.launch {
            delay(700)
            // "Server" results: a handful of extra matches the mirror wouldn't have yet.
            val extra = library.tracks.filter { it.album?.lowercase()?.contains(needle) == true && it !in tracks }.take(6)
            emit(Event.SearchResults(EventSearchResultsInner(SearchResults(q.request_id, q.query, extra.map { it.toSummary() }, emptyList(), emptyList(), emptyList(), true))))
        }
        return QueryResult.Search(local)
    }

    private fun stats(days: Int): QueryResult {
        val since = now() - days * 86_400_000.0
        val plays = playHistory.filter { it.playedAt >= since }
        val byTrack = plays.groupBy { it.track.id }.entries.sortedByDescending { it.value.size }
        val byAlbum = plays.groupBy { it.track.albumId }.entries.sortedByDescending { it.value.size }
        val byArtist = plays.groupBy { library.track(it.track.id)?.artistId }.entries.sortedByDescending { it.value.size }
        val hours = IntArray(24); val weekdays = IntArray(7)
        plays.forEachIndexed { i, p -> hours[((p.playedAt / 3_600_000).toLong() % 24).toInt()]++; weekdays[((p.playedAt / 86_400_000).toLong() % 7).toInt()]++ }
        return QueryResult.Stats(ListeningStats(days.toUInt(), plays.size.toUInt(), plays.sumOf { it.playedMs.toDouble() },
            byTrack.take(10).map { it.value.first().track }, byAlbum.take(10).mapNotNull { it.key?.let(library::album) }, byArtist.take(10).mapNotNull { it.key?.let(library::artist) },
            hours.map { it.toUInt() }, weekdays.map { it.toUInt() }))
    }

    companion object {
        fun defaultBands() = listOf(31.0, 62.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0).map { EqBand(it, 0.0, 1.0) }
    }
}
