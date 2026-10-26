// An album's release kind for the discography shelves, as the core decides it
// (`Album::release_kind`): a compilation first, then an EP, then a single,
// anything else an album. Release types come from OpenSubsonic `releaseTypes`,
// lowercased by the core.
import type { Album } from "@core/api";

export type ReleaseKind = "album" | "ep" | "single" | "compilation";
export const RELEASE_KINDS: ReleaseKind[] = ["album", "ep", "single", "compilation"];

export function releaseKind(a: Pick<Album, "isCompilation" | "releaseTypes">): ReleaseKind {
  const types = a.releaseTypes ?? [];
  if (a.isCompilation || types.includes("compilation")) return "compilation";
  if (types.includes("ep")) return "ep";
  if (types.includes("single")) return "single";
  return "album";
}

/** The albums grouped by kind, each group keeping the given order; empty groups left out. */
export function byReleaseKind<A extends Pick<Album, "isCompilation" | "releaseTypes">>(albums: A[]): { kind: ReleaseKind; albums: A[] }[] {
  return RELEASE_KINDS.map((kind) => ({ kind, albums: albums.filter((a) => releaseKind(a) === kind) })).filter((g) => g.albums.length > 0);
}
