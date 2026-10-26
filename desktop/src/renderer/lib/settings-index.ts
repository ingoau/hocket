// Settings search: every setting row a person might look for, as data, with
// the section it lives in. Labels and descriptions come from the same strings
// the rows show, so the index reads the way the page does; keywords add the
// words people search with that the labels don't use ("dark mode", "bass").
// Each id is the row's anchor on its page (`setting-<id>`), which the Settings
// view scrolls to and highlights when a result is picked.
import { t, type StringId } from "@shared/strings";

export const SETTINGS_SECTIONS = ["general", "audio", "transcoding", "connect", "storage", "lyrics", "appearance", "customisation", "shortcuts", "backup", "diagnostics", "advanced", "about"] as const;
export type SettingsSection = (typeof SETTINGS_SECTIONS)[number];

export interface SettingEntry {
  id: string;
  section: SettingsSection;
  label: StringId;
  desc?: StringId;
  keywords?: StringId;
}

export const SETTINGS_INDEX: SettingEntry[] = [
  { id: "servers", section: "general", label: "settings.servers", keywords: "settingsSearch.kw.servers" },
  { id: "closeToTray", section: "general", label: "settings.closeToTray", keywords: "settingsSearch.kw.closeToTray" },
  { id: "settingsSync", section: "general", label: "settings.settingsSync", keywords: "settingsSearch.kw.settingsSync" },
  { id: "savedQueueCap", section: "general", label: "settingsSearch.savedQueueCap", keywords: "settingsSearch.kw.savedQueueCap" },
  { id: "ratingBridge", section: "general", label: "settings.ratingBridge", keywords: "settingsSearch.kw.ratingBridge" },
  { id: "queueMode", section: "general", label: "settings.queueMode", keywords: "settingsSearch.kw.queueMode" },
  { id: "replayGain", section: "audio", label: "settings.replayGain", keywords: "settingsSearch.kw.replayGain" },
  { id: "replayGainPreamp", section: "audio", label: "settingsSearch.replayGainPreamp", keywords: "settingsSearch.kw.replayGain" },
  { id: "normalisation", section: "audio", label: "settings.normalisation", keywords: "settingsSearch.kw.normalisation" },
  { id: "gapless", section: "audio", label: "settings.gapless", keywords: "settingsSearch.kw.gapless" },
  { id: "outputDevice", section: "audio", label: "settings.outputDevice", keywords: "settingsSearch.kw.outputDevice" },
  { id: "exclusive", section: "audio", label: "settings.exclusive", keywords: "settingsSearch.kw.exclusive" },
  { id: "eq", section: "audio", label: "settings.eq", keywords: "settingsSearch.kw.eq" },
  { id: "transcodingFormat", section: "transcoding", label: "settingsSearch.transcodingFormat", keywords: "settingsSearch.kw.transcoding" },
  { id: "transcodingBitrate", section: "transcoding", label: "settingsSearch.transcodingBitrate", keywords: "settingsSearch.kw.transcoding" },
  { id: "transcodingCannotDecode", section: "transcoding", label: "settings.transcodingCannotDecode", keywords: "settingsSearch.kw.transcoding" },
  { id: "deviceName", section: "connect", label: "settings.deviceName", keywords: "settingsSearch.kw.deviceName" },
  { id: "coordinatorUrl", section: "connect", label: "settings.coordinatorUrl", keywords: "settingsSearch.kw.coordinatorUrl" },
  { id: "lanDiscovery", section: "connect", label: "settings.lanDiscovery", keywords: "settingsSearch.kw.lanDiscovery" },
  { id: "storageUsage", section: "storage", label: "settings.storageUsage", keywords: "settingsSearch.kw.storage" },
  { id: "cacheUsage", section: "storage", label: "settings.cacheUsage", keywords: "settingsSearch.kw.cache" },
  { id: "cacheBudget", section: "storage", label: "settings.cacheBudget", desc: "settings.cacheBudgetDesc", keywords: "settingsSearch.kw.cache" },
  { id: "dataSaved", section: "storage", label: "settings.dataSaved", keywords: "settingsSearch.kw.dataSaved" },
  { id: "prefetchMobile", section: "storage", label: "settings.prefetchMobile", desc: "settings.prefetchMobileDesc", keywords: "settingsSearch.kw.prefetchMobile" },
  { id: "storageWarn", section: "storage", label: "settings.storageWarn", keywords: "settingsSearch.kw.storage" },
  { id: "clearCache", section: "storage", label: "downloads.clearCache", keywords: "settingsSearch.kw.cache" },
  { id: "batterySaverAuto", section: "storage", label: "settings.batterySaverAuto", keywords: "settingsSearch.kw.batterySaver" },
  { id: "batterySaver", section: "storage", label: "settings.batterySaver", keywords: "settingsSearch.kw.batterySaver" },
  { id: "externalLyrics", section: "lyrics", label: "settings.externalLyrics", desc: "settings.externalLyricsPrivacy", keywords: "settingsSearch.kw.externalLyrics" },
  { id: "lyricsFps", section: "lyrics", label: "settings.lyricsFps", keywords: "settingsSearch.kw.lyricsFps" },
  { id: "lyricsFpsBattery", section: "lyrics", label: "settings.lyricsFpsBattery", keywords: "settingsSearch.kw.lyricsFps" },
  { id: "theme", section: "appearance", label: "settings.theme", keywords: "settingsSearch.kw.theme" },
  { id: "accent", section: "appearance", label: "settings.accent", keywords: "settingsSearch.kw.accent" },
  { id: "dynamicAccent", section: "appearance", label: "settings.dynamicAccent", keywords: "settingsSearch.kw.dynamicAccent" },
  { id: "animatedBackground", section: "appearance", label: "settings.animatedBackground", keywords: "settingsSearch.kw.animatedBackground" },
  { id: "immersiveArtwork", section: "appearance", label: "settings.immersiveArtwork", keywords: "settingsSearch.kw.immersiveArtwork" },
  { id: "playerRating", section: "appearance", label: "settings.playerRating", keywords: "settingsSearch.kw.playerRating" },
  { id: "albumTrackArtists", section: "appearance", label: "settings.albumTrackArtists", desc: "settings.albumTrackArtistsDesc", keywords: "settingsSearch.kw.albumTrackArtists" },
  { id: "fullscreenIdleHide", section: "appearance", label: "settings.fullscreenIdleHide", desc: "settings.fullscreenIdleHideDesc" },
  { id: "lyricsSize", section: "appearance", label: "settings.lyricsSize", desc: "settings.lyricsSizeDesc" },
  { id: "lyricsAnimated", section: "appearance", label: "settings.lyricsAnimated", desc: "settings.lyricsAnimatedDesc", keywords: "settingsSearch.kw.animatedBackground" },
  { id: "sidebarItems", section: "customisation", label: "settings.sidebarItems", keywords: "settingsSearch.kw.customise" },
  { id: "contextMenuItems", section: "customisation", label: "settings.contextMenuItems", keywords: "settingsSearch.kw.customise" },
  { id: "mediaSessionButtons", section: "customisation", label: "settings.mediaSessionButtons", keywords: "settingsSearch.kw.customise" },
  { id: "exportConfig", section: "backup", label: "settings.exportConfig", keywords: "settingsSearch.kw.backup" },
  { id: "importConfig", section: "backup", label: "settings.importConfig", keywords: "settingsSearch.kw.backup" },
  { id: "copyDiagnostics", section: "diagnostics", label: "settings.copyDiagnostics", keywords: "settingsSearch.kw.diagnostics" },
  { id: "developerMode", section: "advanced", label: "settings.developerMode", keywords: "settingsSearch.kw.developer" },
  { id: "developerOpen", section: "advanced", label: "settings.developerOpen", desc: "settings.developerOpenDesc", keywords: "settingsSearch.kw.developer" },
];

