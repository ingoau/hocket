package app.hocket.playback.pebble

import android.content.Context
import android.util.Log
import app.hocket.core.Commands
import app.hocket.core.CoreHandle
import app.hocket.core.HocketJson
import app.hocket.core.Queries
import app.hocket.core.SettingKeys
import app.hocket.core.api.Command
import app.hocket.core.api.DeviceInfo
import app.hocket.core.api.Event
import app.hocket.core.api.LyricsTier
import app.hocket.core.api.MediaSessionAction
import app.hocket.core.api.MediaSessionState
import app.hocket.core.api.Page
import app.hocket.core.api.PinTarget
import app.hocket.core.api.QueryResult
import app.hocket.core.api.QueueView
import app.hocket.core.api.RepeatMode
import app.hocket.core.api.ResumeOffer
import app.hocket.core.api.SearchResults
import app.hocket.core.api.Snapshot
import app.hocket.core.api.TrackSummary
import app.hocket.core.client.PositionClock
import app.hocket.playback.pebble.PebbleProtocol.Cmd
import app.hocket.playback.pebble.PebbleProtocol.Flag
import app.hocket.playback.pebble.PebbleProtocol.Key
import app.hocket.playback.pebble.PebbleProtocol.PAGE
import app.hocket.playback.pebble.PebbleProtocol.Row
import app.hocket.playback.pebble.PebbleProtocol.RowFlag
import app.hocket.playback.pebble.PebbleProtocol.Type
import io.rebble.pebblekit2.client.DefaultPebbleSender
import io.rebble.pebblekit2.common.model.PebbleDictionary
import io.rebble.pebblekit2.common.model.PebbleDictionaryItem
import java.util.UUID
import kotlin.math.roundToInt
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.builtins.serializer

/**
 * The phone half of the Pebble watchapp (docs/pebble.md): translates the watch's messages into the
 * same commands and queries the UI uses, and pushes now-playing state and settings back while the
 * watchapp is open. Owned by the playback service, which forwards the core's events; messages from
 * the watch arrive through [PebbleListenerService]. No decisions of its own beyond shaping lists
 * for a small screen.
 */
