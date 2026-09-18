// Constants shared between main, preload and renderer.

/** Must equal `API_SCHEMA_VERSION` in crates/hocket-core/src/api.rs. The main
 * process refuses a native addon reporting a different value. */
export const API_SCHEMA_VERSION = 1;

/** Custom protocol that serves the renderer bundle in production. */
export const APP_SCHEME = "app";
/** Custom protocol that serves artwork files resolved by Query.Artwork. */
export const ART_SCHEME = "hocket-art";
/** Reserved deep-link scheme (`hocket://album/<id>`, `hocket://open`). */
export const DEEP_LINK_SCHEME = "hocket";

export const DEV_SERVER_URL = "http://localhost:5178";

/** Fixed artwork cache sizes, per design.md "Player features → Caching". */
export const ARTWORK_SIZES = { thumb: 64, grid: 300, full: 1000 } as const;

export function artworkUrl(path: string | undefined): string | undefined {
  if (!path) return undefined;
  return `${ART_SCHEME}://file/${encodeURIComponent(path)}`;
}
