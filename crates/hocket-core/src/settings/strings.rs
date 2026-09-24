//! User-facing text for settings, keyed by setting id. Platform layers look
//! these up by id from their own resources; the English here is the
//! fallback (and the source the translators start from).

/// English label and description for a setting key.
pub struct SettingText {
    pub key: &'static str,
    pub label: &'static str,
    pub description: &'static str,
}

pub const TEXTS: &[SettingText] = &[
    t("queue.mode", "Queue mode", "How Play Next behaves: a separate Playing Next list (Apple) or spliced into the context after the current track (YouTube)."),
    t("queue.savedCap", "Saved queues to keep", "How many unpinned recent queues are kept. Zero keeps only pinned queues."),
    t("queue.historyCap", "Playback history length", "How many played items stay reachable with Previous."),
    t("autoplay.settings", "Autoplay", "Provider order, seed window, sonic similarity threshold, exclusion window and optional saved filter."),
    t("autoplay.contextOverrides", "Autoplay per-context order", "Provider order overrides for album and artist contexts."),
    t("audio.settings", "Audio", "ReplayGain, equaliser, normalisation, gapless playback, output device and exclusive mode for this device."),
    t("transcoding.profiles", "Transcoding profiles", "Format and bitrate per network, plus formats this device can't decode natively."),
    t("lyrics.external.enabled", "Fetch lyrics online", "Look lyrics up on an external service when the server has none. Off by default because it reveals what you're listening to."),
    t("lyrics.external.provider", "Lyrics provider", "Which external service to use when online lyrics are enabled."),
    t("lyrics.defaultOffsetMs", "Lyrics timing offset", "Global timing adjustment in milliseconds applied to every track. Positive shows lyrics later."),
    t("lyrics.showTranslations", "Show translations", "Show translated lines under the original when the server provides them."),
    t("ratings.loveBridge.enabled", "Love highly rated tracks", "When you rate a track at or above the threshold, also mark it loved. One-way; lowering a rating never unloves."),
    t("ratings.loveBridge.threshold", "Love threshold", "Star rating at or above which a track is marked loved."),
    t("battery.autoEngage", "Battery saver on battery", "Engage battery saver automatically when running on battery."),
    t("battery.lyricsFps", "Battery saver lyrics frame rate", "Frame rate cap for lyric animations while battery saver is engaged."),
    t("battery.smallArtwork", "Battery saver small artwork", "Use the smaller artwork size while battery saver is engaged."),
    t("battery.pausePrefetch", "Battery saver pauses prefetch", "Pause background prefetch and speculative pre-buffering while battery saver is engaged."),
    t("display.animatedBackground", "Animated background", "Fluid artwork background in the fullscreen player. Independent of battery saver."),
    t("display.lyricsFps", "Lyrics frame rate", "Frame rate cap for lyric animations."),
    t("display.theme", "Theme", "Follow the system, or force light or dark."),
    t("display.accent", "Accent colour", "Accent colour as a hex value, or empty for the default."),
    t("display.dynamicColour", "Dynamic colour", "Derive the accent from the current artwork."),
    t("display.queuePanelSplit", "Queue / lyrics split", "Position of the divider between the queue and lyrics panels."),
    t("actions.order.contextMenu", "Context menu items", "Which actions appear in context menus, and in what order."),
    t("actions.order.sidebar", "Sidebar items", "Which sections appear in the sidebar, and in what order."),
    t("actions.order.mediaSession", "Media session buttons", "Which actions appear as OS media controls, and in what order."),
    t("shortcuts", "Keyboard shortcuts", "Custom key bindings for this device."),
    t("sync.enabled", "Sync settings between devices", "Send account-level settings through the coordinator and accept them from other devices."),
    t("connect.coordinatorUrl", "Coordinator URL", "Address of the coordinator for remote sessions (wss://). Devices on the same network find each other without one."),
    t("connect.allowInsecureCoordinator", "Allow unencrypted coordinator", "Let this device send its server credential to a ws:// coordinator on the local network. Only for a coordinator you run yourself; the credential travels in the clear."),
    t("connect.lanDiscovery", "Find devices on this network", "Announce and discover other Hocket devices on the local network."),
    t("storage.warnThresholdBytes", "Storage warning", "Warn when downloads and cache exceed this size."),
    t("storage.cacheMaxBytes", "Stream cache size", "Maximum size of the evictable stream cache."),
    t("storage.prefetchOnMobileData", "Prefetch on mobile data", "Also cache the next two queue items in the background on metered or cellular connections."),
    t("downloads.transcode", "Transcode downloads", "Download using the transcoding profile instead of the original file."),
    t("downloads.wifiOnly", "Download on Wi-Fi only", "Pause downloads on metered connections."),
    t("sleep.defaultMinutes", "Sleep timer default", "Default duration when starting the sleep timer."),
    t("sleep.stopAtEndOfTrack", "Sleep timer finishes the track", "Let the current track finish before stopping."),
    t("scrobble.enabled", "Scrobble plays", "Report plays to the server (which forwards to Last.fm or ListenBrainz if configured)."),
    t("scrobble.nowPlaying", "Send now playing", "Report the track as it starts, not only when it counts as played."),
    t("library.syncIntervalMinutes", "Library refresh interval", "How often to check the server for changes."),
    t("library.fullReconcileDays", "Full reconcile interval", "How often to run a full library reconcile that catches deletions."),
    t("search.includeServer", "Search the server too", "Query the server when local results are thin."),
];

const fn t(key: &'static str, label: &'static str, description: &'static str) -> SettingText {
    SettingText {
        key,
        label,
        description,
    }
}

/// Text for a key, if it has any.
pub fn text_for(key: &str) -> Option<&'static SettingText> {
    TEXTS.iter().find(|t| t.key == key)
}

pub fn label(key: &str) -> &'static str {
    text_for(key).map(|t| t.label).unwrap_or("")
}

pub fn description(key: &str) -> &'static str {
    text_for(key).map(|t| t.description).unwrap_or("")
}
