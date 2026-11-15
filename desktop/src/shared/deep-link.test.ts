import { describe, expect, it } from "vitest";
import { formatDeepLink, parseDeepLink } from "./deep-link";

describe("deep links", () => {
  it("accepts the documented hosts with a single-segment id", () => {
    expect(parseDeepLink("hocket://album/al-1")).toEqual({ kind: "album", id: "al-1" });
    expect(parseDeepLink("hocket://artist/ar_2")).toEqual({ kind: "artist", id: "ar_2" });
    expect(parseDeepLink("hocket://playlist/pl.3")).toEqual({ kind: "playlist", id: "pl.3" });
    expect(parseDeepLink("hocket://track/t4")).toEqual({ kind: "track", id: "t4" });
    expect(parseDeepLink("hocket://open")).toEqual({ kind: "open" });
    expect(parseDeepLink("HOCKET://Track/t4")).toEqual({ kind: "track", id: "t4" });
  });

  it("rejects other schemes, hosts, nested paths and odd ids", () => {
    expect(parseDeepLink("https://album/x")).toBeUndefined();
    expect(parseDeepLink("hocket://settings/x")).toBeUndefined();
    expect(parseDeepLink("hocket://track/")).toBeUndefined();
    expect(parseDeepLink("hocket://track/a/b")).toBeUndefined();
    expect(parseDeepLink("hocket://track/a%20b")).toBeUndefined();
    expect(parseDeepLink("hocket://open/x")).toBeUndefined();
    expect(parseDeepLink("not a url")).toBeUndefined();
    expect(parseDeepLink("hocket://track/" + "x".repeat(300))).toBeUndefined();
  });

  it("round-trips through the canonical form", () => {
    for (const u of ["hocket://album/a1", "hocket://track/t1", "hocket://open"]) expect(formatDeepLink(parseDeepLink(u)!)).toBe(u);
  });
});

describe("pairing and claim links", () => {
  it("keeps a pairing invite whole", () => {
    const text = "hocket://pair?d=eyJpZGVudGl0eSI6ImFiYyJ9_-x";
    expect(parseDeepLink(text)).toEqual({ kind: "pair", data: "eyJpZGVudGl0eSI6ImFiYyJ9_-x" });
    expect(formatDeepLink(parseDeepLink(text)!)).toBe(text);
  });

  it("rejects a pairing link that isn't one", () => {
    expect(parseDeepLink("hocket://pair")).toBeUndefined();
    expect(parseDeepLink("hocket://pair?d=a%20b")).toBeUndefined();
    expect(parseDeepLink("hocket://pair/x?d=eyJpZGVudGl0eSI6ImFiYyJ9")).toBeUndefined();
    expect(parseDeepLink("hocket://setup?server=https%3A%2F%2Fx&user=ada")).toBeUndefined();
  });

  it("reads a coordinator claim", () => {
    const text = "hocket://claim?url=wss%3A%2F%2Fc.example%2Fws&code=ABCD-EFGH";
    expect(parseDeepLink(text)).toEqual({ kind: "claim", url: "wss://c.example/ws", code: "ABCD-EFGH" });
    expect(formatDeepLink(parseDeepLink(text)!)).toBe(text);
    expect(parseDeepLink("hocket://claim?code=ABCD-EFGH")).toEqual({ kind: "claim", url: "", code: "ABCD-EFGH" });
    expect(parseDeepLink("hocket://claim?url=https%3A%2F%2Fx&code=ABCD")).toBeUndefined();
    expect(parseDeepLink("hocket://claim?url=wss%3A%2F%2Fx")).toBeUndefined();
  });
});
