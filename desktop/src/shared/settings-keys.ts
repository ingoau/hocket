// Settings keys are the core registry's (crates/hocket-core/src/settings/registry.rs).
// Values are JSON strings in Setting.value. Keys the app needs that the registry
// doesn't define (close-to-tray) live in the app's own device-local preferences.
export const SK = {
  queueMode: "queue.mode",
  queueSavedCap: "queue.savedCap",
  lyricsExternalEnabled: "lyrics.external.enabled",
  ratingsLoveBridgeEnabled: "ratings.loveBridge.enabled",
  ratingsLoveBridgeThreshold: "ratings.loveBridge.threshold",
  batteryAutoEngage: "battery.autoEngage",
  batteryLyricsFps: "battery.lyricsFps",
  displayAnimatedBackground: "display.animatedBackground",
  displayLyricsFps: "display.lyricsFps",
  displayTheme: "display.theme",
  displayAccent: "display.accent",
  displayDynamicColour: "display.dynamicColour",
  displayQueuePanelSplit: "display.queuePanelSplit",
  syncEnabled: "sync.enabled",
  connectCoordinatorUrl: "connect.coordinatorUrl",
  connectLanDiscovery: "connect.lanDiscovery",
  storageWarnThresholdBytes: "storage.warnThresholdBytes",
  /** Unset (or at its default) = automatic budget, min(2 GiB, 10% of the cache volume). */
  storageCacheMaxBytes: "storage.cacheMaxBytes",
  storagePrefetchOnMobileData: "storage.prefetchOnMobileData",
  storagePrefetchPlayingElsewhere: "storage.prefetchPlayingElsewhere",
  sleepDefaultMinutes: "sleep.defaultMinutes",
  scrobbleEnabled: "scrobble.enabled",
  searchIncludeServer: "search.includeServer",
} as const;

export const DEFAULT_ACCENT = "#6f5cff";
