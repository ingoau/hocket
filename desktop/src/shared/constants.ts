// Constants shared between main, preload and renderer.
import type { PlayerNoticeCode } from "@core/api";

/** Must equal `API_SCHEMA_VERSION` in crates/hocket-core/src/api.rs. The main
 * process refuses a native addon reporting a different value. */
export const API_SCHEMA_VERSION = 1;

/** Custom protocol that serves the renderer bundle in production. */
export const APP_SCHEME = "app";
/**
 * Custom protocol that serves artwork resolved by Query.Artwork. The renderer
 * only ever sees opaque tokens (`hocket-art://art/<token>`): main maps a
 * token to a file inside the image cache when it answers the query, so the
 * page can't name a path.
 */
export const ART_SCHEME = "hocket-art";
export const ART_HOST = "art";
/** Reserved deep-link scheme (`hocket://album/<id>`, `hocket://open`). */
export const DEEP_LINK_SCHEME = "hocket";

export const DEV_SERVER_URL = "http://localhost:5178";

/** Fixed artwork cache sizes, per design.md "Player features → Caching". */
export const ARTWORK_SIZES = { thumb: 64, grid: 300, full: 1000 } as const;

/** The core answers Query.Artwork with a filesystem path; a `file://` URL is tolerated too. */
export function artworkFilePath(pathOrUrl: string): string {
  if (!pathOrUrl.startsWith("file://")) return pathOrUrl;
  try {
    const u = new URL(pathOrUrl);
    let p = decodeURIComponent(u.pathname);
    if (/^\/[A-Za-z]:/.test(p)) p = p.slice(1); // file:///C:/x on Windows
    return u.host && u.host !== "localhost" ? `//${u.host}${p}` : p;
  } catch {
    return pathOrUrl.replace(/^file:\/\//, "");
  }
}

/** Tokens main hands out for artwork: 32 hex characters, nothing else is accepted. */
export const ART_TOKEN_RE = /^[0-9a-f]{32}$/;

/** URL for an artwork token returned (via main) by Query.Artwork. */
export function artworkUrl(token: string | undefined): string | undefined {
  if (!token || !ART_TOKEN_RE.test(token)) return undefined;
  return `${ART_SCHEME}://${ART_HOST}/${token}`;
}

/**
 * PlayerNotice codes the core emits offline (crates/hocket-core/src/core/
 * handlers/cache.rs); the renderer shows them as a banner with a way to the
 * "Available offline" list. FakeCore emits the same.
 */
export const OFFLINE_NOTICE_CODES: readonly PlayerNoticeCode[] = ["offlineSkipping", "nothingAvailableOffline"];

/** Built-in filters' ids (crates/hocket-core/src/filters/mod.rs `default_filters`). */
export const BUILTIN_FILTER_PREFIX = "builtin:";
export const AVAILABLE_OFFLINE_FILTER_ID = "builtin:available-offline";
