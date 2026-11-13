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

describe("setup links", () => {
  it("round-trips every field in the form Android writes", () => {
    // The same link as SetupLinkTest.roundTripsEveryField on Android.
    const text = "hocket://setup?server=https%3A%2F%2Fmusic.example.net%2Fnavidrome&user=ada%20lovelace&password=p%26ss%3Dw0rd%2B%C3%BC&coordinator=wss%3A%2F%2Fcoord.example.net";
    const link = parseDeepLink(text);
    expect(link).toEqual({ kind: "setup", server: "https://music.example.net/navidrome", user: "ada lovelace", password: "p&ss=w0rd+ü", coordinator: "wss://coord.example.net" });
    expect(formatDeepLink(link!)).toBe(text);
  });

  it("leaves out what the link leaves out and normalises the server", () => {
    expect(parseDeepLink("HOCKET://setup?user=ada&server=Https%3A%2F%2Fmusic.example.net%2F&extra=1")).toEqual({ kind: "setup", server: "https://music.example.net", user: "ada" });
    expect(formatDeepLink({ kind: "setup", server: "https://x", user: "ada" })).toBe("hocket://setup?server=https%3A%2F%2Fx&user=ada");
  });

  it("carries packed settings, dropping a value that isn't packed", () => {
    const link = { kind: "setup", server: "https://x", user: "ada", settings: "eNqrVg_-" } as const;
    expect(formatDeepLink(link)).toBe("hocket://setup?server=https%3A%2F%2Fx&user=ada&settings=eNqrVg_-");
    expect(parseDeepLink(formatDeepLink(link))).toEqual(link);
    expect(parseDeepLink("hocket://setup?server=https%3A%2F%2Fx&user=ada&settings=a%20b")).toEqual({ kind: "setup", server: "https://x", user: "ada" });
  });

  it("rejects setup links without a usable server or user", () => {
    expect(parseDeepLink("hocket://setup?user=ada")).toBeUndefined();
    expect(parseDeepLink("hocket://setup?server=ftp%3A%2F%2Fx&user=ada")).toBeUndefined();
    expect(parseDeepLink("hocket://setup?server=https%3A%2F%2Fx&user=%20")).toBeUndefined();
    expect(parseDeepLink("hocket://setup?server=https%3A%2F%2Fx&user=ada%ZZ")).toBeUndefined();
    expect(parseDeepLink("hocket://setup/x?server=https%3A%2F%2Fx&user=ada")).toBeUndefined();
  });
});
