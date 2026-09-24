// Screen-reader text for values the UI shows as numbers: slider values
// (aria-valuetext), ratings, and the polite now-playing announcement.
import { t } from "@shared/strings";

function unit(kind: "hours" | "minutes" | "seconds", n: number): string {
  return n === 1 ? t(`a11y.${kind}.one`) : t(`a11y.${kind}.other`, { n });
}

/** "1 hour 2 minutes 3 seconds", "3 minutes 32 seconds", "0 seconds". Zero parts are left out. */
export function spokenDuration(ms: number | undefined): string {
  const total = ms === undefined || !Number.isFinite(ms) ? 0 : Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const parts: string[] = [];
  if (h) parts.push(unit("hours", h));
  if (m) parts.push(unit("minutes", m));
  if (s || !parts.length) parts.push(unit("seconds", s));
  return parts.join(" ");
}

/** Seek slider aria-valuetext: "1 minute 32 seconds of 3 minutes 32 seconds". */
export function seekValueText(positionMs: number, durationMs: number | undefined): string {
  const position = spokenDuration(positionMs);
  if (!durationMs || durationMs <= 0) return t("a11y.seekUnknown", { position });
  return t("a11y.seekValue", { position, duration: spokenDuration(durationMs) });
}

/** Volume slider aria-valuetext for a 0…1 volume: "45%". */
export function volumeValueText(volume: number): string {
  return t("a11y.volumeValue", { percent: Math.round(Math.max(0, Math.min(1, volume)) * 100) });
}

/** "No rating", "1 star", "4 stars". */
export function ratingText(rating: number): string {
  if (!rating) return t("a11y.ratingNone");
  return rating === 1 ? t("a11y.ratingValue.one") : t("a11y.ratingValue.other", { n: rating });
}

/** The polite announcement on a track change (never on position ticks). */
export function nowPlayingAnnouncement(title: string | undefined, artist: string | undefined | null): string {
  if (!title) return "";
  return artist ? t("a11y.nowPlaying", { title, artist }) : t("a11y.nowPlayingNoArtist", { title });
}

/** Keyboard step for the seek slider: 5 s per arrow, 30 s per page, Home/End to the ends. */
export function seekKeyTarget(key: string, positionMs: number, durationMs: number): number | undefined {
  if (!durationMs) return undefined;
  const clamp = (v: number) => Math.max(0, Math.min(durationMs, v));
  switch (key) {
    case "ArrowLeft":
    case "ArrowDown":
      return clamp(positionMs - 5000);
    case "ArrowRight":
    case "ArrowUp":
      return clamp(positionMs + 5000);
    case "PageDown":
      return clamp(positionMs - 30000);
    case "PageUp":
      return clamp(positionMs + 30000);
    case "Home":
      return 0;
    case "End":
      return durationMs;
    default:
      return undefined;
  }
}
