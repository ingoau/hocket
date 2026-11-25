import { describe, expect, it, vi } from "vitest";
import type { NetworkState } from "@core/api";
import { linkWatcher, linuxKind, networkIdOf, networkReporter, parseMacArp, parseMacHardwarePorts, parseMacRoute, parseNmMetered, parseProcArp, parseProcRoute, type LinkInfo } from "./network";

const wifi = (networkId: string): LinkInfo => ({ kind: "wifi", metered: false, networkId });

describe("network reporter", () => {
  it("passes on changes only", () => {
    const pushed: NetworkState[] = [];
    const report = networkReporter((s) => pushed.push(s));
    report(true);
    report(true);
    report(true);
    report(false);
    report(false);
    report(true);
    expect(pushed.map((s) => s.kind)).toEqual(["unknown", "offline", "unknown"]);
  });

  it("reports another network with the same kind (woke on another Wi-Fi)", () => {
    const pushed: NetworkState[] = [];
    const report = networkReporter((s) => pushed.push(s));
    report(true, wifi("a"));
    report(true, wifi("a"));
    report(true, wifi("b"));
    report(true, { kind: "wifi", metered: true, networkId: "b" });
    expect(pushed).toEqual([
      { kind: "wifi", metered: false, networkId: "a" },
      { kind: "wifi", metered: false, networkId: "b" },
      { kind: "wifi", metered: true, networkId: "b" },
    ]);
  });

  it("offline carries no link details", () => {
    const pushed: NetworkState[] = [];
    networkReporter((s) => pushed.push(s))(false, { kind: "wifi", metered: true, networkId: "a" });
    expect(pushed).toEqual([{ kind: "offline", metered: false, networkId: undefined }]);
  });
});

describe("network id", () => {
  it("hashes the gateway's MAC, else its address, and never leaks either", () => {
    const a = networkIdOf({ mac: "AA:BB:CC:00:11:22", ip: "192.168.1.1" });
    expect(a).toBe(networkIdOf({ mac: "aa:bb:cc:00:11:22", ip: "10.0.0.1" }));
    expect(a).not.toContain("aa:bb");
    expect(networkIdOf({ mac: "00:00:00:00:00:00", ip: "192.168.1.1" })).toBe(networkIdOf({ ip: "192.168.1.1" }));
    expect(networkIdOf({})).toBeUndefined();
  });
});

describe("linux", () => {
  const route = [
    "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT",
    "enp3s0\t00000000\t0101A8C0\t0003\t0\t0\t100\t00000000\t0\t0\t0",
    "wlp2s0\t00000000\t0100000A\t0003\t0\t0\t600\t00000000\t0\t0\t0",
    "wlp2s0\t0000000A\t00000000\t0001\t0\t0\t600\t00FFFFFF\t0\t0\t0",
  ].join("\n");

  it("takes the default route with the lowest metric", () => {
    expect(parseProcRoute(route)).toEqual({ iface: "enp3s0", gateway: "192.168.1.1" });
    expect(parseProcRoute(route.split("\n").slice(0, 1).join("\n"))).toBeUndefined();
  });

  it("finds the gateway's MAC on the route's interface", () => {
    const arp = ["IP address       HW type     Flags       HW address            Mask     Device", "192.168.1.1      0x1         0x2         a4:2b:b0:12:34:56     *        enp3s0", "192.168.1.1      0x1         0x0         00:00:00:00:00:00     *        wlp2s0"].join("\n");
    expect(parseProcArp(arp, "192.168.1.1", "enp3s0")).toBe("a4:2b:b0:12:34:56");
    expect(parseProcArp(arp, "192.168.1.1", "wlp2s0")).toBeUndefined();
  });

  it("tells the kind from sysfs", () => {
    const files: Record<string, string> = { "/sys/class/net/wlp2s0/phy80211": "", "/sys/class/net/enp3s0/type": "1\n", "/sys/class/net/enp3s0/device": "", "/sys/class/net/tun0/type": "65534\n", "/sys/class/net/br0/type": "1\n" };
    const has = (p: string) => p in files;
    const read = (p: string) => files[p];
    expect(linuxKind("wlp2s0", has, read)).toBe("wifi");
    expect(linuxKind("enp3s0", has, read)).toBe("wired");
    expect(linuxKind("wwan0", has, read)).toBe("cellular");
    expect(linuxKind("tun0", has, read)).toBe("unknown");
    expect(linuxKind("br0", has, read)).toBe("unknown");
  });

  it("reads NetworkManager's metered flag", () => {
    expect(parseNmMetered("yes (guessed)\n")).toBe(true);
    expect(parseNmMetered("yes")).toBe(true);
    expect(parseNmMetered("no (guessed)")).toBe(false);
    expect(parseNmMetered("unknown")).toBe(false);
  });
});

describe("macOS", () => {
  it("parses the default route, the hardware ports and the gateway's MAC", () => {
    expect(parseMacRoute("   route to: default\ndestination: default\n    gateway: 192.168.1.1\n  interface: en0\n")).toEqual({ iface: "en0", gateway: "192.168.1.1" });
    expect(parseMacRoute("route: writing to routing socket: not in table")).toBeUndefined();
    const ports = "Hardware Port: Wi-Fi\nDevice: en0\nEthernet Address: x\n\nHardware Port: Thunderbolt Ethernet Slot 1\nDevice: en5\nEthernet Address: y\n\nHardware Port: iPhone USB\nDevice: en7\n";
    expect(parseMacHardwarePorts(ports, "en0")).toBe("wifi");
    expect(parseMacHardwarePorts(ports, "en5")).toBe("wired");
    expect(parseMacHardwarePorts(ports, "en7")).toBe("cellular");
    expect(parseMacHardwarePorts(ports, "utun3")).toBe("unknown");
    expect(parseMacArp("? (192.168.1.1) at a4:2b:0:1:34:56 on en0 ifscope [ethernet]\n")).toBe("a4:2b:00:01:34:56");
    expect(parseMacArp("192.168.1.1 (192.168.1.1) -- no entry")).toBeUndefined();
  });
});

describe("link watcher", () => {
  it("looks the link up again only when the interfaces change or when forced", async () => {
    let print = "a";
    const detect = vi.fn(async () => wifi(print));
    const look = linkWatcher(detect, () => print);
    expect(await look()).toEqual(wifi("a"));
    expect(await look()).toEqual(wifi("a"));
    expect(detect).toHaveBeenCalledTimes(1);
    print = "b";
    expect(await look()).toEqual(wifi("b"));
    await look(true);
    expect(detect).toHaveBeenCalledTimes(3);
  });
});
