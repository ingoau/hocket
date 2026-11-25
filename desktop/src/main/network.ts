// What this device tells the core about its network. Online or offline comes from Electron
// (net.isOnline, the renderer's online/offline events); the kind of link, an id for the network
// and whether it is metered come from the OS where it says so without a native module:
// - Linux: the default route (/proc/net/route), the interface's type (/sys/class/net), the
//   gateway's MAC (/proc/net/arp), and NetworkManager's metered flag (nmcli, when installed).
// - macOS: `route -n get default`, `networksetup -listallhardwareports` and `arp -n`. Never metered
//   (Low Data Mode isn't readable from here).
// - Windows: none of it (kind "unknown", no id, never metered); that needs WinRT's connection profile.
// The network id is a hash of the gateway's MAC, so a laptop that sleeps on one Wi-Fi and wakes on
// another reports a new network and the core reconnects. Never its address: every other home router
// is 192.168.1.1. Right after link-up or resume the ARP table often has no MAC for the gateway yet;
// that lookup is `pending` and is made again next time (see `linkWatcher`).
// Offline mode (network.offlineMode) is the core's to apply on top of this.
import { createHash } from "node:crypto";
import { execFile } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { networkInterfaces } from "node:os";
import type { NetworkKind, NetworkState } from "@core/api";

/** What the OS says about the link the default route goes out on. */
export interface LinkInfo {
  kind: NetworkKind;
  metered: boolean;
  networkId?: string;
  /** The default route's gateway address: tells a pending lookup whether it is still the same gateway. */
  gateway?: string;
  /** A gateway was found but not its MAC (yet), so there is no id: look again next time. */
  pending?: boolean;
}

export const UNKNOWN_LINK: LinkInfo = { kind: "unknown", metered: false };

export function networkState(online: boolean, link: LinkInfo = UNKNOWN_LINK): NetworkState {
  if (!online) return { kind: "offline", metered: false, networkId: undefined };
  return { kind: link.kind === "offline" ? "unknown" : link.kind, metered: link.metered, networkId: link.networkId };
}

const sameState = (a: NetworkState, b: NetworkState) => a.kind === b.kind && a.metered === b.metered && a.networkId === b.networkId;

/** Passes on changes only: the poll repeats the same answer every time, and the core acts on each one it gets. */
export function networkReporter(push: (state: NetworkState) => void): (online: boolean, link?: LinkInfo) => void {
  let last: NetworkState | undefined;
  return (online, link) => {
    const next = networkState(online, link);
    if (last && sameState(last, next)) return;
    last = next;
    push(next);
  };
}

export function networkIdOf(mac: string | undefined): string | undefined {
  if (!mac || mac === "00:00:00:00:00:00") return undefined;
  return createHash("sha256").update(`hocket-net|${mac.toLowerCase()}`).digest("hex").slice(0, 16);
}

/** The id part of a link with this gateway: its MAC's hash, or `pending` while the MAC isn't known. */
export function gatewayLink(gateway: string | undefined, mac: string | undefined): Pick<LinkInfo, "networkId" | "gateway" | "pending"> {
  if (!gateway) return {};
  const networkId = networkIdOf(mac);
  return networkId ? { networkId, gateway } : { gateway, pending: true };
}

// -- Linux -------------------------------------------------------------------------------------

/** The default route's interface and gateway from /proc/net/route (lowest metric wins). */
export function parseProcRoute(text: string): { iface: string; gateway: string | undefined } | undefined {
  let best: { iface: string; gateway: string | undefined; metric: number } | undefined;
  for (const line of text.split("\n").slice(1)) {
    const f = line.trim().split(/\s+/);
    if (f.length < 8) continue;
    const [iface, dest, gw, flags, , , metric, mask] = f;
    // RTF_UP (0x1); destination and mask 0: the default route.
    if (dest !== "00000000" || mask !== "00000000" || !(parseInt(flags!, 16) & 1)) continue;
    const m = Number(metric);
    if (best && best.metric <= m) continue;
    best = { iface: iface!, gateway: gw === "00000000" ? undefined : hexToIpv4(gw!), metric: m };
  }
  return best && { iface: best.iface, gateway: best.gateway };
}

