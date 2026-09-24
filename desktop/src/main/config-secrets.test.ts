import { describe, expect, it } from "vitest";
import type { ConfigDocument, ServerInfo } from "@core/api";
import { extractSecrets, parseConfigDocument, resolveSecretReferences } from "./config-secrets";

const server = (id: string, url: string, username: string) => ({ id, url, username, name: `srv ${id}`, reachable: true }) as unknown as ServerInfo;

const doc = (secrets?: Record<string, string>): ConfigDocument => ({
  version: 1,
  exportedAt: 0,
  settings: [],
  filters: [],
  shortcuts: [],
  servers: [server("s1", "https://a.example", "alice"), server("s2", "https://b.example", "bob")],
  secrets,
  audio: { replayGain: "off", replayGainPreampDb: 0, normalisation: false, eq: { enabled: false, preampDb: 0, bands: [], preset: undefined }, gapless: true, outputDevice: undefined, exclusive: false },
  autoplay: { chain: [], seedWindow: 5, minSimilarity: undefined, exclusionWindow: 0, filterId: undefined },
});

describe("config secrets", () => {
  it("resolves keystore references from the credential store and drops what it can't", () => {
    const d = doc({ "server:s1:password": "keystore://hocket/server/s1/password", "server:s2:password": "keystore://hocket/server/s2/password" });
    const r = resolveSecretReferences(d, (url, user) => (url === "https://a.example" && user === "alice" ? "pw-a" : undefined));
    expect(r.resolved).toBe(1);
    expect(r.unresolved).toBe(1);
    expect(r.document.secrets).toEqual({ "server:s1:password": "pw-a" });
    // Nothing resolvable: the map goes away entirely.
    const none = resolveSecretReferences(d, () => undefined);
    expect(none.document.secrets).toBeUndefined();
    expect(resolveSecretReferences(doc(undefined), () => "x").resolved).toBe(0);
  });

  it("turns plain-text secrets back into servers to add and strips them from what the core sees", () => {
    const d = doc({ "server:s1:password": "pw-a", "server:s2:password": "keystore://hocket/server/s2/password", "server:zzz:password": "orphan" });
    const { document, servers } = extractSecrets(d);
    expect(servers).toEqual([{ url: "https://a.example", username: "alice", name: "srv s1", password: "pw-a" }]);
    expect(document.secrets).toBeUndefined();
    expect(document.servers).toHaveLength(2);
  });

  it("round-trips export → import", () => {
    const exported = resolveSecretReferences(doc({ "server:s1:password": "keystore://hocket/server/s1/password" }), () => "secret").document;
    const { servers } = extractSecrets(parseConfigDocument(JSON.stringify(exported)));
    expect(servers.map((s) => s.password)).toEqual(["secret"]);
  });

  it("rejects things that aren't documents", () => {
    expect(() => parseConfigDocument("[]")).toThrow();
    expect(() => parseConfigDocument("null")).toThrow();
    expect(() => parseConfigDocument("{\"secrets\": []}")).toThrow();
    expect(() => parseConfigDocument("{\"servers\": 1}")).toThrow();
    expect(() => parseConfigDocument("nope")).toThrow();
  });
});
