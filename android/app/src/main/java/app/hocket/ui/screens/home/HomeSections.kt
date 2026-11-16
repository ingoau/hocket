package app.hocket.ui.screens.home

import androidx.annotation.StringRes
import app.hocket.R
import app.hocket.core.Commands
import app.hocket.core.SettingKeys
import app.hocket.core.api.HomeShelf
import app.hocket.core.client.CoreClient
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.contentOrNull

/** How a Home section lays out its items. */
enum class SectionStyle(val id: String, @StringRes val label: Int) {
    Row("row", R.string.home_style_row),
    Grid("grid", R.string.home_style_grid),
    List("list", R.string.home_style_list),
    Tile("tile", R.string.home_style_tile),
    Hero("hero", R.string.home_style_hero);

    companion object {
        fun of(id: String?): SectionStyle? = entries.firstOrNull { it.id == id }
    }
}

/**
 * The sections Home can show (`home.sections`, synced). The same ids, defaults and styles are the
 * desktop's (`desktop/src/renderer/lib/home-sections.ts`); a section either device doesn't know is
 * kept in the stored list but not shown. A [pinned] section always comes first: it can be switched
 * off but never moved.
 */
enum class HomeSection(
    val id: String,
    @StringRes val title: Int,
    @StringRes val description: Int,
    val styles: kotlin.collections.List<SectionStyle>,
    val defaultOn: Boolean,
    val defaultStyle: SectionStyle,
    val pinned: Boolean = false,
) {
    Spotlight("spotlight", R.string.home_section_spotlight, R.string.home_section_spotlight_desc, listOf(SectionStyle.Hero), true, SectionStyle.Hero, pinned = true),
    Rediscover("rediscover", R.string.home_section_rediscover, R.string.home_section_rediscover_desc, listOf(SectionStyle.Row, SectionStyle.Grid, SectionStyle.List), true, SectionStyle.Row),
    Filters("filters", R.string.home_section_filters, R.string.home_section_filters_desc, listOf(SectionStyle.Row, SectionStyle.Grid, SectionStyle.List), true, SectionStyle.Row),
    FavouriteArtists("favouriteArtists", R.string.home_section_favourite_artists, R.string.home_section_favourite_artists_desc, listOf(SectionStyle.Row, SectionStyle.Grid), true, SectionStyle.Row),
    TopGenre("topGenre", R.string.home_section_top_genre, R.string.home_section_top_genre_desc, listOf(SectionStyle.Row, SectionStyle.Grid, SectionStyle.List), true, SectionStyle.Grid),
    MoreFromArtist("moreFromArtist", R.string.home_section_more_from_artist, R.string.home_section_more_from_artist_desc, listOf(SectionStyle.List, SectionStyle.Row), false, SectionStyle.List),
    OnRepeat("onRepeat", R.string.home_section_on_repeat, R.string.home_section_on_repeat_desc, listOf(SectionStyle.Row, SectionStyle.Grid, SectionStyle.List), false, SectionStyle.Row),
    RecentQueues("recentQueues", R.string.home_section_recent_queues, R.string.home_section_recent_queues_desc, listOf(SectionStyle.List, SectionStyle.Row), false, SectionStyle.List),
    Stats("stats", R.string.home_section_stats, R.string.home_section_stats_desc, listOf(SectionStyle.Tile), false, SectionStyle.Tile);

    companion object {
        fun of(id: String): HomeSection? = entries.firstOrNull { it.id == id }
    }
}

/** The styles a shelf can be drawn in. */
val SHELF_STYLES: kotlin.collections.List<SectionStyle> = listOf(SectionStyle.Row, SectionStyle.Grid, SectionStyle.List)

/** One entry of `home.sections`. [id] may name a section this build doesn't know. */
data class SectionPref(val id: String, val on: Boolean, val style: String?) {
    val section: HomeSection? get() = HomeSection.of(id)
    /** The core's shelf this names (`shelf:<shelf id>`), when the core supplied it. */
    val shelf: HomeShelf? get() = HomeSections.shelfOf(id)
    /** A built-in section or a shelf the core supplied: one Home can draw. */
    val known: Boolean get() = section != null || shelf != null
    /** The styles it can be drawn in. */
    val styles: kotlin.collections.List<SectionStyle> get() = section?.styles ?: if (shelf != null) SHELF_STYLES else emptyList()
    /** The style to draw: the stored one when the section offers it, else its default. */
    val resolvedStyle: SectionStyle get() = section?.let { s -> SectionStyle.of(style)?.takeIf { it in s.styles } ?: s.defaultStyle }
        ?: SectionStyle.of(style)?.takeIf { shelf != null && it in SHELF_STYLES } ?: SectionStyle.Row
}

object HomeSections {
    /** The built-in sections' defaults. */
    val DEFAULT: kotlin.collections.List<SectionPref> = HomeSection.entries.map { SectionPref(it.id, it.defaultOn, it.defaultStyle.id) }

