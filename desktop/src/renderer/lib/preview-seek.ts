/** Where a horizontal drag seeks, as a fraction of the song. `edge` is kept clear at both ends. */
export function seekFraction(x: number, width: number, edge: number): number {
  const span = Math.max(1, width - edge * 2);
  return Math.min(1, Math.max(0, (x - edge) / span));
}

/** Position extrapolated from the core's sample, capped at the song. */
export function previewPosition(sample: { positionMs: number; takenAt: number; playing: boolean; durationMs: number }, now: number): number {
  const extra = sample.playing ? Math.max(0, now - sample.takenAt) : 0;
  const at = sample.positionMs + extra;
  return sample.durationMs > 0 ? Math.min(sample.durationMs, at) : at;
}
