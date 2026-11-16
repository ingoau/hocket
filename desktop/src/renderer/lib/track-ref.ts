import type { RatingTarget, TrackRef } from "../../core/api";

/** Anything that names a song on an account: a `Track`, `TrackSummary`, `QueueItem` track… */
export interface HasRef {
  id: string;
  serverId: string;
}

/** The reference commands and queries name a song by (`docs/providers.md`, Phase 3). */
export const refOf = (t: HasRef): TrackRef => ({ source: t.serverId, id: t.id });

export const refsOf = (list: readonly HasRef[]): TrackRef[] => list.map(refOf);

/** A song as a rating or love target. */
export const ratingTarget = (t: HasRef): RatingTarget => ({ type: "track", data: { id: t.id, source: t.serverId } });

/** Stable string key for a reference (sets, React keys). */
export const refKey = (r: TrackRef): string => `${r.source}\u0001${r.id}`;

/** The reference a [refKey] names. */
export const parseRefKey = (key: string): TrackRef => {
  const i = key.indexOf("\u0001");
  return i < 0 ? { source: "", id: key } : { source: key.slice(0, i), id: key.slice(i + 1) };
};

/** A song's primer key ([refKey] of it), or nothing. */
export const primeKey = (t: HasRef | undefined): string | undefined => (t ? refKey(refOf(t)) : undefined);
