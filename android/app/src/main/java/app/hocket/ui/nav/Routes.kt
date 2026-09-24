package app.hocket.ui.nav

import kotlinx.serialization.Serializable

/** Type-safe navigation routes. */
sealed interface Route {
    @Serializable data object Home : Route
    @Serializable data class Library(val tab: Int = 0) : Route
    @Serializable data object Search : Route
    @Serializable data object Settings : Route
    @Serializable data object Downloads : Route
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

/** Navigation items the user can choose and order (setting `ui.navItems`). */
enum class NavItem(val id: String, val canonicalActionId: String) {
    Home("home", "navigateHome"), Library("library", "navigateAlbums"), Search("search", "findInList"), Downloads("downloads", "navigateDownloads"),
    Filters("filters", "navigateFilters"), Stats("stats", "navigateStats"), Settings("settings", "navigateSettings");

    companion object {
        val DEFAULT = listOf(Home, Library, Search, Settings)
        fun fromIds(ids: List<String>): List<NavItem> = ids.mapNotNull { id -> entries.firstOrNull { it.id == id } }.ifEmpty { DEFAULT }
    }
}
