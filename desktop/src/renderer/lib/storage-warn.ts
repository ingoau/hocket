// The storage warning thresholds offered (Settings and the Downloads card share them), in GB.
export const STORAGE_WARN_GB = [2, 4, 5, 10, 20, 50, 100];
export const GIB = 1024 ** 3;

/** The choices, with the current value added when it is not one of them (set elsewhere, or an older list). */
export function warnChoices(currentGb: number): number[] {
  return currentGb && !STORAGE_WARN_GB.includes(currentGb) ? [...STORAGE_WARN_GB, currentGb].sort((a, b) => a - b) : STORAGE_WARN_GB;
}
