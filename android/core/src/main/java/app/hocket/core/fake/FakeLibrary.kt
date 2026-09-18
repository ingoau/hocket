package app.hocket.core.fake

import app.hocket.core.api.*
import kotlin.random.Random

/**
 * A deterministic, plausible library for the fake core: artists, albums, tracks, genres, playlists
 * and lyrics, generated from a seed so previews and tests are stable. Mutable where the real mirror
 * is mutable (ratings, loves, play counts, playlists, offline state).
 */
class FakeLibrary(seed: Long = 42L, val serverId: ServerId = "fake-server") {
    private val rng = Random(seed)

    val artists = ArrayList<Artist>()
    val albums = ArrayList<Album>()
    val tracks = ArrayList<Track>()
    val genres = ArrayList<Genre>()
    val playlists = ArrayList<Playlist>()
    /** Playlist id -> ordered track ids. */
    val playlistTracks = HashMap<PlaylistId, MutableList<TrackId>>()
    val lyricsByTrack = HashMap<TrackId, Lyrics?>()

    private val albumTracksIndex = HashMap<AlbumId, MutableList<Int>>()
    private val trackIndex = HashMap<TrackId, Int>()
    private val albumIndex = HashMap<AlbumId, Int>()
    private val artistIndex = HashMap<ArtistId, Int>()

