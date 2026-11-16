import { createHash } from "node:crypto";

/**
 * A server URL normalised the way the core's `server_key` does it: blanks and
 * trailing slashes go, the scheme and host are lowercased, the path keeps its case.
 */
export function serverKey(url: string): string {
  const u = url.trim().replace(/\/+$/, "");
  const start = u.indexOf("://") >= 0 ? u.indexOf("://") + 3 : 0;
  const rest = u.slice(start).search(/[/?#]/);
  const end = rest < 0 ? u.length : start + rest;
  return u.slice(0, end).toLowerCase() + u.slice(end);
}

/**
 * The core's `AccountId` for a Navidrome login: the first 8 bytes of
 * md5(`serverKey(url)|username`), in hex. The same on every device.
 */
export function accountIdFor(url: string, username: string): string {
  return createHash("md5").update(`${serverKey(url)}|${username}`).digest("hex").slice(0, 16);
}
