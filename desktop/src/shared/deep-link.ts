// `hocket://` deep links, parsed once in main before anything is forwarded to
// a renderer. Only the four documented hosts plus `open` are accepted; the id
// is a single path segment. Anything else is dropped.
import { DEEP_LINK_SCHEME } from "./constants";

export type DeepLink = { kind: "album" | "artist" | "playlist" | "track"; id: string } | { kind: "open" };

const ID_RE = /^[A-Za-z0-9._~-]{1,256}$/;

export function parseDeepLink(url: string): DeepLink | undefined {
  if (typeof url !== "string" || url.length > 2048) return undefined;
  let u: URL;
  try {
    u = new URL(url);
  } catch {
    return undefined;
  }
  if (u.protocol !== `${DEEP_LINK_SCHEME}:`) return undefined;
  const kind = u.hostname.toLowerCase();
  const id = u.pathname.replace(/^\/+/, "");
  if (kind === "open") return id ? undefined : { kind: "open" };
  if (kind !== "album" && kind !== "artist" && kind !== "playlist" && kind !== "track") return undefined;
  if (!ID_RE.test(id)) return undefined;
  return { kind, id };
}

/** Canonical form of a parsed link, the only string main forwards. */
export function formatDeepLink(link: DeepLink): string {
  return link.kind === "open" ? `${DEEP_LINK_SCHEME}://open` : `${DEEP_LINK_SCHEME}://${link.kind}/${link.id}`;
}