    init {
        val genreNames = listOf("Ambient", "Jazz", "Post-rock", "Electronic", "Folk", "Hip hop", "Classical", "Indie", "Soul", "Techno", "Shoegaze", "Dub")
        val now = 1_758_000_000_000.0
        val nArtists = 48
        for (a in 0 until nArtists) {
            val name = artistName(a)
            artists += Artist("ar$a", serverId, name, 0u, 0u, "ar$a", null, rng.nextInt(10) == 0, null,
                if (a % 5 == 0) "$name formed in ${1980 + rng.nextInt(40)} and released a string of records that quietly redefined the scene." else null)
            artistIndex["ar$a"] = a
        }
        var albumNo = 0
        var trackNo = 0
        for (a in 0 until nArtists) {
            val albumCount = 1 + rng.nextInt(4)
            for (b in 0 until albumCount) {
                val albumId = "al$albumNo"
                val year = 1975 + rng.nextInt(50)
                val genre = genreNames[rng.nextInt(genreNames.size)]
                val songCount = 6 + rng.nextInt(9)
                var dur = 0u
                val list = ArrayList<Int>()
                for (t in 0 until songCount) {
                    val id = "t$trackNo"
                    val duration = (120_000 + rng.nextInt(360_000)).toUInt()
                    dur += duration
                    val rating = if (rng.nextInt(3) == 0) (1 + rng.nextInt(5)).toUInt() else 0u
                    val plays = if (rng.nextInt(4) == 0) rng.nextInt(60).toUInt() else 0u
                    tracks += Track(
                        id = id, serverId = serverId, title = trackTitle(trackNo), albumId = albumId, album = albumTitle(albumNo),
                        artistId = "ar$a", artist = artists[a].name, albumArtist = artists[a].name, trackNumber = (t + 1).toUInt(),
                        discNumber = 1u, year = year.toUInt(), genre = genre, durationMs = duration, bitRate = if (rng.nextBoolean()) 320u else 1000u,
                        sampleRate = 44100u, bitDepth = 16u, channels = 2u, suffix = if (rng.nextInt(3) == 0) "mp3" else "flac",
                        contentType = null, sizeBytes = duration.toDouble() * 40, path = null, coverArt = albumId, rating = rating,
                        loved = rng.nextInt(6) == 0, playCount = plays,
                        lastPlayed = if (plays > 0u) now - rng.nextInt(30) * 86_400_000.0 else null,
                        created = now - rng.nextInt(900) * 86_400_000.0, replayGain = ReplayGain(-6.0 + rng.nextDouble() * 4, 0.98, -5.5, 0.99),
                        sonic = SonicAttributes(70.0 + rng.nextInt(110), listOf("C", "D", "F#", "A", "Bb")[rng.nextInt(5)], rng.nextDouble(), listOf("calm", "bright", "dark", "warm")[rng.nextInt(4)], rng.nextDouble(), rng.nextDouble()),
                        offline = when (rng.nextInt(10)) { 0 -> OfflineState.Downloaded; 1 -> OfflineState.Cached; else -> OfflineState.None },
                        musicBrainzId = null, explicit = rng.nextInt(12) == 0, comment = null,
                    )
                    trackIndex[id] = tracks.size - 1
                    list += tracks.size - 1
                    trackNo++
                }
                albumTracksIndex[albumId] = list
                albums += Album(
                    id = albumId, serverId = serverId, name = albumTitle(albumNo), artistId = "ar$a", artist = artists[a].name, year = year.toUInt(),
                    genre = genre, songCount = songCount.toUInt(), durationMs = dur, coverArt = albumId, rating = 0u, loved = rng.nextInt(8) == 0,
                    playCount = rng.nextInt(20).toUInt(), created = now - rng.nextInt(900) * 86_400_000.0,
                    lastPlayed = if (rng.nextBoolean()) now - rng.nextInt(60) * 86_400_000.0 else null, isCompilation = false, musicBrainzId = null,
                    replayGain = null, offline = OfflineState.None,
                )
                albumIndex[albumId] = albums.size - 1
                albumNo++
            }
            artists[a] = artists[a].copy(albumCount = albumCount.toUInt(), songCount = albumTracksIndex.filterKeys { albums[albumIndex[it]!!].artistId == "ar$a" }.values.sumOf { it.size }.toUInt())
        }
        for (g in genreNames) {
            val songs = tracks.count { it.genre == g }
            val albs = albums.count { it.genre == g }
            if (songs > 0) genres += Genre(g, songs.toUInt(), albs.toUInt())
        }
        val playlistNames = listOf("Late night", "Running", "Sunday morning", "Focus", "Discoveries 2025", "Loved recently", "Road trip", "Cooking")
        playlistNames.forEachIndexed { i, name ->
            val id = "pl$i"
            val ids = (0 until 12 + rng.nextInt(30)).map { tracks[rng.nextInt(tracks.size)].id }.distinct().toMutableList()
            playlistTracks[id] = ids
            val dur = ids.sumOf { track(it)!!.durationMs.toLong() }.toUInt()
            playlists += Playlist(id, serverId, name, if (i == 1) "Keep the pace up" else null, "me", i % 3 == 0, ids.size.toUInt(), dur,
                track(ids.first())!!.coverArt, now - i * 86_400_000.0 * 9, now - i * 86_400_000.0, isSmart = i == 5, isMine = i != 4, offline = if (i == 0) OfflineState.Downloaded else OfflineState.None)
        }
        // Lyrics: every third track syllable-synced, every third line-synced, some unsynced, rest none.
        tracks.forEachIndexed { i, t ->
            lyricsByTrack[t.id] = when (i % 4) {
                0 -> syllableLyrics(t)
                1 -> lineLyrics(t)
                2 -> if (i % 8 == 2) unsyncedLyrics(t) else null
                else -> null
            }
        }
    }

    fun track(id: TrackId): Track? = trackIndex[id]?.let { tracks[it] }
    fun album(id: AlbumId): Album? = albumIndex[id]?.let { albums[it] }
    fun artist(id: ArtistId): Artist? = artistIndex[id]?.let { artists[it] }
    fun albumTracks(id: AlbumId): List<Track> = albumTracksIndex[id]?.map { tracks[it] } ?: emptyList()
    fun artistAlbums(id: ArtistId): List<Album> = albums.filter { it.artistId == id }
    fun playlist(id: PlaylistId): Playlist? = playlists.firstOrNull { it.id == id }
    fun genreTracks(name: String): List<Track> = tracks.filter { it.genre == name }

