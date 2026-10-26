/** How many top songs the artist page asks for. */
export const TOP_SONGS_COUNT = 10;
/** Top songs shown before "View more": two columns of three. */
export const TOP_SONGS_PREVIEW = 6;
// Rows of 60px, two columns: the loading space matches the list it holds for.
const ROW_HEIGHT = 60;

export type TopSongsSection = { kind: "loading"; height: number } | { kind: "list"; height: number } | { kind: "none" };

/**
 * What the artist page shows for its top songs. While the first answer is on its way the section
 * keeps the space the first rows take, so the albums below do not jump when the songs arrive.
 */
export function topSongsSection(count: number | undefined, failed: boolean): TopSongsSection {
  if (count === undefined) return failed ? { kind: "none" } : { kind: "loading", height: tableHeight(TOP_SONGS_PREVIEW) };
  if (count === 0) return { kind: "none" };
  return { kind: "list", height: tableHeight(Math.min(count, TOP_SONGS_COUNT)) };
}

function tableHeight(rows: number): number {
  return Math.ceil(rows / 2) * ROW_HEIGHT;
}
