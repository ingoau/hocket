/** How many top songs the artist page asks for. */
export const TOP_SONGS_COUNT = 10;
/** Top songs shown before "View more": two columns of three. */
export const TOP_SONGS_PREVIEW = 6;

export type TopSongsSection = { kind: "loading"; rows: number } | { kind: "list"; rows: number } | { kind: "none" };

/**
 * What the artist page shows for its top songs. While the first answer is on its way the section
 * holds placeholder rows in the same grid as the songs (so as many columns, at any width), and the
 * albums below do not jump when the songs arrive.
 */
export function topSongsSection(count: number | undefined, failed: boolean): TopSongsSection {
  if (count === undefined) return failed ? { kind: "none" } : { kind: "loading", rows: TOP_SONGS_PREVIEW };
  if (count === 0) return { kind: "none" };
  return { kind: "list", rows: Math.min(count, TOP_SONGS_COUNT) };
}