    fun updateTrack(id: TrackId, f: (Track) -> Track) {
        trackIndex[id]?.let { tracks[it] = f(tracks[it]) }
    }

    fun updateAlbum(id: AlbumId, f: (Album) -> Album) {
        albumIndex[id]?.let { albums[it] = f(albums[it]) }
    }

    fun updateArtist(id: ArtistId, f: (Artist) -> Artist) {
        artistIndex[id]?.let { artists[it] = f(artists[it]) }
    }

    fun refreshPlaylist(id: PlaylistId) {
        val i = playlists.indexOfFirst { it.id == id }
        if (i < 0) return
        val ids = playlistTracks[id] ?: mutableListOf()
        playlists[i] = playlists[i].copy(songCount = ids.size.toUInt(), durationMs = ids.sumOf { track(it)?.durationMs?.toLong() ?: 0L }.toUInt(),
            coverArt = ids.firstOrNull()?.let { track(it)?.coverArt }, changed = 1_758_000_000_000.0)
    }

    fun sorted(list: List<Track>, sort: SortOrder, descending: Boolean): List<Track> {
        val s = when (sort) {
            SortOrder.Default, SortOrder.Title -> list.sortedBy { it.title.lowercase() }
            SortOrder.Artist -> list.sortedWith(compareBy({ it.artist?.lowercase() }, { it.album }, { it.trackNumber }))
            SortOrder.Album -> list.sortedWith(compareBy({ it.album?.lowercase() }, { it.discNumber }, { it.trackNumber }))
            SortOrder.Year -> list.sortedBy { it.year }
            SortOrder.DateAdded -> list.sortedByDescending { it.created }
            SortOrder.Rating -> list.sortedByDescending { it.rating }
            SortOrder.PlayCount -> list.sortedByDescending { it.playCount }
            SortOrder.Duration -> list.sortedBy { it.durationMs }
            SortOrder.Random -> list.shuffled(Random(7))
            SortOrder.Bpm -> list.sortedBy { it.sonic?.bpm }
            SortOrder.Energy -> list.sortedBy { it.sonic?.energy }
        }
        return if (descending) s.reversed() else s
    }

    fun sortedAlbums(list: List<Album>, sort: SortOrder, descending: Boolean): List<Album> {
        val s = when (sort) {
            SortOrder.Default, SortOrder.Title, SortOrder.Album -> list.sortedBy { it.name.lowercase() }
            SortOrder.Artist -> list.sortedWith(compareBy({ it.artist?.lowercase() }, { it.year }))
            SortOrder.Year -> list.sortedBy { it.year }
            SortOrder.DateAdded -> list.sortedByDescending { it.created }
            SortOrder.Rating -> list.sortedByDescending { it.rating }
            SortOrder.PlayCount -> list.sortedByDescending { it.playCount }
            SortOrder.Duration -> list.sortedBy { it.durationMs }
            SortOrder.Random -> list.shuffled(Random(7))
            SortOrder.Bpm, SortOrder.Energy -> list
        }
        return if (descending) s.reversed() else s
    }

    // -- generators ---------------------------------------------------------------------------------
    private fun artistName(i: Int): String {
        val first = listOf("Silver", "Hollow", "Neon", "Quiet", "Paper", "Iron", "Velvet", "Northern", "Glass", "Wild", "Amber", "Low")
        val second = listOf("Harbour", "Signals", "Orchard", "Machines", "Tide", "Choir", "Weather", "Lanterns", "Fields", "Static", "Meridian", "Company")
        return if (i % 7 == 3) listOf("Ada Marsh", "Jonah Reyes", "Ines Kalla", "Theo Lund", "Mara Okafor", "Elias Voss", "Nour Haddad")[i % 7] else "${first[i % first.size]} ${second[(i * 5) % second.size]}"
    }

