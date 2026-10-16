package app.hocket.ui.screens.library

import androidx.annotation.StringRes
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.toShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavHostController
import app.hocket.R
import app.hocket.core.Commands
import app.hocket.core.Queries
import app.hocket.core.SettingKeys
import app.hocket.core.api.ActionTarget
import app.hocket.core.api.QueryResult
import app.hocket.core.client.CoreClient
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.nav.Route
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.contentOrNull

/** What tapping a pin does (`library.pins` `tap`). */
enum class PinTap(val id: String, @StringRes val label: Int) {
    Open("open", R.string.library_pin_tap_open),
    Play("play", R.string.library_pin_tap_play),
    Shuffle("shuffle", R.string.library_pin_tap_shuffle);

    companion object {
        fun of(id: String?) = entries.firstOrNull { it.id == id } ?: Open
    }
}

/** The kinds `library.pins` holds (the core's `LIBRARY_PIN_KINDS`). */
enum class PinKind(val id: String, @StringRes val label: Int) {
    Album("album", R.string.kind_album),
    Artist("artist", R.string.kind_artist),
    Playlist("playlist", R.string.kind_playlist),
    Track("track", R.string.kind_song),
    Genre("genre", R.string.kind_genre),
    Filter("filter", R.string.kind_filter);

    /** Play and Shuffle are offered as the tap for albums, artists and playlists only. */
    val choosesTap: Boolean get() = this == Album || this == Artist || this == Playlist

    companion object {
        fun of(id: String) = entries.firstOrNull { it.id == id }
    }
}

/** One entry of `library.pins`. [kind] may be one this build doesn't know (kept, not shown). */
data class PinEntry(val kind: String, val id: String, val tap: String = "open") {
    val pinKind: PinKind? get() = PinKind.of(kind)
    val pinTap: PinTap get() = PinTap.of(tap)
}

object LibraryPins {
    fun parse(value: String?): List<PinEntry> = runCatching {
        (Json.parseToJsonElement(value ?: "[]") as JsonArray).mapNotNull { e ->
            val o = e as? JsonObject ?: return@mapNotNull null
            val kind = (o["kind"] as? JsonPrimitive)?.contentOrNull ?: return@mapNotNull null
            val id = (o["id"] as? JsonPrimitive)?.contentOrNull ?: return@mapNotNull null
            PinEntry(kind, id, (o["tap"] as? JsonPrimitive)?.contentOrNull ?: "open")
        }.distinctBy { it.kind to it.id }
    }.getOrDefault(emptyList())

    fun encode(pins: List<PinEntry>): String = JsonArray(pins.map { p ->
        JsonObject(mapOf("kind" to JsonPrimitive(p.kind), "id" to JsonPrimitive(p.id), "tap" to JsonPrimitive(p.tap)))
    }).toString()

    fun current(client: CoreClient): List<PinEntry> = parse(client.settings.value[SettingKeys.LIBRARY_PINS]?.value)

    fun save(client: CoreClient, pins: List<PinEntry>) = client.dispatch(Commands.setSetting(SettingKeys.LIBRARY_PINS, encode(pins)))

    fun isPinned(client: CoreClient, kind: PinKind, id: String) = current(client).any { it.kind == kind.id && it.id == id }

    /** Pins or unpins; a song plays when tapped, everything else opens (as the core's action does). */
    fun toggle(client: CoreClient, kind: PinKind, id: String) {
        val pins = current(client)
        save(client, if (pins.any { it.kind == kind.id && it.id == id }) pins.filterNot { it.kind == kind.id && it.id == id }
        else pins + PinEntry(kind.id, id, if (kind == PinKind.Track) "play" else "open"))
    }

    fun remove(client: CoreClient, pin: PinEntry): Int {
        val pins = current(client)
        val at = pins.indexOfFirst { it.kind == pin.kind && it.id == pin.id }
        if (at >= 0) save(client, pins.filterIndexed { i, _ -> i != at })
        return at
    }

    /** Puts [pin] back where it was ([at]), for Undo. */
    fun restore(client: CoreClient, pin: PinEntry, at: Int) {
        val pins = current(client).filterNot { it.kind == pin.kind && it.id == pin.id }.toMutableList()
        pins.add(at.coerceIn(0, pins.size), pin)
        save(client, pins)
    }

    fun setTap(client: CoreClient, pin: PinEntry, tap: PinTap) =
        save(client, current(client).map { if (it.kind == pin.kind && it.id == pin.id) it.copy(tap = tap.id) else it })

