package app.hocket.ui.nav

import kotlinx.serialization.Serializable

/** Type-safe navigation routes. */
sealed interface Route {
    @Serializable data object Home : Route
    @Serializable data class Library(val tab: Int = 0) : Route
    @Serializable data object Search : Route
    @Serializable data object Settings : Route
    @Serializable data object Downloads : Route
    /** The library's lists on their own (bottom-bar places). */
    @Serializable data object Albums : Route
    @Serializable data object Artists : Route
    @Serializable data object Playlists : Route
    @Serializable data object Songs : Route
    @Serializable data object Genres : Route
    @Serializable data object AvailableOffline : Route
    @Serializable data object Filters : Route
    @Serializable data class FilterBuilder(val id: String? = null) : Route
    @Serializable data object Stats : Route
    @Serializable data class Album(val id: String) : Route
    @Serializable data class Artist(val id: String) : Route
    @Serializable data class Playlist(val id: String) : Route
    @Serializable data class Genre(val name: String) : Route
    @Serializable data object SavedQueues : Route
    @Serializable data object AudioSettings : Route
    @Serializable data object TranscodingSettings : Route
    @Serializable data object ConnectSettings : Route
    @Serializable data object CustomiseSettings : Route
    @Serializable data object About : Route
    @Serializable data object SettingsAccount : Route
    @Serializable data object SettingsAppearance : Route
    @Serializable data object SettingsPlayback : Route
    @Serializable data object SettingsDownloads : Route
    @Serializable data object SettingsLyrics : Route
    @Serializable data object SettingsLibrary : Route
    @Serializable data object SettingsBattery : Route
    @Serializable data object SettingsBackup : Route
}

/**
 * The places the phone's bottom bar can hold, chosen and ordered by the user (a device-local app
 * preference, [app.hocket.AppPrefs]). Settings is never one: it lives behind the account button in
 * the main destinations' top app bars. [canonicalActionId] is the registry action for the place,
 * used to keep the core's `sidebar` surface in step.
 */
enum class NavItem(val id: String, val canonicalActionId: String) {
    Home("home", "navigateHome"),
    Search("search", "findInList"),
    Library("library", "navigateLibrary"),
    Playlists("playlists", "navigatePlaylists"),
    Artists("artists", "navigateArtists"),
    Albums("albums", "navigateAlbums"),
    Songs("songs", "navigateTracks"),
    Genres("genres", "navigateGenres"),
    RecentQueues("recentQueues", "navigateRecent"),
    Stats("stats", "navigateStats"),
    Downloads("downloads", "navigateDownloads"),
    Filters("filters", "navigateFilters");

    companion object {
        val DEFAULT = listOf(Home, Search, Library)

        /** Material's navigation bar holds at most five destinations; one is pointless. */
        const val MIN = 2
        const val MAX = 5

        /**
         * The bar from stored ids, migrating older lists: unknown ids (the old `settings` item)
         * are dropped, duplicates collapse, the rest keep their order and are capped at [MAX];
         * fewer than [MIN] left (or nothing stored) is the default bar.
         */
        fun fromIds(ids: List<String>): List<NavItem> {
            val known = ids.mapNotNull { id -> entries.firstOrNull { it.id == id } }.distinct().take(MAX)
            return if (known.size < MIN) DEFAULT else known
        }
    }
}
