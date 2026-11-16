// `hocket://` deep links, parsed once in main before anything is forwarded to
// a renderer. Only the four documented hosts plus `open`, `pair` and `claim`
// are accepted; the id is a single path segment. Anything else is dropped.
//
// `pair` is another device's pairing invite (Settings → Devices → Add a
// device): `hocket://pair?d=<base64url JSON>`, which the core reads (the
// identity, the inviter's key, a one-time secret, where to reach it, and
// the account configs without passwords). `claim` is what
// `hocket-coordinator pair --url` prints as a QR code:
// `hocket://claim?url=<coordinator>&code=<code>`. Both are kept whole; the
// core checks them. Android reads the same forms.
import { DEEP_LINK_SCHEME } from "./constants";

export type PairLink = { kind: "pair"; data: string };
export type ClaimLink = { kind: "claim"; url: string; code: string };

export type DeepLink = { kind: "album" | "artist" | "playlist" | "track"; id: string } | { kind: "open" } | PairLink | ClaimLink;

const ID_RE = /^[A-Za-z0-9._~-]{1,256}$/;
/** A pairing invite's payload: unpadded base64url. */
const PACKED_RE = /^[A-Za-z0-9_-]{16,6000}$/;
const CODE_RE = /^[A-Za-z0-9 -]{4,32}$/;

export function parseDeepLink(url: string): DeepLink | undefined {
  if (typeof url !== "string" || url.length > 8192) return undefined;
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
  if (kind === "pair") {
    const data = query(u.search).get("d");
    return !id && data && PACKED_RE.test(data) ? { kind: "pair", data } : undefined;
  }
  if (kind === "claim") {
    const q = query(u.search);
    const target = q.get("url")?.trim() ?? "";
    const code = q.get("code")?.trim() ?? "";
    if (id || !CODE_RE.test(code) || (target && !/^wss?:\/\/[^\s]+$/i.test(target))) return undefined;
    return { kind: "claim", url: target, code };
  }
  if (kind !== "album" && kind !== "artist" && kind !== "playlist" && kind !== "track") return undefined;
  if (!ID_RE.test(id)) return undefined;
  return { kind, id };
}

/** Canonical form of a parsed link, the only string main forwards. */
export function formatDeepLink(link: DeepLink): string {
  if (link.kind === "open") return `${DEEP_LINK_SCHEME}://open`;
  if (link.kind === "pair") return `${DEEP_LINK_SCHEME}://pair?d=${link.data}`;
  if (link.kind === "claim") return `${DEEP_LINK_SCHEME}://claim?url=${encodeURIComponent(link.url)}&code=${encodeURIComponent(link.code)}`;
  return `${DEEP_LINK_SCHEME}://${link.kind}/${link.id}`;
}

/** A query string's values (not URLSearchParams: it reads "+" as a space). */
function query(search: string): Map<string, string> {
  const values = new Map<string, string>();
  for (const pair of search.replace(/^\?/, "").split("&")) {
    const i = pair.indexOf("=");
    if (i <= 0) continue;
    try {
      values.set(pair.slice(0, i), decodeURIComponent(pair.slice(i + 1)));
    } catch {
      // A malformed escape: as if the parameter were missing.
    }
  }
  return values;
}