    fun move(client: CoreClient, pin: PinEntry, by: Int) {
        val pins = current(client).toMutableList()
        val i = pins.indexOfFirst { it.kind == pin.kind && it.id == pin.id }
        val j = i + by
        if (i < 0 || j !in pins.indices) return
        pins.add(j, pins.removeAt(i))
        save(client, pins)
    }
}

/** A pin with what it shows: its title, artwork and kind. */
data class ResolvedPin(val entry: PinEntry, val kind: PinKind, val title: String, val coverArt: String?, val target: ActionTarget?)

/**
 * The pins this build can show, resolved against the library (a pin whose album, artist, playlist,
 * song or filter is gone is left out, not removed: it may be back after a sync).
 */
@Composable
fun rememberResolvedPins(): List<ResolvedPin>? {
    val client = LocalCoreClient.current
    val settings by client.settings.collectAsStateWithLifecycle()
    val filters by client.filters.collectAsStateWithLifecycle()
    val libraryGen by client.libraryGeneration.collectAsStateWithLifecycle()
    val pins = LibraryPins.parse(settings[SettingKeys.LIBRARY_PINS]?.value)
    val resolved by produceState<List<ResolvedPin>?>(null, pins, filters, libraryGen) {
        value = pins.mapNotNull { p ->
            when (p.pinKind) {
                PinKind.Album -> (client.query(Queries.album(p.id)) as? QueryResult.AlbumDetail)?.data?.let { ResolvedPin(p, PinKind.Album, it.name, it.coverArt, Commands.albums(listOf(it.id))) }
                PinKind.Artist -> (client.query(Queries.artist(p.id)) as? QueryResult.ArtistDetail)?.data?.let { ResolvedPin(p, PinKind.Artist, it.name, it.coverArt, Commands.artists(listOf(it.id))) }
                PinKind.Playlist -> (client.query(Queries.playlist(p.id)) as? QueryResult.PlaylistDetail)?.data?.let { ResolvedPin(p, PinKind.Playlist, it.name, it.coverArt, Commands.playlists(listOf(it.id))) }
                PinKind.Track -> (client.query(Queries.track(p.id)) as? QueryResult.TrackDetail)?.data?.let { ResolvedPin(p, PinKind.Track, it.title, it.coverArt, Commands.tracks(listOf(it.id))) }
                PinKind.Genre -> ResolvedPin(p, PinKind.Genre, p.id, null, null)
                PinKind.Filter -> filters.firstOrNull { it.id == p.id }?.let { ResolvedPin(p, PinKind.Filter, it.name, null, null) }
                null -> null
            }
        }
    }
    return resolved
}

/** Each kind's artwork shape: the shape says what a pin is at a glance. */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun pinShape(kind: PinKind): Shape = when (kind) {
    PinKind.Album -> RoundedCornerShape(12.dp)
    PinKind.Artist -> CircleShape
    PinKind.Playlist -> RoundedCornerShape(20.dp)
    PinKind.Track -> RoundedCornerShape(8.dp)
    PinKind.Genre -> MaterialShapes.Flower.toShape()
    PinKind.Filter -> MaterialShapes.Clover4Leaf.toShape()
}

/** Performs a pin's tap: open it, or play / shuffle it. */
fun performPin(client: CoreClient, nav: NavHostController, serverId: String, pin: ResolvedPin, tap: PinTap = pin.entry.pinTap) {
    val id = pin.entry.id
    when (pin.kind) {
        PinKind.Track -> client.dispatch(Commands.playTracks(serverId, listOf(id), 0, pin.title))
        PinKind.Album -> if (tap == PinTap.Open) nav.navigate(Route.Album(id)) else client.dispatch(Commands.playContext(Commands.albumContext(serverId, id, pin.title), shuffle = tap == PinTap.Shuffle))
        PinKind.Artist -> if (tap == PinTap.Open) nav.navigate(Route.Artist(id)) else client.dispatch(Commands.playContext(Commands.artistContext(serverId, id, pin.title), shuffle = tap == PinTap.Shuffle))
        PinKind.Playlist -> if (tap == PinTap.Open) nav.navigate(Route.Playlist(id)) else client.dispatch(Commands.playContext(Commands.playlistContext(serverId, id, pin.title), shuffle = tap == PinTap.Shuffle))
        PinKind.Genre -> nav.navigate(Route.Genre(id))
        PinKind.Filter -> nav.navigate(Route.FilterBuilder(id))
    }
}