    private fun albumTitle(i: Int): String {
        val w = listOf("Arrivals", "Slow Light", "Everything After", "Small Hours", "Coastline", "Field Notes", "The Long Way", "Halfway Home", "Signal Fire", "Undertow", "Cartography", "Blue Hour", "Weather Systems", "Interior", "Distances", "Echo Park")
        return if (i % 9 == 4) "${w[i % w.size]} (Deluxe)" else w[(i * 7) % w.size].let { if (i > 15) "$it ${listOf("II", "III", "IV", "")[i % 4]}".trim() else it }
    }

    private fun trackTitle(i: Int): String {
        val w = listOf("Open Water", "Paper Boats", "Nightshift", "All the Way Down", "Gardens", "Lighthouse", "Every Other Day", "Tallest Building", "Second Nature", "Static", "Half Moon", "Copper", "Slow Motion", "Last Train", "Undone", "Wintering", "Cascade", "Low Tide", "Circles", "Empty Rooms", "Marigold", "Airplane Mode", "Skylines", "Fault Lines")
        return w[(i * 11) % w.size].let { if (i % 13 == 0) "$it (Reprise)" else it }
    }

    private val verses = listOf(
        "We drove out past the harbour lights", "And the radio was only static", "You said the city never sleeps", "But tonight it's holding its breath",
        "Every window was a photograph", "Of someone else's summer", "I kept the change in my coat pocket", "Just to hear it when I walk",
        "Open water, open water", "Take me somewhere I can float", "All the lanterns on the shoreline", "Are counting down to morning",
    )

    private fun syllableLyrics(t: Track): Lyrics {
        val lines = ArrayList<LyricLine>()
        var cursor = 8_000L
        val step = ((t.durationMs.toLong() - 16_000L) / verses.size).coerceAtLeast(3_000L)
        verses.forEachIndexed { li, text ->
            val words = text.split(" ")
            val lineDur = (step * 0.8).toLong()
            val syllables = ArrayList<LyricSyllable>()
            var sc = cursor
            val each = lineDur / words.size
            for ((wi, word) in words.withIndex()) {
                // Split longer words into two syllables so the sweep has intra-word joins.
                if (word.length > 5) {
                    val cut = word.length / 2
                    syllables += LyricSyllable(word.substring(0, cut), sc.toUInt(), (sc + each / 2).toUInt(), joined = true)
                    syllables += LyricSyllable(word.substring(cut), (sc + each / 2).toUInt(), (sc + each).toUInt(), joined = wi == words.lastIndex)
                } else {
                    syllables += LyricSyllable(word, sc.toUInt(), (sc + each).toUInt(), joined = wi == words.lastIndex)
                }
                sc += each
            }
            val agent = if (li in 8..11) "v2" else "v1"
            val background = li == 9
            lines += LyricLine(cursor.toUInt(), (cursor + lineDur).toUInt(), text, syllables, agent, background, if (li % 4 == 0) "(translation of line ${li + 1})" else null)
            cursor += step
        }
        return Lyrics(t.id, LyricsTier.Syllable, "en", t.artist, t.title, listOf(LyricsAgent("v1", null, 0u), LyricsAgent("v2", "Duet", 1u)), lines, LyricsSource.Server, 0)
    }

    private fun lineLyrics(t: Track): Lyrics {
        val lines = ArrayList<LyricLine>()
        var cursor = 5_000L
        val step = ((t.durationMs.toLong() - 10_000L) / verses.size).coerceAtLeast(3_000L)
        for (text in verses) {
            lines += LyricLine(cursor.toUInt(), null, text, emptyList(), null, false, null)
            cursor += step
        }
        return Lyrics(t.id, LyricsTier.Line, "en", null, null, emptyList(), lines, LyricsSource.Server, 0)
    }

    private fun unsyncedLyrics(t: Track): Lyrics =
        Lyrics(t.id, LyricsTier.Unsynced, "en", null, null, emptyList(), verses.map { LyricLine(null, null, it, emptyList(), null, false, null) }, LyricsSource.Embedded, 0)
}
