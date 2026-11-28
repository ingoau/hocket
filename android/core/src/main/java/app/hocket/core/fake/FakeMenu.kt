package app.hocket.core.fake

import app.hocket.core.api.ActionDescriptor
import app.hocket.core.api.ActionTarget
import app.hocket.core.api.MenuItem
import app.hocket.core.api.MenuItemInfo
import app.hocket.core.api.MenuLayout
import app.hocket.core.api.MenuScreen
import app.hocket.core.api.MenuSection
import app.hocket.core.api.MenuSection.About
import app.hocket.core.api.MenuSection.Danger
import app.hocket.core.api.MenuSection.GoTo
import app.hocket.core.api.MenuSection.Keep
import app.hocket.core.api.MenuSection.Organise
import app.hocket.core.api.MenuSection.Play
import app.hocket.core.api.MenuSection.Rate
import app.hocket.core.api.MenuSection.Tune
import app.hocket.core.api.MenuTier
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.contentOrNull

/**
 * The fake's copy of the core's item menu model (`crates/hocket-core/src/actions/menu.rs`): the
 * same rows, sections, default tiers, priorities and fold rule, laid over the fake's own actions.
 */
internal object FakeMenu {
    private const val SHORT_LIST_MAX = 6
    private const val NO_FOLD_MAX = 7

    /** One row: [actions] null for an app-handled one (offered when the app lists it). */
    private class Def(val id: String, val section: MenuSection, val folded: Boolean, val priority: Int, val single: Boolean, val actions: List<String>?)

    private val DEFS = listOf(
        Def("play", Play, false, 0, false, listOf("play")),
        Def("playShuffled", Play, false, 0, false, listOf("playShuffled")),
        Def("playNext", Play, false, 0, false, listOf("playNext")),
        Def("addToQueue", Play, false, 0, false, listOf("addToQueue")),
        Def("playLater", Play, false, 0, false, listOf("playLater")),
        Def("rating", Rate, false, 0, false, listOf("rate5", "rate4", "rate3", "rate2", "rate1", "rate0")),
        Def("love", Rate, false, 0, false, listOf("love", "unlove")),
        Def("addToPlaylist", Organise, false, 80, false, listOf("addToPlaylist")),
        Def("stash", Organise, false, 60, false, listOf("stash")),
        Def("pinToLibrary", Organise, true, 30, true, listOf("pinToLibrary", "unpinFromLibrary")),
        Def("addToLibrary", Organise, true, 25, false, listOf("addToLibrary", "removeFromLibrary")),
        Def("restoreSavedQueue", Organise, false, 90, false, listOf("restoreSavedQueue")),
        Def("pinSavedQueue", Organise, false, 70, false, listOf("pinSavedQueue", "unpinSavedQueue")),
        Def("saveQueueAsPlaylist", Organise, false, 65, false, listOf("saveQueueAsPlaylist")),
        Def("goToAlbum", GoTo, false, 95, true, listOf("goToAlbum")),
        Def("goToArtist", GoTo, false, 100, true, listOf("goToArtist")),
        Def("download", Keep, false, 20, false, listOf("download", "unpin")),
        Def("share", Keep, false, 50, false, null),
        Def("preview", About, true, 15, true, listOf("preview")),
        Def("info", About, true, 12, true, null),
        Def("genius", About, true, 10, true, null),
        Def("lyricsTiming", About, true, 8, true, null),
        Def("snooze", Tune, true, 5, false, listOf("snooze", "unsnooze")),
        Def("removeFromQueue", Danger, false, 0, false, listOf("removeFromQueue")),
        Def("removeFromPlaylist", Danger, false, 0, false, listOf("removeFromPlaylist")),
        Def("deleteSavedQueue", Danger, false, 0, false, listOf("deleteSavedQueue")),
        Def("deletePlaylist", Danger, false, 0, false, listOf("deletePlaylist")),
    )

    private fun choices(d: Def): List<MenuTier> = when (d.section) {
        Play -> emptyList()
        Rate, Danger -> listOf(MenuTier.Shown, MenuTier.Hidden)
        else -> listOf(MenuTier.Shown, MenuTier.More, MenuTier.Hidden)
    }

    private fun defaultTier(d: Def) = if (d.folded) MenuTier.More else MenuTier.Shown

    private fun tierOf(s: String?): MenuTier? = MenuTier.entries.firstOrNull { it.string == s }

