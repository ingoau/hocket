// YouTube Music sign-in (docs/providers.md, Phase 4): a BrowserWindow on
// Google's sign-in page with a session of its own, closed once it lands on
// music.youtube.com signed in. Its cookies for music.youtube.com become the
// account's `Cookie:` header, which the core keeps in memory and the
// credential store keeps OS-encrypted on this device only (never synced).
//
// Google refuses sign-in in browsers it takes for embedded ones, so the
// window presents a plain Chrome user agent (no Electron or app tokens).
// Whether that's enough is a manual check (docs/spikes/manual.md, 2).
import { BrowserWindow, session } from "electron";

/** Where the window starts; Google sends it on to music.youtube.com once signed in. */
export const SIGN_IN_URL = "https://accounts.google.com/ServiceLogin?ltmpl=music&service=youtube&passive=true&continue=https%3A%2F%2Fmusic.youtube.com%2F";
const PARTITION = "persist:hocket-ytm-signin";
/** A current stable Chrome's user agent, without Electron's tokens. */
export const CHROME_UA = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";

export interface CookieLike {
  name: string;
  value: string;
}

/** The cookies a signed-in YouTube session has: `SAPISID` (or its secure twin). */
export function hasSession(cookies: CookieLike[]): boolean {
  return cookies.some((c) => (c.name === "SAPISID" || c.name === "__Secure-3PAPISID") && c.value.length > 0);
}

/** A `Cookie:` header from cookies, first of each name. */
export function cookieHeader(cookies: CookieLike[]): string {
  const seen = new Set<string>();
  const parts: string[] = [];
  for (const c of cookies) {
    if (seen.has(c.name) || !/^[!#-'*+.0-9A-Z^-z|~-]+$/.test(c.name)) continue;
    seen.add(c.name);
    parts.push(`${c.name}=${c.value}`);
  }
  return parts.join("; ");
}

/** Whether a URL is YouTube Music itself (where a finished sign-in lands). */
export function isYouTubeMusic(url: string): boolean {
  try {
    return new URL(url).hostname === "music.youtube.com";
  } catch {
    return false;
  }
}

/**
 * Open the sign-in window and resolve with the cookie header once signed in,
 * or undefined when the window is closed first. `HOCKET_FAKE_YTM_SIGNIN`
 * (e2e) answers at once with fake cookies, without a window.
 */
export async function signInToYouTubeMusic(parent?: BrowserWindow): Promise<string | undefined> {
  if (process.env.HOCKET_FAKE_YTM_SIGNIN === "1") return "SAPISID=fake-e2e; SID=fake-e2e";
  const ses = session.fromPartition(PARTITION);
  ses.setUserAgent(CHROME_UA);
  // A fresh sign-in each time: the session a sign-in came from must not be reused.
  await ses.clearStorageData();
  const win = new BrowserWindow({
    parent,
    modal: !!parent,
    width: 480,
    height: 720,
    title: "Sign in to YouTube Music",
    autoHideMenuBar: true,
    webPreferences: { session: ses, sandbox: true, contextIsolation: true, nodeIntegration: false },
  });
  win.webContents.setUserAgent(CHROME_UA);
  // Links that open new windows stay in this one (Google's help pages).
  win.webContents.setWindowOpenHandler(({ url }) => {
    void win.loadURL(url);
    return { action: "deny" };
  });
  return new Promise((resolve) => {
    let done = false;
    const finish = (cookies?: string) => {
      if (done) return;
      done = true;
      resolve(cookies);
      if (!win.isDestroyed()) win.close();
    };
    const check = async () => {
      if (done || win.isDestroyed() || !isYouTubeMusic(win.webContents.getURL())) return;
      const cookies = await ses.cookies.get({ url: "https://music.youtube.com" });
      if (hasSession(cookies)) finish(cookieHeader(cookies));
    };
    win.webContents.on("did-navigate", () => void check());
    win.webContents.on("did-finish-load", () => void check());
    win.on("closed", () => finish(undefined));
    void win.loadURL(SIGN_IN_URL).catch((err: unknown) => console.error("[ytm] sign-in page failed to load", err));
  });
}