class PebbleBridge(
    private val context: Context,
    private val core: () -> CoreHandle,
    private val serverId: () -> String?,
    private val scope: CoroutineScope,
) {
    companion object {
        private const val TAG = "PebbleBridge"
        /** Session updates come in bursts (a skip changes metadata, then position); the watch hears once. */
        const val NOW_DEBOUNCE_MS = 150L
        private const val SEARCH_LIMIT = 10
        private const val RECENT_LIMIT = 50
        private const val TEXT_MAX = 95
        /** One volume button press, as the core's volume up/down. */
        private const val VOLUME_STEP = 0.05
        /** Played tracks before the current one: fewer than a page, so the first page holds it. */
        private const val QUEUE_HISTORY = 8
        /** Page size when a held playlist row asks for all of its tracks. */
        private const val PLAYLIST_PAGE = 500u
    }

    private val senderLazy = lazy { DefaultPebbleSender(context) }
    private val sender by senderLazy
    private val sendLock = Mutex()
    private var nowJob: Job? = null
    private var artJob: Job? = null
    private var artJobGen = -1

    /** The watchapp is open on a watch: state is pushed only then. */
    private var open = false
    private var session: MediaSessionState? = null
    private var clockOffsetMs = 0.0
    private var queue: QueueView? = null
    private var devices: List<DeviceInfo> = emptyList()
    private var resumeOffer: ResumeOffer? = null
    private val settings = HashMap<String, String>()
    /** [settings] holds the core's values (a snapshot arrived); until then the watch keeps its own. */
    private var settingsLoaded = false
    private var lastSearch: Pair<String, SearchResults>? = null

    // -- core events (main thread) -----------------------------------------------------------------

    fun onEvent(event: Event) {
        when (event) {
            is Event.MediaSession -> onSession(event.data.state)
            is Event.QueueChanged -> { queue = event.data.queue; pushNowSoon() }
            is Event.DevicesChanged -> devices = event.data.devices
            is Event.ResumeOfferChanged -> { resumeOffer = event.data.offer; pushNowSoon() }
            is Event.ConnectionChanged -> clockOffsetMs = event.data.state.clockOffsetMs
            is Event.SettingChanged -> {
                val s = event.data.setting
                settings[s.key] = s.value
                if (s.key.startsWith("pebble.") && open && settingsLoaded) scope.launch { send(settingsMessage()) }
            }
            is Event.Started -> onSnapshot(event.data.snapshot)
            is Event.Snapshot -> onSnapshot(event.data.snapshot)
            else -> Unit
        }
    }

    private fun onSnapshot(snapshot: Snapshot) {
        clockOffsetMs = snapshot.connection.clockOffsetMs
        queue = snapshot.queue
        devices = snapshot.devices
        resumeOffer = snapshot.resumeOffer
        snapshot.settings.forEach { settings[it.key] = it.value }
        settingsLoaded = true
        if (open) scope.launch { send(settingsMessage()) }
        onSession(snapshot.mediaSession)
    }

    private fun onSession(state: MediaSessionState) {
        val before = session
        session = state
        // Auto-open: this device started playing (not the first state seen, not a remote device).
        val started = before != null && !(before.isPlaying && before.ownsTransport) && state.isPlaying && state.ownsTransport
        if (started && bool(SettingKeys.PEBBLE_AUTO_OPEN, false) && !open) {
            scope.launch { runCatching { sender.startAppOnTheWatch(PebbleProtocol.APP_UUID) }.onFailure { Log.w(TAG, "start app failed: ${it.message}") } }
        }
        pushNowSoon()
    }

    // -- the watch -----------------------------------------------------------------------------------

    fun appOpened() { open = true }

    fun appClosed() {
        open = false
        nowJob?.cancel()
        artJob?.cancel()
    }

    fun release() {
        appClosed()
        // Never create the sender just to close it (it connects to the Pebble app as it starts).
        if (senderLazy.isInitialized()) runCatching { sender.close() }
    }

    suspend fun onMessage(data: PebbleDictionary) {
        open = true
        when (num(data, Key.TYPE)) {
            Type.HELLO -> onHello(num(data, Key.PROTO), text(data, Key.BUILD))
            Type.CMD -> onCommand(num(data, Key.CMD), num(data, Key.ARG), text(data, Key.PATH), text(data, Key.ITEM))
            // Lists can take a while (a search, a long playlist): answer the watch's message first.
            Type.LIST_REQ -> scope.launch { onListRequest(num(data, Key.REQ_ID), text(data, Key.PATH), num(data, Key.OFFSET)) }
            Type.ART_REQ -> onArtRequest(num(data, Key.ART_GEN), num(data, Key.ARG))
        }
    }

    private suspend fun onHello(proto: Int, build: String) {
        send(mapOf(Key.TYPE to u8(Type.HELLO_REPLY), Key.PROTO to u32(PebbleProtocol.PROTO), Key.BUILD to PebbleDictionaryItem.Text(PebbleInstaller.bundledVersion)))
        // Before the core's settings have loaded these would be the defaults, wiping the watch's copy.
        if (settingsLoaded) send(settingsMessage())
        send(nowMessage())
        if (build.isNotEmpty()) PebbleInstaller.recordWatchVersion(context, build)
        if (proto != PebbleProtocol.PROTO || build != PebbleInstaller.bundledVersion) PebbleInstaller.notifyUpdate(context)
    }

    private fun pushNowSoon() {
        if (!open) return
        nowJob?.cancel()
        nowJob = scope.launch {
            delay(NOW_DEBOUNCE_MS)
            send(nowMessage())
        }
    }

    private suspend fun send(message: PebbleDictionary) = sendLock.withLock {
        runCatching { sender.sendDataToPebble(PebbleProtocol.APP_UUID, message) }
            .onFailure { Log.w(TAG, "send failed: ${it.message}") }
    }

    // -- now playing ---------------------------------------------------------------------------------

    private fun nowMessage(): PebbleDictionary {
        val s = session
        val m = s?.metadata
        val q = queue
        var flags = 0
        if (s?.isPlaying == true) flags = flags or Flag.PLAYING
        if (s?.shuffle == true) flags = flags or Flag.SHUFFLE
        if (m?.loved == true) flags = flags or Flag.LOVED
        if (m != null) flags = flags or Flag.HAS_TRACK
        val resumeLabel = resumeOffer?.let { "${it.track.title} · ${it.deviceName}" } ?: q?.current?.track?.title
        if (resumeLabel != null) flags = flags or Flag.CAN_RESUME
        flags = flags or Flag.HAS_VOLUME
        if (s != null && MediaSessionAction.Seek in s.actions && (m?.durationMs ?: 0u) > 0u) flags = flags or Flag.REPORTS_POSITION
        val position = s?.let { PositionClock.extrapolate(it.position, System.currentTimeMillis().toDouble(), clockOffsetMs, m?.durationMs?.toLong()) } ?: 0L
        return mapOf(
            Key.TYPE to u8(Type.NOW),
            Key.TITLE to str(m?.title),
            Key.ARTIST to str(m?.artist),
            Key.ALBUM to str(m?.album),
            Key.DURATION to u32(m?.durationMs?.toInt() ?: 0),
            Key.POSITION to u32(position.toInt()),
            Key.FLAGS to u8(flags),
            Key.REPEAT to u8(when (s?.repeat) { RepeatMode.All -> 1; RepeatMode.One -> 2; else -> 0 }),
            Key.RATING to u8(m?.rating?.toInt()?.coerceIn(0, 5) ?: 0),
            Key.RESUME_LABEL to str(resumeLabel),
            Key.ART_GEN to u32(PebbleProtocol.artGeneration(m?.artworkPath)),
            Key.VOLUME to u8(volumePercent()),
        )
    }

    // -- commands ------------------------------------------------------------------------------------

    private suspend fun onCommand(cmd: Int, arg: Int, path: String, item: String) {
        val s = session
        val trackId = s?.metadata?.trackId
        when (cmd) {
            Cmd.PLAY_PAUSE -> dispatch(Commands.mediaSessionCommand(if (s?.isPlaying == true) MediaSessionAction.Pause else MediaSessionAction.Play))
            Cmd.PLAY -> dispatch(Commands.mediaSessionCommand(MediaSessionAction.Play))
            Cmd.PAUSE -> dispatch(Commands.mediaSessionCommand(MediaSessionAction.Pause))
            Cmd.NEXT -> dispatch(Commands.mediaSessionCommand(MediaSessionAction.Next))
            Cmd.PREVIOUS -> dispatch(Commands.mediaSessionCommand(MediaSessionAction.Previous))
            Cmd.VOLUME_UP -> changeVolume(up = true)
            Cmd.VOLUME_DOWN -> changeVolume(up = false)
            Cmd.TOGGLE_LOVE -> trackId?.let { dispatch(Commands.loveTrack(it, !(s.metadata?.loved ?: false))) }
            Cmd.SET_RATING -> trackId?.let { dispatch(Commands.rateTrack(it, arg.coerceIn(0, 5))) }
            Cmd.TOGGLE_SHUFFLE -> dispatch(Commands.setShuffle(!(s?.shuffle ?: false)))
            Cmd.CYCLE_REPEAT -> dispatch(Commands.setRepeat(when (s?.repeat) { RepeatMode.Off, null -> RepeatMode.All; RepeatMode.All -> RepeatMode.One; RepeatMode.One -> RepeatMode.Off }))
            Cmd.RESUME -> dispatch(if (resumeOffer != null) Command.ResumeHere else Commands.mediaSessionCommand(MediaSessionAction.Play))
            Cmd.SEEK -> dispatch(Commands.mediaSessionCommand(MediaSessionAction.Seek, arg.coerceAtLeast(0).toDouble()))
            Cmd.ACTIVATE -> activate(path, item)
            Cmd.PLAY_NEXT, Cmd.PLAY_LATER -> {
                val server = serverId() ?: return
                // A whole playlist can take a few queries: off the watch's message.
                scope.launch {
                    val ids = trackIds(path, item)
                    if (ids.isNotEmpty()) dispatch(if (cmd == Cmd.PLAY_NEXT) Commands.playNext(server, ids) else Commands.playLater(server, ids))
                }
            }
        }
        // The watch already changed its own state optimistically; this confirms it.
        pushNowSoon()
    }

    /**
     * The watch's volume buttons drive the device playing, like every volume control: the core
     * routes the change (this phone's own volume is its music volume). 5 % a press, from silence
     * when muted (a press unmutes).
     */
    private fun changeVolume(up: Boolean) {
        val s = session ?: return
        val from = if (s.muted == true) 0.0 else s.volume
        dispatch(Commands.setVolume((from + if (up) VOLUME_STEP else -VOLUME_STEP).coerceIn(0.0, 1.0)))
    }

    /** The level [changeVolume] changes, 0..100 (255 = unknown). */
    private fun volumePercent(): Int {
        val s = session ?: return 255
        return if (s.muted == true) 0 else (s.volume.coerceIn(0.0, 1.0) * 100).roundToInt()
    }

    /** Selecting a row that is not browsable: play it, jump to it, or hand off to it. */
    private suspend fun activate(rawPath: String, item: String) {
        val server = serverId() ?: return
        val path = resolve(rawPath) ?: return
        val (kind, id) = split(path)
        when (kind) {
            "queue" -> dispatch(Commands.jumpToQueueItem(item))
            "devices" -> dispatch(Commands.handoffTo(item))
            "album" -> albumName(id)?.let { name -> playIn(Commands.albumContext(server, id, name), item) }
            "playlist" -> playlistName(id)?.let { name -> playIn(Commands.playlistContext(server, id, name), item) }
            "artist" -> artistName(id)?.let { name -> playIn(Commands.artistContext(server, id, name), item) }
            "search" -> if (item == "top") playTopResult(server, id) else playRow(server, item)
            else -> playRow(server, item)
        }
    }

    /** `play`, `shuffle` or `t:<index>` inside an album, playlist or artist. */
    private fun playIn(context: app.hocket.core.api.QueueContext, item: String) {
        when {
            item == "play" -> dispatch(Commands.playContext(context))
            item == "shuffle" -> dispatch(Commands.playContext(context, shuffle = true))
            item.startsWith("t:") -> item.removePrefix("t:").toIntOrNull()?.let { dispatch(Commands.playContext(context, startIndex = it)) }
        }
    }

    private suspend fun playRow(server: String, item: String) {
        if (item.startsWith("track:")) {
            val id = item.removePrefix("track:")
            dispatch(Commands.playTracks(server, listOf(id), 0, trackTitle(id) ?: ""))
        }
    }

    private suspend fun playTopResult(server: String, text: String) {
        val r = search(server, text) ?: return
        r.tracks.firstOrNull()?.let { dispatch(Commands.playTracks(server, listOf(it.id), 0, it.title)); return }
        r.albums.firstOrNull()?.let { dispatch(Commands.playContext(Commands.albumContext(server, it.id, it.name))); return }
        r.artists.firstOrNull()?.let { dispatch(Commands.playContext(Commands.artistContext(server, it.id, it.name))); return }
        r.playlists.firstOrNull()?.let { dispatch(Commands.playContext(Commands.playlistContext(server, it.id, it.name))) }
    }

    /** The tracks a held row stands for (Play next / Add to queue). */
    private suspend fun trackIds(rawPath: String, item: String): List<String> {
        if (item.startsWith("track:")) return listOf(item.removePrefix("track:"))
        if (item.startsWith("album:")) return albumTracks(item.removePrefix("album:")).map { it.id }
        if (item.startsWith("playlist:")) return playlistTrackIds(item.removePrefix("playlist:"))
        val path = resolve(rawPath) ?: return emptyList()
        val (kind, id) = split(path)
        val index = item.removePrefix("t:").toIntOrNull()
        return when {
            kind == "queue" -> queueEntries().firstOrNull { it.item.key == item }?.let { listOf(it.track.id) }.orEmpty()
            kind == "album" && index != null -> listOfNotNull(albumTracks(id).getOrNull(index)?.id)
            kind == "playlist" && index != null -> (query(Queries.playlistTracks(id, Page(index.toUInt(), 1u))) as? QueryResult.Tracks)?.data?.items?.map { it.id }.orEmpty()
            else -> emptyList()
        }
    }

    /** Every track of a playlist, page by page. */
    private suspend fun playlistTrackIds(id: String): List<String> {
        val ids = mutableListOf<String>()
        while (true) {
            val page = (query(Queries.playlistTracks(id, Page(ids.size.toUInt(), PLAYLIST_PAGE))) as? QueryResult.Tracks)?.data ?: break
            if (page.items.isEmpty()) break
            ids += page.items.map { it.id }
            if (ids.size.toUInt() >= page.total) break
        }
        return ids
    }

    // -- lists ---------------------------------------------------------------------------------------

    private suspend fun onListRequest(reqId: Int, rawPath: String, offset: Int) {
        val (total, rows, title) = runCatching { list(rawPath, offset.coerceAtLeast(0)) }
            .onFailure { Log.w(TAG, "list $rawPath failed: ${it.message}") }
            .getOrDefault(Triple(0, emptyList(), null))
        val message = mutableMapOf(
            Key.TYPE to u8(Type.LIST),
            Key.REQ_ID to u16(reqId),
            Key.OFFSET to u16(offset),
            Key.TOTAL to u16(total),
            Key.ITEMS to PebbleDictionaryItem.Bytes(PebbleProtocol.encodeRows(rows.take(PAGE))),
        )
        if (title != null) message[Key.LIST_TITLE] = str(title)
        send(message)
    }

    /** One page of [rawPath] from [offset]: the total, the rows and (sometimes) a title for the watch. */
    private suspend fun list(rawPath: String, offset: Int): Triple<Int, List<Row>, String?> {
        val server = serverId()
        val path = resolve(rawPath) ?: return Triple(0, emptyList(), null)
        val (kind, id) = split(path)
        val title = if (rawPath == "goAlbum" || rawPath == "goArtist") session?.metadata?.let { if (rawPath == "goAlbum") it.album else it.artist } else null
        fun whole(rows: List<Row>) = Triple(rows.size, rows.drop(offset).take(PAGE), title)
        return when (kind) {
            "queue" -> whole(queueRows())
            "lyrics" -> whole(lyricsRows())
            "devices" -> whole(devices.filter { !it.isSelf }.map { Row(it.id, it.name, if (it.playing) "Playing" else "") })
            "library" -> whole(librarySections().map { section ->
                if (section == "search") Row("search", "Search", flags = RowFlag.SEARCH) else Row(section, sectionTitle(section), flags = RowFlag.BROWSABLE)
            })
            else -> {
                if (server == null) return Triple(0, emptyList(), title)
                when (kind) {
                    "playlists" -> whole((query(Queries.playlists(server)) as? QueryResult.Playlists)?.data.orEmpty()
                        .map { Row("playlist:${it.id}", it.name, "${it.songCount} songs", RowFlag.BROWSABLE) })
                    "albums" -> {
                        val page = (query(Queries.albums(server, Page(offset.toUInt(), PAGE.toUInt()))) as? QueryResult.Albums)?.data
                        Triple(page?.total?.toInt() ?: 0, page?.items.orEmpty().map { Row("album:${it.id}", it.name, it.artist ?: "", RowFlag.BROWSABLE) }, title)
                    }
                    "artists" -> {
                        val page = (query(Queries.artists(server, Page(offset.toUInt(), PAGE.toUInt()))) as? QueryResult.Artists)?.data
                        Triple(page?.total?.toInt() ?: 0, page?.items.orEmpty().map { Row("artist:${it.id}", it.name, "${it.albumCount} albums", RowFlag.BROWSABLE) }, title)
                    }
                    "recent" -> whole((query(Queries.recentlyPlayed(RECENT_LIMIT)) as? QueryResult.History)?.data.orEmpty()
                        .map { it.track }.distinctBy { it.id }.map(::trackRow))
                    "downloads" -> whole((query(app.hocket.core.api.Query.Pins) as? QueryResult.Pins)?.data.orEmpty().map { pin ->
                        when (val t = pin.target) {
                            is PinTarget.Album -> Row("album:${t.data.id}", pin.label, "Album", RowFlag.BROWSABLE)
                            is PinTarget.Playlist -> Row("playlist:${t.data.id}", pin.label, "Playlist", RowFlag.BROWSABLE)
                            is PinTarget.Track -> Row("track:${t.data.id}", pin.label, "", RowFlag.TRACK)
                        }
                    })
                    "album" -> whole(playShuffle() + albumTracks(id).mapIndexed { i, t ->
                        Row("t:$i", t.title, PebbleProtocol.duration(t.durationMs.toLong()), RowFlag.TRACK)
                    })
                    "playlist" -> {
                        // Paged: the play / shuffle rows are 0 and 1, track i is row i + 2.
                        val from = (offset - 2).coerceAtLeast(0)
                        val page = (query(Queries.playlistTracks(id, Page(from.toUInt(), PAGE.toUInt()))) as? QueryResult.Tracks)?.data
                        val tracks = page?.items.orEmpty().mapIndexed { i, t -> Row("t:${from + i}", t.title, t.artist ?: "", RowFlag.TRACK) }
                        val rows = (if (offset < 2) playShuffle().drop(offset) else emptyList()) + tracks
                        Triple(2 + (page?.total?.toInt() ?: 0), rows, title)
                    }
                    "artist" -> {
                        val albums = (query(Queries.albums(server, Page(0u, 500u), artistId = id)) as? QueryResult.Albums)?.data?.items.orEmpty()
                        whole(playShuffle() + albums.map { Row("album:${it.id}", it.name, it.year?.toString() ?: "", RowFlag.BROWSABLE) })
                    }
                    "search" -> {
                        val r = search(server, id) ?: return Triple(0, emptyList(), title)
                        val rows = buildList {
                            if (r.tracks.isNotEmpty() || r.albums.isNotEmpty() || r.artists.isNotEmpty() || r.playlists.isNotEmpty()) add(Row("top", "Play top result"))
                            r.tracks.forEach { add(trackRow(it)) }
                            r.albums.forEach { add(Row("album:${it.id}", it.name, it.artist ?: "Album", RowFlag.BROWSABLE)) }
                            r.artists.forEach { add(Row("artist:${it.id}", it.name, "Artist", RowFlag.BROWSABLE)) }
                            r.playlists.forEach { add(Row("playlist:${it.id}", it.name, "Playlist", RowFlag.BROWSABLE)) }
                        }
                        Triple(rows.size, rows.drop(offset).take(PAGE), "“$id”")
                    }
                    else -> Triple(0, emptyList(), title)
                }
            }
        }
    }

    private fun playShuffle() = listOf(Row("play", "Play"), Row("shuffle", "Shuffle"))

    private fun trackRow(t: TrackSummary) = Row("track:${t.id}", t.title, t.artist ?: "", RowFlag.TRACK)

    /** Recent history (enough to scroll back a little), the current track, Playing Next and the rest. */
    private fun queueEntries() = queue?.let { it.history.takeLast(QUEUE_HISTORY) + listOfNotNull(it.current) + it.playingNext + it.upcoming }.orEmpty()

    /** [queueEntries] as rows; the current one is in the first page, where the watch selects it. */
    private fun queueRows(): List<Row> {
        val q = queue ?: return emptyList()
        val current = q.current?.item?.key
        return queueEntries().map { e ->
            if (e.item.key == current) Row(e.item.key, e.track.title, "Now playing", RowFlag.CURRENT or RowFlag.TRACK)
            else Row(e.item.key, e.track.title, e.track.artist ?: "", RowFlag.TRACK)
        }
    }

    /** The playing track's lines; id = start ms with the track's timing offset, empty when unsynced. */
    private suspend fun lyricsRows(): List<Row> {
        val trackId = session?.metadata?.trackId ?: return emptyList()
        val lyrics = (query(Queries.lyrics(trackId)) as? QueryResult.LyricsResult)?.data ?: return emptyList()
        val synced = lyrics.tier != LyricsTier.Unsynced
        return lyrics.lines.filter { !it.background }.map { line ->
            val start = line.startMs?.takeIf { synced }?.let { (it.toLong() + lyrics.offsetMs).coerceAtLeast(0) }
            Row(start?.toString() ?: "", line.text.ifBlank { "♪" })
        }
    }

    private fun librarySections(): List<String> =
        stringList(SettingKeys.PEBBLE_LIBRARY).filter { it in PebbleProtocol.LIBRARY_SECTIONS }.ifEmpty { PebbleProtocol.LIBRARY_SECTIONS }

    private fun sectionTitle(section: String) = when (section) {
        "playlists" -> "Playlists"
        "albums" -> "Albums"
        "artists" -> "Artists"
        "recent" -> "Recently played"
        "downloads" -> "Downloads"
        else -> section
    }

    /** `goAlbum` / `goArtist` become the playing track's album / artist; null when there is none. */
    private suspend fun resolve(path: String): String? = when (path) {
        "goAlbum", "goArtist" -> {
            val trackId = session?.metadata?.trackId
            val track = trackId?.let { (query(Queries.track(it)) as? QueryResult.TrackDetail)?.data }
            if (path == "goAlbum") track?.albumId?.let { "album:$it" } else track?.artistId?.let { "artist:$it" }
        }
        else -> path
    }

    private fun split(path: String): Pair<String, String> =
        path.indexOf(':').let { if (it < 0) path to "" else path.substring(0, it) to path.substring(it + 1) }

    private suspend fun search(server: String, text: String): SearchResults? {
        lastSearch?.takeIf { it.first == text }?.let { return it.second }
        val r = (query(Queries.search(server, text.trim(), SEARCH_LIMIT, includeServer = false, requestId = "pebble-" + UUID.randomUUID())) as? QueryResult.Search)?.data
        if (r != null) lastSearch = text to r
        return r
    }

    // -- album art -----------------------------------------------------------------------------------

    private fun onArtRequest(gen: Int, size: Int) {
        val path = session?.metadata?.artworkPath ?: return
        if (gen != PebbleProtocol.artGeneration(path)) return
        // The watch retries when chunks are slow; a transfer of this cover still going carries on.
        if (artJob?.isActive == true && artJobGen == gen) return
        artJob?.cancel()
        artJobGen = gen
        artJob = scope.launch {
            val art = kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.Default) {
                runCatching { PebbleArt.load(path, size ushr 16, size and 0xffff) }.getOrNull()
            } ?: return@launch
            var offset = 0
            while (offset < art.data.size) {
                val end = minOf(offset + PebbleProtocol.ART_CHUNK, art.data.size)
                val message = mutableMapOf(
                    Key.TYPE to u8(Type.ART),
                    Key.ART_GEN to u32(gen),
                    Key.ART_OFFSET to u32(offset),
                    Key.ART_DATA to PebbleDictionaryItem.Bytes(art.data.copyOfRange(offset, end)),
                )
                if (offset == 0) {
                    message[Key.ART_PALETTE] = PebbleDictionaryItem.Bytes(art.palette)
                    message[Key.ART_WIDTH] = u16(art.width)
                    message[Key.ART_HEIGHT] = u16(art.height)
                    message[Key.ART_SIZE] = u32(art.data.size)
                }
                send(message)
                offset = end
            }
        }
    }

    // -- settings ------------------------------------------------------------------------------------

    private fun settingsMessage(): PebbleDictionary {
        val defaults = PebbleProtocol.WatchSettings()
        val gestures = listOf(SettingKeys.PEBBLE_GESTURE_UP, SettingKeys.PEBBLE_GESTURE_DOWN, SettingKeys.PEBBLE_GESTURE_LEFT, SettingKeys.PEBBLE_GESTURE_RIGHT, SettingKeys.PEBBLE_GESTURE_HOLD)
        val s = PebbleProtocol.WatchSettings(
            touchNavigation = bool(SettingKeys.PEBBLE_TOUCH_NAVIGATION, defaults.touchNavigation),
            buttonsOnly = bool(SettingKeys.PEBBLE_BUTTONS_ONLY, defaults.buttonsOnly),
            showVolumeControls = bool(SettingKeys.PEBBLE_SHOW_VOLUME_CONTROLS, defaults.showVolumeControls),
            showProgressBar = bool(SettingKeys.PEBBLE_SHOW_PROGRESS_BAR, defaults.showProgressBar),
            showAlbumArt = bool(SettingKeys.PEBBLE_SHOW_ALBUM_ART, defaults.showAlbumArt),
            gestures = gestures.mapIndexed { i, key -> string(key) ?: defaults.gestures[i] },
            menuTimeoutSeconds = settings[SettingKeys.PEBBLE_MENU_TIMEOUT_SECONDS]?.trim()?.toIntOrNull() ?: defaults.menuTimeoutSeconds,
            menu = stringList(SettingKeys.PEBBLE_MENU).ifEmpty { PebbleProtocol.DEFAULT_MENU },
            actions = stringList(SettingKeys.PEBBLE_ACTIONS).ifEmpty { PebbleProtocol.DEFAULT_ACTIONS },
            closeWhenPausedMinutes = settings[SettingKeys.PEBBLE_CLOSE_WHEN_PAUSED_MINUTES]?.trim()?.toIntOrNull() ?: defaults.closeWhenPausedMinutes,
        )
        return mapOf(Key.TYPE to u8(Type.SETTINGS), Key.SETTINGS to PebbleDictionaryItem.Bytes(PebbleProtocol.encodeSettings(s)))
    }

    private fun bool(key: String, default: Boolean): Boolean = when (settings[key]?.trim()) {
        "true" -> true
        "false" -> false
        else -> default
    }

    private fun string(key: String): String? = settings[key]?.trim()?.takeIf { it.startsWith("\"") }?.trim('"')

    private fun stringList(key: String): List<String> = settings[key]?.let {
        runCatching { HocketJson.json.decodeFromString(ListSerializer(String.serializer()), it) }.getOrNull()
    }.orEmpty()

    // -- helpers -------------------------------------------------------------------------------------

    private fun dispatch(command: Command) = core().dispatch(command)

    private suspend fun query(q: app.hocket.core.api.Query): QueryResult? =
        runCatching { core().query(q) }.onFailure { Log.w(TAG, "query failed: ${it.message}") }.getOrNull()

    private suspend fun albumTracks(id: String) = (query(Queries.albumTracks(id)) as? QueryResult.TrackList)?.data.orEmpty()
    private suspend fun albumName(id: String) = (query(Queries.album(id)) as? QueryResult.AlbumDetail)?.data?.name
    private suspend fun playlistName(id: String) = (query(Queries.playlist(id)) as? QueryResult.PlaylistDetail)?.data?.name
    private suspend fun artistName(id: String) = (query(Queries.artist(id)) as? QueryResult.ArtistDetail)?.data?.name
    private suspend fun trackTitle(id: String) = (query(Queries.track(id)) as? QueryResult.TrackDetail)?.data?.title

    private fun num(data: PebbleDictionary, key: UInt): Int = when (val v = data[key]) {
        is PebbleDictionaryItem.UInt32 -> v.value.toInt()
        is PebbleDictionaryItem.Int32 -> v.value
        is PebbleDictionaryItem.UInt16 -> v.value.toInt()
        is PebbleDictionaryItem.Int16 -> v.value.toInt()
        is PebbleDictionaryItem.UInt8 -> v.value.toInt()
        is PebbleDictionaryItem.Int8 -> v.value.toInt()
        else -> 0
    }

    private fun text(data: PebbleDictionary, key: UInt): String = (data[key] as? PebbleDictionaryItem.Text)?.value.orEmpty()

    private fun str(value: String?) = PebbleDictionaryItem.Text(String(PebbleProtocol.utf8Prefix(value.orEmpty(), TEXT_MAX), Charsets.UTF_8))
    private fun u8(v: Int) = PebbleDictionaryItem.UInt8(v.coerceIn(0, 255).toUByte())
    private fun u16(v: Int) = PebbleDictionaryItem.UInt16(v.coerceIn(0, 0xffff).toUShort())
    private fun u32(v: Int) = PebbleDictionaryItem.UInt32(v.coerceAtLeast(0).toUInt())
}
