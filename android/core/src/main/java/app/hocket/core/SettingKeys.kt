package app.hocket.core

/**
 * The core's settings registry keys (`crates/hocket-core/src/settings/registry.rs`). Values are JSON
 * strings. Anything the app needs that is not here is an app-local preference, never a core setting
 * (the core refuses unknown keys).
 */
object SettingKeys {
    const val QUEUE_MODE = "queue.mode"
    const val QUEUE_SAVED_CAP = "queue.savedCap"
    const val QUEUE_HISTORY_CAP = "queue.historyCap"
    const val AUTOPLAY_SETTINGS = "autoplay.settings"
    const val AUDIO_SETTINGS = "audio.settings"
    const val TRANSCODING_PROFILES = "transcoding.profiles"
    const val LYRICS_EXTERNAL_ENABLED = "lyrics.external.enabled"
    const val LYRICS_EXTERNAL_PROVIDER = "lyrics.external.provider"
    const val LYRICS_DEFAULT_OFFSET_MS = "lyrics.defaultOffsetMs"
    const val LYRICS_SHOW_TRANSLATIONS = "lyrics.showTranslations"
    const val RATINGS_LOVE_BRIDGE_ENABLED = "ratings.loveBridge.enabled"
    const val RATINGS_LOVE_BRIDGE_THRESHOLD = "ratings.loveBridge.threshold"
    const val BATTERY_AUTO_ENGAGE = "battery.autoEngage"
    const val BATTERY_LYRICS_FPS = "battery.lyricsFps"
    const val BATTERY_SMALL_ARTWORK = "battery.smallArtwork"
    const val BATTERY_PAUSE_PREFETCH = "battery.pausePrefetch"
    const val DISPLAY_ANIMATED_BACKGROUND = "display.animatedBackground"
    const val DISPLAY_LYRICS_FPS = "display.lyricsFps"
    const val DISPLAY_THEME = "display.theme"
    const val DISPLAY_ACCENT = "display.accent"
    const val DISPLAY_DYNAMIC_COLOUR = "display.dynamicColour"
    const val ACTIONS_ORDER_CONTEXT_MENU = "actions.order.contextMenu"
    const val ACTIONS_ORDER_SIDEBAR = "actions.order.sidebar"
    const val ACTIONS_ORDER_MEDIA_SESSION = "actions.order.mediaSession"
    const val SYNC_ENABLED = "sync.enabled"
    const val CONNECT_COORDINATOR_URL = "connect.coordinatorUrl"
    const val CONNECT_LAN_DISCOVERY = "connect.lanDiscovery"
    const val STORAGE_WARN_THRESHOLD_BYTES = "storage.warnThresholdBytes"
    /** Unset (or at its default) = automatic: min(2 GiB, 10% of the cache volume). */
    const val STORAGE_CACHE_MAX_BYTES = "storage.cacheMaxBytes"
    const val STORAGE_PREFETCH_ON_MOBILE_DATA = "storage.prefetchOnMobileData"
    const val DOWNLOADS_TRANSCODE = "downloads.transcode"
    const val DOWNLOADS_WIFI_ONLY = "downloads.wifiOnly"
    const val SLEEP_DEFAULT_MINUTES = "sleep.defaultMinutes"
    const val SLEEP_STOP_AT_END_OF_TRACK = "sleep.stopAtEndOfTrack"
    const val SCROBBLE_ENABLED = "scrobble.enabled"
    const val SCROBBLE_NOW_PLAYING = "scrobble.nowPlaying"
    const val LIBRARY_SYNC_INTERVAL_MINUTES = "library.syncIntervalMinutes"
    const val LIBRARY_FULL_RECONCILE_DAYS = "library.fullReconcileDays"
    const val SEARCH_INCLUDE_SERVER = "search.includeServer"

    fun actionOrder(surface: String) = "actions.order.$surface"
}

/**
 * Canonical action ids served by the core's registry (`crates/hocket-core/src/actions/defs.rs`).
 * Descriptors from `Query.Actions` always carry these; aliases are only accepted on input.
 * Ids marked "ui-handled" produce no core command: the platform performs them.
 */
object ActionIds {
    const val PLAY = "play"
    const val PLAY_SHUFFLED = "playShuffled"
    const val TOGGLE_PLAY = "togglePlay"
    const val NEXT = "next"
    const val PREVIOUS = "previous"
    const val PLAY_NEXT = "playNext"
    const val PLAY_LATER = "playLater"
    const val REMOVE_FROM_QUEUE = "removeFromQueue"
    const val SHUFFLE = "shuffle"
    const val REPEAT = "repeat"
    const val AUTOPLAY = "autoplay"
    const val SAVE_QUEUE_AS_PLAYLIST = "saveQueueAsPlaylist"
    const val RESTORE_SAVED_QUEUE = "restoreSavedQueue"
    const val PIN_SAVED_QUEUE = "pinSavedQueue"
    const val UNPIN_SAVED_QUEUE = "unpinSavedQueue"
    const val DELETE_SAVED_QUEUE = "deleteSavedQueue"
    const val ADD_TO_PLAYLIST = "addToPlaylist" // ui-handled
    const val REMOVE_FROM_PLAYLIST = "removeFromPlaylist"
    const val RATE = "rate" // ui-handled
    const val LOVE = "love"
    const val UNLOVE = "unlove"
    const val DOWNLOAD = "download"
    const val UNPIN = "unpin"
    const val GO_TO_ALBUM = "goToAlbum" // ui-handled
    const val GO_TO_ARTIST = "goToArtist" // ui-handled
    const val DELETE_PLAYLIST = "deletePlaylist"
    const val SLEEP_TIMER = "sleepTimer" // ui-handled
    const val HANDOFF = "handoff"
    const val TOGGLE_LYRICS = "toggleLyrics" // ui-handled
    const val COPY_DIAGNOSTICS = "copyDiagnostics" // ui-handled

    fun rate(stars: Int) = "rate$stars"

    /** Ids the platform performs itself (`ActionDef::ui_handled`). */
    val UI_HANDLED = setOf(ADD_TO_PLAYLIST, RATE, GO_TO_ALBUM, GO_TO_ARTIST, SLEEP_TIMER, TOGGLE_LYRICS, COPY_DIAGNOSTICS,
        "navigateHome", "navigateLibrary", "navigateTracks", "navigateAlbums", "navigateArtists", "navigatePlaylists", "navigateGenres",
        "navigateRecent", "navigateFilters", "navigateDownloads", "navigateStats", "navigateSettings")

    /** The registry's default context-menu order (surface `contextMenu`). */
    val CONTEXT_MENU = listOf(PLAY, PLAY_SHUFFLED, PLAY_NEXT, PLAY_LATER, ADD_TO_PLAYLIST, LOVE, UNLOVE, rate(5), rate(4), rate(3), rate(2), rate(1), rate(0),
        DOWNLOAD, UNPIN, GO_TO_ALBUM, GO_TO_ARTIST, REMOVE_FROM_QUEUE, REMOVE_FROM_PLAYLIST, RESTORE_SAVED_QUEUE, PIN_SAVED_QUEUE, UNPIN_SAVED_QUEUE,
        SAVE_QUEUE_AS_PLAYLIST, DELETE_SAVED_QUEUE, DELETE_PLAYLIST)
    val MEDIA_SESSION = listOf(PREVIOUS, TOGGLE_PLAY, NEXT, SHUFFLE, REPEAT, LOVE)
}