/** /proc/net/route keeps addresses as little-endian hex. */
function hexToIpv4(hex: string): string | undefined {
  const n = parseInt(hex, 16);
  if (!Number.isFinite(n)) return undefined;
  return [n & 255, (n >>> 8) & 255, (n >>> 16) & 255, (n >>> 24) & 255].join(".");
}

/** The MAC /proc/net/arp holds for `ip` on `iface`. */
export function parseProcArp(text: string, ip: string, iface: string): string | undefined {
  for (const line of text.split("\n").slice(1)) {
    const f = line.trim().split(/\s+/);
    if (f[0] === ip && f[5] === iface && f[3] && f[3] !== "00:00:00:00:00:00") return f[3];
  }
  return undefined;
}

export function linuxKind(iface: string, has: (path: string) => boolean, read: (path: string) => string | undefined): NetworkKind {
  const sys = `/sys/class/net/${iface}`;
  if (has(`${sys}/wireless`) || has(`${sys}/phy80211`)) return "wifi";
  if (/^(ww|rmnet|usb)/.test(iface)) return "cellular";
  // ARPHRD_ETHER, and a real device (not a bridge, a veth or a VPN's tun).
  if (read(`${sys}/type`)?.trim() === "1" && has(`${sys}/device`)) return "wired";
  return "unknown";
}

/** NetworkManager's GENERAL.METERED: "yes", "yes (guessed)", "no", "no (guessed)", "unknown". */
export function parseNmMetered(text: string): boolean {
  return /^yes/.test(text.trim());
}

function readOr(path: string): string | undefined {
  try {
    return readFileSync(path, "utf8");
  } catch {
    return undefined;
  }
}

function run(cmd: string, args: string[]): Promise<string | undefined> {
  return new Promise((resolve) => {
    try {
      execFile(cmd, args, { timeout: 3000, windowsHide: true }, (err, stdout) => resolve(err ? undefined : stdout));
    } catch {
      resolve(undefined);
    }
  });
}

async function linuxLink(): Promise<LinkInfo> {
  const route = parseProcRoute(readOr("/proc/net/route") ?? "");
  if (!route) return UNKNOWN_LINK;
  const kind = linuxKind(route.iface, existsSync, readOr);
  const mac = route.gateway ? parseProcArp(readOr("/proc/net/arp") ?? "", route.gateway, route.iface) : undefined;
  const nm = await run("nmcli", ["-t", "-g", "GENERAL.METERED", "device", "show", route.iface]);
  return { kind, metered: nm !== undefined && parseNmMetered(nm), ...gatewayLink(route.gateway, mac) };
}

// -- macOS -------------------------------------------------------------------------------------

/** `route -n get default`: "interface: en0", "gateway: 192.168.1.1". */
export function parseMacRoute(text: string): { iface: string; gateway: string | undefined } | undefined {
  const iface = /^\s*interface:\s*(\S+)/m.exec(text)?.[1];
  if (!iface) return undefined;
  const gateway = /^\s*gateway:\s*(\d+\.\d+\.\d+\.\d+)/m.exec(text)?.[1];
  return { iface, gateway };
}

/** `networksetup -listallhardwareports`: the hardware port of `iface`, as a kind. */
export function parseMacHardwarePorts(text: string, iface: string): NetworkKind {
  for (const block of text.split(/\n\s*\n/)) {
    const port = /Hardware Port:\s*(.+)/.exec(block)?.[1]?.trim();
    const device = /Device:\s*(\S+)/.exec(block)?.[1];
    if (device !== iface || !port) continue;
    if (/wi-?fi|airport/i.test(port)) return "wifi";
    if (/iphone|ipad|bluetooth pan|modem/i.test(port)) return "cellular";
    if (/ethernet|lan|thunderbolt bridge/i.test(port)) return "wired";
    return "unknown";
  }
  return "unknown";
}

