// `hocket://` deep links, parsed once in main before anything is forwarded to
// a renderer. Only the four documented hosts plus `open` and `setup` are
// accepted; the id is a single path segment. Anything else is dropped.
//
// `setup` is another device's setup code (Set up another device):
// `hocket://setup?server=<url>&user=<name>[&password=<p>][&coordinator=<url>][&settings=<s>]`,
// each value percent-encoded; `settings` (sent when there is no coordinator)
// is packed by ./setup-settings and kept packed here, since unpacking is async.
// Android reads and writes the same form (android/…/ui/screens/setup/SetupLink.kt).
import { DEEP_LINK_SCHEME } from "./constants";

export type SetupLink = { kind: "setup"; server: string; user: string; password?: string; coordinator?: string; settings?: string };

/** A packed settings value: unpadded base64url. */
const PACKED_RE = /^[A-Za-z0-9_-]+$/;

export type DeepLink = { kind: "album" | "artist" | "playlist" | "track"; id: string } | { kind: "open" } | SetupLink;

const ID_RE = /^[A-Za-z0-9._~-]{1,256}$/;

export function parseDeepLink(url: string): DeepLink | undefined {
  // Room for a setup link's packed settings.
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
  if (kind === "setup") return id ? undefined : parseSetup(u.search);
  if (kind !== "album" && kind !== "artist" && kind !== "playlist" && kind !== "track") return undefined;
  if (!ID_RE.test(id)) return undefined;
  return { kind, id };
}

/** Canonical form of a parsed link, the only string main forwards. */
export function formatDeepLink(link: DeepLink): string {
  if (link.kind === "open") return `${DEEP_LINK_SCHEME}://open`;
  if (link.kind === "setup") {
    const params: [string, string | undefined][] = [["server", link.server], ["user", link.user], ["password", link.password], ["coordinator", link.coordinator], ["settings", link.settings]];
    return `${DEEP_LINK_SCHEME}://setup?${params.filter(([, v]) => v !== undefined).map(([k, v]) => `${k}=${encodeURIComponent(v!)}`).join("&")}`;
  }
  return `${DEEP_LINK_SCHEME}://${link.kind}/${link.id}`;
}

/**
 * A setup link's query: an http(s) server (trimmed, without trailing slashes,
 * the scheme lower-cased) and a user are required; unknown parameters are ignored.
 */
function parseSetup(search: string): SetupLink | undefined {
  const values = new Map<string, string>();
  for (const pair of search.replace(/^\?/, "").split("&")) {
    const i = pair.indexOf("=");
    if (i <= 0) continue;
    try {
      // Not URLSearchParams: it reads "+" as a space, and the links encode spaces as %20.
      values.set(pair.slice(0, i), decodeURIComponent(pair.slice(i + 1)));
    } catch {
      // A malformed escape: as if the parameter were missing.
    }
  }
  const [, scheme, rest] = /^(https?):\/\/(.+)$/i.exec((values.get("server") ?? "").trim().replace(/\/+$/, "")) ?? [];
  const user = values.get("user")?.trim();
  if (!scheme || !rest || !user) return undefined;
  const link: SetupLink = { kind: "setup", server: `${scheme.toLowerCase()}://${rest}`, user };
  const password = values.get("password");
  if (password) link.password = password;
  const coordinator = values.get("coordinator")?.trim();
  if (coordinator) link.coordinator = coordinator;
  // Settings that aren't packed are dropped, not the whole link.
  const settings = values.get("settings")?.trim();
  if (settings && PACKED_RE.test(settings)) link.settings = settings;
  return link;
}