    /**
     * The shelves the core supplied last (`Query::HomeShelves`: Continue listening and each
     * account's Recently added, Most played and Recently played). Home sets them as they arrive;
     * they're sections named `shelf:<shelf id>`, ordered and hidden like the rest. The desktop
     * keeps the same list (`desktop/src/renderer/lib/home-sections.ts`).
     */
    @Volatile var shelves: kotlin.collections.List<HomeShelf> = emptyList()

    fun shelfSectionId(shelfId: String) = "shelf:$shelfId"
    private const val CONTINUE_LISTENING = "shelf:hocket/continueListening"

    fun shelfOf(id: String): HomeShelf? = if (id.startsWith("shelf:")) shelves.firstOrNull { shelfSectionId(it.id) == id } else null

    /** The default list with the shelves: Continue listening after Spotlight, the accounts' shelves after the sections that are on. */
    fun defaults(): kotlin.collections.List<SectionPref> {
        val shelfPrefs = shelves.map { SectionPref(shelfSectionId(it.id), it.defaultOn, SectionStyle.Row.id) }
        val (cont, rest) = shelfPrefs.partition { it.id == CONTINUE_LISTENING }
        val (pinned, others) = DEFAULT.partition { it.section?.pinned == true }
        return pinned + cont + others.filter { it.on } + rest + others.filter { !it.on }
    }

    /**
     * The stored list as the editor and Home use it: every stored entry in order (unknown ids kept,
     * so a newer device's sections survive this one's edits), then any known section the list
     * doesn't mention yet (added in a later release), off. A pinned section goes first, and one
     * the list doesn't mention yet is on (it arrived switched on). An empty or unreadable value is the default.
     */
    fun parse(value: String?): kotlin.collections.List<SectionPref> {
        val stored = runCatching {
            (Json.parseToJsonElement(value ?: "[]") as JsonArray).mapNotNull { e ->
                val o = e as? JsonObject ?: return@mapNotNull null
                val id = (o["id"] as? JsonPrimitive)?.contentOrNull ?: return@mapNotNull null
                val on = (o["on"] as? JsonPrimitive)?.booleanOrNull ?: return@mapNotNull null
                SectionPref(id, on, (o["style"] as? JsonPrimitive)?.contentOrNull)
            }.distinctBy { it.id }
        }.getOrDefault(emptyList())
        if (stored.isEmpty()) return defaults()
        val missing = HomeSection.entries.filter { s -> stored.none { it.id == s.id } }.map { SectionPref(it.id, it.pinned && it.defaultOn, it.defaultStyle.id) } +
            // A shelf the list doesn't mention yet (a new account's) arrives as it defaults.
            shelves.map { SectionPref(shelfSectionId(it.id), it.defaultOn, SectionStyle.Row.id) }.filter { s -> stored.none { it.id == s.id } }
        val all = stored + missing
        val (pinned, rest) = all.partition { it.section?.pinned == true }
        return pinned + rest
    }

    fun encode(prefs: kotlin.collections.List<SectionPref>): String = JsonArray(prefs.map { p ->
        JsonObject(buildMap {
            put("id", JsonPrimitive(p.id))
            put("on", JsonPrimitive(p.on))
            p.style?.let { put("style", JsonPrimitive(it)) }
        })
    }).toString()

    fun current(client: CoreClient): kotlin.collections.List<SectionPref> = parse(client.settings.value[SettingKeys.HOME_SECTIONS]?.value)

    fun save(client: CoreClient, prefs: kotlin.collections.List<SectionPref>) {
        // The default is stored as [], so a later release's new defaults reach it.
        val value = if (prefs == defaults()) "[]" else encode(prefs)
        client.dispatch(Commands.setSetting(SettingKeys.HOME_SECTIONS, value))
    }

    /** [id] moved by [by] places among the shown sections (hidden ones keep their slots). */
    fun moveShown(prefs: kotlin.collections.List<SectionPref>, id: String, by: Int): kotlin.collections.List<SectionPref> {
        val shown = prefs.filter { it.on && it.known }
        val i = shown.indexOfFirst { it.id == id }
        val other = shown.getOrNull(i + by) ?: return prefs
        if (pinnedId(id) || pinnedId(other.id)) return prefs
        val list = prefs.toMutableList()
        val from = list.indexOfFirst { it.id == id }
        val moved = list.removeAt(from)
        val to = list.indexOfFirst { it.id == other.id }
        list.add(if (by > 0) to + 1 else to, moved)
        return list
    }

    fun move(prefs: kotlin.collections.List<SectionPref>, from: Int, to: Int): kotlin.collections.List<SectionPref> {
        if (from == to || from !in prefs.indices || to !in prefs.indices) return prefs
        if (pinnedId(prefs[from].id) || pinnedId(prefs[to].id)) return prefs
        return prefs.toMutableList().apply { add(to, removeAt(from)) }
    }

    private fun pinnedId(id: String) = HomeSection.of(id)?.pinned == true

    fun update(prefs: kotlin.collections.List<SectionPref>, id: String, change: (SectionPref) -> SectionPref) = prefs.map { if (it.id == id) change(it) else it }
}
