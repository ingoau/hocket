/** Position extrapolated from the core's sample, capped at the song. */
export function previewPosition(sample: { positionMs: number; takenAt: number; playing: boolean; durationMs: number }, now: number): number {
  const extra = sample.playing ? Math.max(0, now - sample.takenAt) : 0;
  const at = sample.positionMs + extra;
  return sample.durationMs > 0 ? Math.min(sample.durationMs, at) : at;
}

/** How far through a preview is, 0 to 1: through its clip, else through the whole song. */
export function previewProgress(sample: { positionMs: number; takenAt: number; playing: boolean; durationMs: number; clip?: { startMs: number; endMs: number } }, now: number): number {
  const start = sample.clip?.startMs ?? 0;
  const end = sample.clip?.endMs ?? sample.durationMs;
  if (end <= start) return 0;
  return Math.min(1, Math.max(0, (previewPosition(sample, now) - start) / (end - start)));
}
