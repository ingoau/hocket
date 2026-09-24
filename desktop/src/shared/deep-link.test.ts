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
