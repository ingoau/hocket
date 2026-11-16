import { EventEmitter } from "node:events";
import { afterEach, describe, expect, it, vi } from "vitest";

/** The sign-in session: its cookies, and how often it was cleared. */
const ses = {
  cleared: 0,
  jar: [] as { name: string; value: string }[],
  setUserAgent: () => undefined,
  clearStorageData: async () => {
    ses.cleared++;
    ses.jar = [];
  },
  cookies: { get: async () => ses.jar },
};
const partitions: string[] = [];

class FakeContents extends EventEmitter {
  url = "";
  openHandler: ((d: { url: string }) => { action: string }) | undefined;
  setUserAgent = () => undefined;
  setWindowOpenHandler(h: (d: { url: string }) => { action: string }) {
    this.openHandler = h;
  }
  getURL() {
    return this.url;
  }
  /** A page-initiated navigation: whether it was let through. */
  navigate(url: string): boolean {
    let prevented = false;
    this.emit("will-navigate", { url, preventDefault: () => (prevented = true) });
    return !prevented;
  }
}

const wins: FakeWin[] = [];
class FakeWin extends EventEmitter {
  webContents = new FakeContents();
  destroyed = false;
  focused = 0;
  loaded: string[] = [];
  constructor() {
    super();
    wins.push(this);
  }
  async loadURL(url: string) {
    this.loaded.push(url);
    this.webContents.url = url;
  }
  isDestroyed() {
    return this.destroyed;
  }
  focus() {
    this.focused++;
  }
  close() {
    this.destroyed = true;
    this.emit("closed");
  }
}

vi.mock("electron", () => ({
  BrowserWindow: FakeWin,
  session: {
    fromPartition: (p: string) => {
      partitions.push(p);
      return ses;
    },
  },
}));

const { cookieHeader, hasSession, isSignInContents, isSignInUrl, isYouTubeMusic, signInToYouTubeMusic } = await import("./ytm-signin");

const tick = () => new Promise((r) => setTimeout(r, 0));
async function openWindow(): Promise<FakeWin> {
  for (let i = 0; i < 10 && !wins.length; i++) await tick();
  const w = wins.at(-1);
  if (!w) throw new Error("no sign-in window");
  return w;
}

afterEach(() => {
  wins.length = 0;
  partitions.length = 0;
  ses.cleared = 0;
  ses.jar = [];
});

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

  it("lets the window go only to Google's and YouTube's own https pages", () => {
    for (const ok of [
      "https://accounts.google.com/CheckCookie?continue=https%3A%2F%2Fmusic.youtube.com%2F",
      "https://music.youtube.com/",
      "https://consent.youtube.com/m?continue=x",
      "https://consent.google.com/ml",
      "https://www.youtube.com/signin",
      "https://accounts.google.co.uk/",
      "https://www.google.de/",
      "https://www.google.com.au/",
      "https://support.google.com/accounts",
    ]) expect(isSignInUrl(ok), ok).toBe(true);
    for (const bad of [
      "http://accounts.google.com/",
      "https://evil.example/",
      "https://google.com.evil.example/",
      "https://accounts.google.com.evil.example/",
      "https://notyoutube.com/",
      "https://accounts.google.com:8443/",
      "https://user@accounts.google.com/",
      "javascript:alert(1)",
      "file:///etc/passwd",
      "not a url",
    ]) expect(isSignInUrl(bad), bad).toBe(false);
  });

  it("signs in through Google's redirects, then clears the in-memory session", async () => {
    const result = signInToYouTubeMusic();
    const win = await openWindow();
    expect(partitions).toEqual(["hocket-ytm-signin"]);
    expect(isSignInContents(win.webContents as never)).toBe(true);
    expect(ses.cleared).toBe(1);
    // Google's own page-initiated hops go ahead; anywhere else doesn't.
    expect(win.webContents.navigate("https://accounts.google.com/CheckCookie")).toBe(true);
    expect(win.webContents.navigate("https://evil.example/")).toBe(false);
    expect(win.webContents.openHandler?.({ url: "https://evil.example/" })).toEqual({ action: "deny" });
    expect(win.loaded).toHaveLength(1);
    // Lands on YouTube Music signed in.
    ses.jar = [{ name: "SAPISID", value: "abc" }, { name: "SID", value: "s" }];
    win.webContents.url = "https://music.youtube.com/";
    win.webContents.emit("did-navigate");
    await expect(result).resolves.toBe("SAPISID=abc; SID=s");
    expect(win.destroyed).toBe(true);
    expect(ses.cleared).toBe(2);
    expect(ses.jar).toEqual([]);
  });

  it("clears the session when the window is closed first", async () => {
    const result = signInToYouTubeMusic();
    const win = await openWindow();
    ses.jar = [{ name: "NID", value: "half-way" }];
    win.close();
    await expect(result).resolves.toBeUndefined();
    expect(ses.cleared).toBe(2);
    expect(ses.jar).toEqual([]);
  });

  it("opens one sign-in at a time: another request focuses it and is busy", async () => {
    const first = signInToYouTubeMusic();
    const second = signInToYouTubeMusic();
    await expect(second).resolves.toBe("busy");
    const win = await openWindow();
    await expect(signInToYouTubeMusic()).resolves.toBe("busy");
    expect(win.focused).toBe(1);
    expect(wins).toHaveLength(1);
    win.close();
    await expect(first).resolves.toBeUndefined();
    // Done: the next one opens a window again.
    wins.length = 0;
    const third = signInToYouTubeMusic();
    const again = await openWindow();
    expect(again).not.toBe(win);
    again.close();
    await expect(third).resolves.toBeUndefined();
  });
});
