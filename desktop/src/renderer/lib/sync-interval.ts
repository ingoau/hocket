// Choices for the library refresh (library.syncIntervalMinutes, 5–1440) and
// full reconcile (library.fullReconcileDays, 1–90) intervals. A value set
// elsewhere (another build, a restored backup) that isn't a preset is kept as
// a choice of its own, so the select never shows something it isn't.
import { t } from "@shared/strings";

export const SYNC_INTERVAL_MINUTES = [15, 30, 60, 120, 360, 720, 1440];
export const FULL_RECONCILE_DAYS = [1, 3, 7, 14, 30, 90];

function withCurrent(presets: number[], current: number): number[] {
  return presets.includes(current) ? presets : [...presets, current].sort((a, b) => a - b);
}

export function syncIntervalChoices(current: number): number[] {
  return withCurrent(SYNC_INTERVAL_MINUTES, current);
}

export function reconcileChoices(current: number): number[] {
  return withCurrent(FULL_RECONCILE_DAYS, current);
}

export function syncIntervalLabel(minutes: number): string {
  if (minutes === 60) return t("settings.syncIntervalHour");
  if (minutes === 1440) return t("settings.syncIntervalDay");
  if (minutes > 60 && minutes % 60 === 0) return t("settings.syncIntervalHours", { n: minutes / 60 });
  return t("settings.syncIntervalMinutes", { n: minutes });
}

export function reconcileLabel(days: number): string {
  return days === 1 ? t("settings.fullReconcileDay") : t("settings.fullReconcileDays", { n: days });
}
