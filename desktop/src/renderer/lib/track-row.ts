/** A track table row's height in px; global.css's --row-h matches. */
export const TRACK_ROW_H = 40;
/** The column labels' height over a track table. */
export const TRACK_HEAD_H = 48;
/** Most rows a page shows in full before its table scrolls on its own (and virtualises). */
const IN_PAGE_ROWS = 300;

/** The height a page gives a track table so it shows every row (or a long list's window of them). */
export function trackTableHeight(rows: number): string {
  return rows > IN_PAGE_ROWS ? "max(360px, 75vh)" : `${Math.max(1, rows) * TRACK_ROW_H + TRACK_HEAD_H}px`;
}
