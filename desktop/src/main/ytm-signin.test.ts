import { describe, expect, it, vi } from "vitest";

vi.mock("electron", () => ({ BrowserWindow: class {}, session: {} }));

const { cookieHeader, hasSession, isYouTubeMusic } = await import("./ytm-signin");

describe("YouTube Music sign-in", () => {
  it("knows a signed-in session by its SAPISID", () => {
    expect(hasSession([{ name: "PREF", value: "x" }])).toBe(false);
    expect(hasSession([{ name: "SAPISID", value: "" }])).toBe(false);
    expect(hasSession([{ name: "SAPISID", value: "abc/def" }])).toBe(true);
    expect(hasSession([{ name: "__Secure-3PAPISID", value: "z" }])).toBe(true);
  });

  it("builds a cookie header, first of each name, skipping malformed names", () => {
    expect(cookieHeader([{ name: "SID", value: "1" }, { name: "SAPISID", value: "a/b" }, { name: "SID", value: "2" }, { name: "bad name", value: "x" }])).toBe("SID=1; SAPISID=a/b");
  });

  it("recognises where a finished sign-in lands", () => {
    expect(isYouTubeMusic("https://music.youtube.com/")).toBe(true);
    expect(isYouTubeMusic("https://accounts.google.com/ServiceLogin")).toBe(false);
    expect(isYouTubeMusic("not a url")).toBe(false);
  });
});