    /**
     * `menu.overrides`, skipping a row this build doesn't know or a tier it doesn't allow there (a
     * newer device's): they stay in the stored map ([withOverride] keeps them).
     */
    fun parseOverrides(v: JsonObject?): Map<String, MenuTier> = v.orEmpty().mapNotNull { (id, t) ->
        val d = DEFS.firstOrNull { it.id == id } ?: return@mapNotNull null
        val tier = tierOf((t as? JsonPrimitive)?.contentOrNull) ?: return@mapNotNull null
        if (tier in choices(d)) id to tier else null
    }.toMap()

    /** `menu.overrides` with [id] at [tier] (its default removes it); null when that isn't allowed. */
    fun withOverride(current: JsonObject?, id: String, tier: MenuTier): JsonObject? {
        val d = DEFS.firstOrNull { it.id == id } ?: return null
        if (tier !in choices(d)) return null
        val map = current.orEmpty().toMutableMap()
        if (tier == defaultTier(d)) map.remove(id) else map[id] = JsonPrimitive(tier.string)
        return JsonObject(map)
    }

    fun itemsInfo(overrides: Map<String, MenuTier>): List<MenuItemInfo> = DEFS.filter { choices(it).isNotEmpty() }.map { d ->
        MenuItemInfo(d.id, d.section, defaultTier(d), overrides[d.id] ?: defaultTier(d), choices(d), d.actions == null)
    }

    private fun dropped(id: String, screen: MenuScreen) = when (id) {
        "preview" -> screen == MenuScreen.NowPlaying
        "goToAlbum" -> screen == MenuScreen.AlbumPage
        "goToArtist" -> screen == MenuScreen.ArtistPage
        "removeFromPlaylist" -> screen != MenuScreen.Playlist
        else -> false
    }

    private fun count(target: ActionTarget) = when (target) {
        is ActionTarget.Tracks -> target.data.tracks.size
        is ActionTarget.Albums -> target.data.ids.size
        is ActionTarget.Artists -> target.data.ids.size
        is ActionTarget.Playlists -> target.data.ids.size
        is ActionTarget.QueueItems -> target.data.keys.size
        is ActionTarget.SavedQueue -> 1
        ActionTarget.None -> 0
    }

    /** As `menu_layout`: [available] are the actions the target is offered. */
    fun layout(available: List<ActionDescriptor>, target: ActionTarget, screen: MenuScreen, appItems: List<String>, extras: Int, overrides: Map<String, MenuTier>): MenuLayout {
        val byId = available.associateBy { it.id }
        val n = count(target)
        val rows = DEFS.mapNotNull { d ->
            val tier = overrides[d.id]
            if (tier == MenuTier.Hidden || dropped(d.id, screen) || (d.single && n > 1)) return@mapNotNull null
            val actions = if (d.actions == null) {
                if (d.id !in appItems) return@mapNotNull null
                emptyList()
            } else d.actions.mapNotNull { byId[it] }.ifEmpty { return@mapNotNull null }
            Triple(d, actions, tier)
        }
        fun foldable(d: Def) = d.section != Play && d.section != Rate && d.section != Danger
        // A row the user put under More stays there, however short the menu.
        val folded = BooleanArray(rows.size) { foldable(rows[it].first) && rows[it].third == MenuTier.More }
        if (rows.count { foldable(it.first) } + extras > NO_FOLD_MAX) {
            val pinned = rows.count { foldable(it.first) && it.third == MenuTier.Shown }
            var room = (SHORT_LIST_MAX - extras - pinned).coerceAtLeast(0)
            rows.forEachIndexed { i, (d, _, t) ->
                if (foldable(d)) folded[i] = when (t) { MenuTier.Shown -> false; null -> d.folded; else -> true }
            }
            rows.indices.filter { foldable(rows[it].first) && rows[it].third == null && !rows[it].first.folded }
                .sortedByDescending { rows[it].first.priority }
                .forEach { i -> if (room > 0) room-- else folded[i] = true }
            // A More holding one row is a tap for nothing, unless the user put it there.
            folded.indices.filter { folded[it] }.singleOrNull()?.let { i -> if (rows[i].third != MenuTier.More) folded[i] = false }
        }
        return MenuLayout(rows.mapIndexed { i, (d, actions, _) -> MenuItem(d.id, d.section, folded[i], actions) })
    }

    /** As the core's `overrides_valid`: the shape only, an object of "shown" | "more" | "hidden". */
    fun shapeValid(json: String): Boolean = parse(json)?.values?.all { v -> tierOf((v as? JsonPrimitive)?.takeIf { it.isString }?.contentOrNull) != null } == true

    fun parse(json: String?): JsonObject? = runCatching { app.hocket.core.HocketJson.json.parseToJsonElement(json ?: "{}") as? JsonObject }.getOrNull()
}