/** `arp -n <ip>`: "? (192.168.1.1) at a4:2b:b0:12:34:56 on en0 ifscope [ethernet]". */
export function parseMacArp(text: string): string | undefined {
  const mac = /\sat\s+([0-9a-f]{1,2}(?::[0-9a-f]{1,2}){5})\s/i.exec(text)?.[1];
  // macOS drops leading zeros ("a4:2b:0:1:..."): pad so the id matches whatever wrote it.
  return mac?.split(":").map((b) => b.padStart(2, "0")).join(":");
}

async function macLink(): Promise<LinkInfo> {
  const route = parseMacRoute((await run("route", ["-n", "get", "default"])) ?? "");
  if (!route) return UNKNOWN_LINK;
  const kind = parseMacHardwarePorts((await run("networksetup", ["-listallhardwareports"])) ?? "", route.iface);
  const mac = route.gateway ? parseMacArp((await run("arp", ["-n", route.gateway])) ?? "") : undefined;
  // An iPhone's Personal Hotspot is metered, whichever way it is joined (Wi-Fi, USB, Bluetooth).
  const hotspot = route.gateway === "172.20.10.1";
  return { kind, metered: hotspot || kind === "cellular", ...gatewayLink(route.gateway, mac) };
}

// -- Watching ------------------------------------------------------------------------------------

/** The interfaces' addresses: cheap to read, and they change whenever the link does. */
export function interfacesFingerprint(ifaces: ReturnType<typeof networkInterfaces> = networkInterfaces()): string {
  return Object.entries(ifaces)
    .flatMap(([name, list]) => (list ?? []).filter((a) => !a.internal).map((a) => `${name}|${a.address}|${a.netmask}`))
    .sort()
    .join(",");
}

export function detectLink(platform: NodeJS.Platform = process.platform): Promise<LinkInfo> {
  if (platform === "linux") return linuxLink().catch(() => UNKNOWN_LINK);
  if (platform === "darwin") return macLink().catch(() => UNKNOWN_LINK);
  return Promise.resolve(UNKNOWN_LINK);
}

/**
 * The link, looked up again only when the interfaces' addresses changed or `force` is set (resume,
 * unlock: the same addresses can belong to another network). Lookups don't overlap.
 *
 * A pending lookup (a gateway but no MAC yet) isn't kept: the next call looks again. Until the MAC
 * turns up it carries the id the last full lookup found for the same gateway address, so a network
 * that merely woke up with an empty ARP table doesn't flip to "no id" and back (each a reconnect).
 * Another network behind the same address shows its own id once its MAC is known.
 */
export function linkWatcher(detect: () => Promise<LinkInfo> = () => detectLink(), fingerprint: () => string = () => interfacesFingerprint()): (force?: boolean) => Promise<LinkInfo> {
  let last: { print: string; link: LinkInfo } | undefined;
  let known: LinkInfo | undefined;
  let running: Promise<LinkInfo> | undefined;
  return async (force = false) => {
    const print = fingerprint();
    if (!force && last?.print === print && !last.link.pending) return last.link;
    if (running) return running;
    running = detect().then((found) => {
      let link = found;
      if (!found.pending) known = found.networkId ? found : undefined;
      else if (known && known.gateway === found.gateway) link = { ...found, networkId: known.networkId };
      last = { print, link };
      return link;
    });
    try {
      return await running;
    } finally {
      running = undefined;
    }
  };
}

/**
 * After a wake (resume, unlock): look now, and once more a few seconds later, when Wi-Fi has
 * usually joined again and the gateway answered ARP. A second wake before then starts over.
 */
export function wakeLookups(look: () => void, againMs = 5_000): () => void {
  let timer: ReturnType<typeof setTimeout> | undefined;
  return () => {
    if (timer) clearTimeout(timer);
    look();
    timer = setTimeout(() => {
      timer = undefined;
      look();
    }, againMs);
    timer.unref?.();
  };
}