/** Keywords for a whole section, so "keys" finds Keyboard shortcuts and "logs" finds Diagnostics. */
const SECTION_KEYWORDS: Partial<Record<SettingsSection, StringId>> = {
  customisation: "settingsSearch.kw.customise",
  shortcuts: "settingsSearch.kw.shortcuts",
  backup: "settingsSearch.kw.backup",
  diagnostics: "settingsSearch.kw.diagnostics",
  advanced: "settingsSearch.kw.developer",
  about: "settingsSearch.kw.about",
};

export function sectionLabel(section: SettingsSection): string {
  return t(`settings.section.${section}` as StringId);
}

export type SettingsHit =
  | { kind: "section"; section: SettingsSection; label: string }
  | { kind: "setting"; id: string; section: SettingsSection; label: string };

/** Lower case without accents, so "equaliser" and "Égaliseur" compare plainly. */
function fold(s: string): string {
  return s.normalize("NFD").replace(/[̀-ͯ]/g, "").toLowerCase();
}

/**
 * Every word of the query must appear in the entry (its label, description,
 * keywords or section name). A label that starts with the query ranks first,
 * then a label that contains every word, then the rest; sections come before
 * the settings in them at the same rank. Empty for a blank query.
 */
export function searchSettings(query: string): SettingsHit[] {
  const words = fold(query).split(/\s+/).filter(Boolean);
  if (!words.length) return [];
  const q = words.join(" ");
  const scored: { hit: SettingsHit; score: number; order: number }[] = [];
  const consider = (hit: SettingsHit, haystack: string, order: number) => {
    const label = fold(hit.label);
    const all = `${label} ${fold(haystack)}`;
    if (!words.every((w) => all.includes(w))) return;
    const score = label.startsWith(q) ? 0 : words.every((w) => label.includes(w)) ? 1 : 2;
    scored.push({ hit, score, order });
  };
  SETTINGS_SECTIONS.forEach((section, i) => {
    const kw = SECTION_KEYWORDS[section];
    consider({ kind: "section", section, label: sectionLabel(section) }, kw ? t(kw) : "", i * 100);
  });
  SETTINGS_INDEX.forEach((e, i) => {
    const haystack = [e.desc ? t(e.desc) : "", e.keywords ? t(e.keywords) : "", sectionLabel(e.section)].join(" ");
    consider({ kind: "setting", id: e.id, section: e.section, label: t(e.label) }, haystack, SETTINGS_SECTIONS.indexOf(e.section) * 100 + 1 + i);
  });
  return scored.sort((a, b) => a.score - b.score || a.order - b.order).map((s) => s.hit);
}
