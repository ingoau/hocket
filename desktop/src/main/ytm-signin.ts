// YouTube Music sign-in (docs/providers.md, Phase 4): a BrowserWindow on
// Google's sign-in page with an in-memory session of its own, closed once it
// lands on music.youtube.com signed in. Its cookies for music.youtube.com
// become the account's `Cookie:` header, which the core keeps in memory and
// the credential store keeps OS-encrypted on this device only (never synced).
// The session is never written to disk and is cleared as soon as the window
// is done (signed in or not), so the Google session it holds lives on only
// as that header.
//
// Google refuses sign-in in browsers it takes for embedded ones, so the
// window presents a plain Chrome user agent (no Electron or app tokens).
// Whether that's enough is a manual check (docs/spikes/manual.md, 2).
//
// The window may only go to Google's and YouTube's own pages (https); the
// app's global "never navigate" rule (index.ts) leaves it alone through
// [isSignInContents]. One sign-in at a time: a second request focuses the
// open window instead.
import { BrowserWindow, session, type WebContents } from "electron";

/** Where the window starts; Google sends it on to music.youtube.com once signed in. */
export const SIGN_IN_URL = "https://accounts.google.com/ServiceLogin?ltmpl=music&service=youtube&passive=true&continue=https%3A%2F%2Fmusic.youtube.com%2F";
/** Not `persist:`: the sign-in's cookies and storage stay in memory, never in userData/Partitions. */
const PARTITION = "hocket-ytm-signin";
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
 * Whether the sign-in window may go to `url`: https on Google's or YouTube's own hosts
 * (accounts.google.com and its country domains such as google.co.uk, consent pages,
 * music.youtube.com where it ends).
 */
export function isSignInUrl(url: string): boolean {
  let u: URL;
  try {
    u = new URL(url);
  } catch {
    return false;
  }
  if (u.protocol !== "https:" || u.username || u.password || u.port) return false;
  const host = u.hostname.toLowerCase();
  return /(^|\.)youtube\.com$/.test(host) || /(^|\.)google\.(com|[a-z]{2}|co\.[a-z]{2}|com\.[a-z]{2})$/.test(host);
}

/** The sign-in windows' contents, which handle their own navigation (not the app's global rule). */
const signInContents = new WeakSet<WebContents>();
export const isSignInContents = (contents: WebContents): boolean => signInContents.has(contents);

/** The open sign-in's window, once made (a second request focuses it). */
let active: { win?: BrowserWindow } | undefined;

/**
 * Open the sign-in window and resolve with the cookie header once signed in,
 * or undefined when the window is closed first. `HOCKET_FAKE_YTM_SIGNIN`
 * (e2e) answers at once with fake cookies, without a window. While one is open,
 * a second call focuses it and resolves "busy" at once: two windows would share
 * (and clear) one session, and the second caller must not get the first's account.
 */
export async function signInToYouTubeMusic(parent?: BrowserWindow): Promise<string | undefined | "busy"> {
  if (process.env.HOCKET_FAKE_YTM_SIGNIN === "1") return "SAPISID=fake-e2e; SID=fake-e2e";
  if (active) {
    if (active.win && !active.win.isDestroyed()) active.win.focus();
    return "busy";
  }
  const entry: { win?: BrowserWindow } = {};
  active = entry;
  const ses = session.fromPartition(PARTITION);
  try {
    ses.setUserAgent(CHROME_UA);
    // A fresh sign-in each time: the session a sign-in came from must not be reused.
    await ses.clearStorageData();
    return await signInWindow(ses, entry, parent);
  } finally {
    // Signed in or not, the Google session is dropped: only the header (if any) lives on.
    await ses.clearStorageData().catch((err: unknown) => console.error("[ytm] couldn't clear the sign-in session", err));
    if (active === entry) active = undefined;
  }
}

function signInWindow(ses: Electron.Session, entry: { win?: BrowserWindow }, parent?: BrowserWindow): Promise<string | undefined> {
  const win = new BrowserWindow({
    parent,
    modal: !!parent,
    width: 480,
    height: 720,
    title: "Sign in to YouTube Music",
    autoHideMenuBar: true,
    webPreferences: { session: ses, sandbox: true, contextIsolation: true, nodeIntegration: false },
  });
  entry.win = win;
  signInContents.add(win.webContents);
  win.webContents.setUserAgent(CHROME_UA);
  // Google's own redirects (CheckCookie → music.youtube.com, consent) go ahead; nothing else.
  win.webContents.on("will-navigate", (e) => {
    if (!isSignInUrl(e.url)) e.preventDefault();
  });
  // Links that open new windows stay in this one (Google's help pages), if they're Google's.
  win.webContents.setWindowOpenHandler(({ url }) => {
    if (isSignInUrl(url)) void win.loadURL(url).catch(() => undefined);
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
