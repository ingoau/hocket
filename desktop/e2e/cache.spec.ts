// The stream cache on the desktop (FakeCore): album priming (dwell and play
// intent), Cached / Downloaded badges, "Available offline" (sidebar, Downloads
// tab, offline banner) and Settings → Storage.
import type { ElectronApplication, Page } from "@playwright/test";
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { completeSetup, expect, test } from "./fixtures";
import { expectNoViolations, serverId, setTheme } from "./a11y-helpers";

interface Prime { kind: string; id: string; trackId: string; outcome: string }

/** The primes FakeCore received, from its diagnostics (`prime <kind> <id> -> <track> <outcome>`). */
async function primes(page: Page): Promise<Prime[]> {
  const r = await page.evaluate(async () => (await window.hocket.query({ type: "diagnostics" })) as { type: string; data: string });
  return [...r.data.matchAll(/^ {2}prime (\w+) (\S+) -> (\S+) (\w+)/gm)].map((m) => ({ kind: m[1]!, id: m[2]!, trackId: m[3]!, outcome: m[4]! }));
}

async function openFirstAlbum(page: Page): Promise<string> {
  await page.getByTestId("nav-albums").click();
  const tile = page.getByTestId("grid-tile").first();
  await expect(tile).toBeVisible();
  const id = (await tile.getAttribute("data-id")) ?? "";
  await tile.dblclick();
  await expect(page.getByTestId("view-album")).toBeVisible();
  await expect(page.getByTestId("track-row").first()).toBeVisible();
  return id;
}

/** Albums whose tracks are all in the given offline state(s), via the core's queries. */
async function albumWhere(page: Page, pred: "none" | "someCached" | "someDownloaded"): Promise<string> {
  const sid = await serverId(page);
  return page.evaluate(async ({ sid, pred }) => {
    const albums = (await window.hocket.query({ type: "albums", data: { server_id: sid, sort: "default", descending: false, page: { offset: 0, limit: 300 } } })) as { data: { items: { id: string }[] } };
    for (const a of albums.data.items) {
      const tr = (await window.hocket.query({ type: "albumTracks", data: { id: a.id } })) as { data: { offline: string }[] };
      const states = tr.data.map((x) => x.offline);
      if (pred === "none" && states.every((s) => s === "none")) return a.id;
      if (pred === "someCached" && states.includes("cached")) return a.id;
      if (pred === "someDownloaded" && states.includes("downloaded")) return a.id;
    }
    return "";
  }, { sid, pred });
}

/** Open an album by deep link (main → renderer, like `hocket://album/<id>` from the OS). */
async function gotoAlbum(app: ElectronApplication, page: Page, id: string): Promise<void> {
  expect(id).not.toBe("");
  await app.evaluate(({ BrowserWindow }, albumId) => {
    for (const w of BrowserWindow.getAllWindows()) w.webContents.send("hocket:deep-link", `hocket://album/${albumId}`);
  }, id);
  await expect(page.getByTestId("view-album")).toBeVisible();
  await expect(page.getByTestId("track-row").first()).toBeVisible();
}

const SHOTS = process.env.HOCKET_SHOTS_DIR;
async function shot(page: Page, name: string): Promise<void> {
  if (!SHOTS) return;
  mkdirSync(SHOTS, { recursive: true });
  await page.screenshot({ path: join(SHOTS, `cache-desktop-${name}.png`) });
}

test.describe("album priming", () => {
  test("the album page primes its album once, after 2 s visible and focused", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    const albumId = await openFirstAlbum(page);
    const shownAt = Date.now();
    // Well inside the dwell: nothing yet.
    await page.waitForTimeout(1200);
    expect((await primes(page)).filter((p) => p.kind === "album")).toEqual([]);
    await expect.poll(async () => (await primes(page)).filter((p) => p.kind === "album").length, { timeout: 5000, intervals: [100] }).toBe(1);
    expect(Date.now() - shownAt).toBeGreaterThanOrEqual(1900);
    const [first] = (await primes(page)).filter((p) => p.kind === "album");
    expect(first).toMatchObject({ id: albumId, outcome: expect.stringMatching(/primed|skipped/) });
    // Staying longer doesn't prime again (once per visit).
    await page.waitForTimeout(2500);
    expect((await primes(page)).filter((p) => p.kind === "album")).toHaveLength(1);
    // A new visit may prime again (the core deduplicates).
    await page.getByTestId("nav-songs").click();
    await page.waitForTimeout(300);
    await page.getByTestId("nav-albums").click();
    await page.getByTestId("grid-tile").first().dblclick();
    await expect(page.getByTestId("view-album")).toBeVisible();
    await expect.poll(async () => (await primes(page)).filter((p) => p.kind === "album").length, { timeout: 5000 }).toBe(2);
  });

  test("leaving before 2 s never primes", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await openFirstAlbum(page);
    await page.waitForTimeout(1000);
    await page.getByTestId("nav-songs").click();
    await expect(page.getByTestId("view-songs")).toBeVisible();
    await page.waitForTimeout(2500);
    expect((await primes(page)).filter((p) => p.kind === "album")).toEqual([]);
  });

  test("resting on a track's play button (or focusing the album's) primes that track once", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await openFirstAlbum(page);
    const row = page.getByTestId("track-row").nth(2);
    const trackId = (await row.getAttribute("data-track-id")) ?? "";
    const play = row.getByTestId("row-play");
    // Passing over it doesn't count.
    await play.hover();
    await page.waitForTimeout(120);
    await page.getByTestId("view-album").locator("h1").hover();
    await page.waitForTimeout(500);
    expect((await primes(page)).filter((p) => p.kind === "track")).toEqual([]);
    // Resting on it does, after ~300 ms.
    await play.hover();
    await expect.poll(async () => (await primes(page)).filter((p) => p.kind === "track").map((p) => p.id), { timeout: 3000, intervals: [50] }).toEqual([trackId]);
    // Once per track per visit.
    await page.getByTestId("view-album").locator("h1").hover();
    await play.hover();
    await page.waitForTimeout(700);
    expect((await primes(page)).filter((p) => p.kind === "track")).toHaveLength(1);
    // Keyboard focus resting on the album's Play button primes the first track.
    const firstId = (await page.getByTestId("track-row").first().getAttribute("data-track-id")) ?? "";
    await page.getByTestId("album-play").focus();
    await expect.poll(async () => (await primes(page)).filter((p) => p.kind === "track").map((p) => p.id), { timeout: 3000 }).toContain(firstId);
  });

  test("a grid scrolling under a resting pointer never primes", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await page.getByTestId("nav-albums").click();
    const tile = page.getByTestId("grid-tile").nth(1);
    await expect(tile).toBeVisible();
    const box = (await tile.locator(".play").boundingBox())!;
    // Park the pointer where play buttons pass, then scroll the grid under it.
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.waitForTimeout(100);
    const before = (await primes(page)).length;
    // Scroll exactly one row pitch in small steps: play buttons cross the pointer and one ends under it.
    const pitch = await page.evaluate(() => {
      const tiles = Array.from(document.querySelectorAll<HTMLElement>('[data-testid="grid-tile"]'));
      const y0 = tiles[0]!.getBoundingClientRect().top;
      const next = tiles.find((t) => t.getBoundingClientRect().top > y0 + 10)!;
      return next.getBoundingClientRect().top - y0;
    });
    const steps = 12;
    for (let i = 1; i <= steps; i++) {
      await page.getByTestId("albums-grid").evaluate((el, y) => { el.scrollTop = y; }, Math.round((pitch * i) / steps));
      await page.waitForTimeout(40);
    }
    // Long enough for the browser's post-scroll hover update and the dwell after it.
    await page.waitForTimeout(1500);
    expect(await primes(page)).toHaveLength(before);
    // Moving the pointer on the play button now under it is intent again.
    await page.mouse.move(box.x + box.width / 2 + 4, box.y + box.height / 2, { steps: 2 });
    await expect.poll(async () => (await primes(page)).length, { timeout: 3000 }).toBe(before + 1);
  });
});

test.describe("cached and downloaded badges", () => {
  test("album tracks and the queue show Cached / Downloaded, and playing fills the cache", async ({ hocket }) => {
    const { page, app } = hocket;
    await completeSetup(page);
    // An album with some cached tracks: its rows say so, by name.
    await gotoAlbum(app, page, await albumWhere(page, "someCached"));
    const cached = page.getByTestId("album-tracks").locator('[data-testid="offline-badge"][data-state="cached"]');
    await expect(cached.first()).toBeVisible();
    await expect(cached.first().getByRole("img", { name: "Cached" })).toBeVisible();
    const cachedIcon = await cached.first().locator("svg").getAttribute("data-icon");
    // A downloaded album: every row is Downloaded, with a different icon.
    await gotoAlbum(app, page, await albumWhere(page, "someDownloaded"));
    const dl = page.getByTestId("album-tracks").locator('[data-testid="offline-badge"][data-state="downloaded"]');
    await expect(dl.first().getByRole("img", { name: "Downloaded" })).toBeVisible();
    expect(await dl.first().locator("svg").getAttribute("data-icon")).not.toBe(cachedIcon);
    for (const theme of ["light", "dark"] as const) {
      await setTheme(page, theme);
      await expectNoViolations(page, `${theme} album with downloaded badges`);
    }
    // Play an album with nothing on disk: after a few seconds the current
    // track is complete in the cache and the next two are prefetched.
    await gotoAlbum(app, page, await albumWhere(page, "none"));
    await expect(page.getByTestId("album-tracks").getByTestId("offline-badge")).toHaveCount(0);
    await page.getByTestId("album-play").click();
    await expect(page.getByTestId("queue-row-current")).toBeVisible();
    await expect(page.getByTestId("queue-row-current").locator('[data-testid="offline-badge"][data-state="cached"]')).toBeVisible({ timeout: 12_000 });
    await expect(page.getByTestId("queue-row-upcoming").nth(1).locator('[data-state="cached"]')).toBeVisible();
    await expect(page.getByTestId("album-tracks").locator('[data-state="cached"]')).toHaveCount(3, { timeout: 5000 });
    await shot(page, "album");
    for (const theme of ["light", "dark"] as const) {
      await setTheme(page, theme);
      await expectNoViolations(page, `${theme} album with cached badges and queue`);
    }
  });
});

test.describe("available offline", () => {
  test("the built-in filter is in the sidebar and opens the Downloads tab listing what plays offline", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    const nav = page.getByTestId("nav-available-offline");
    await expect(nav).toHaveText("Available offline");
    await nav.click();
    await expect(page.getByTestId("view-downloads")).toBeVisible();
    await expect(page.getByRole("tab", { name: "Available offline" })).toHaveAttribute("aria-selected", "true");
    await expect(nav).toHaveAttribute("aria-current", "page");
    const table = page.getByTestId("available-offline-table");
    await expect(table.getByTestId("track-row").first()).toBeVisible();
    const count = async () => Number(((await page.getByTestId("available-offline-count").textContent()) ?? "0").replace(/\D/g, ""));
    const all = await count();
    expect(all).toBeGreaterThan(0);
    // Every row is downloaded or cached.
    const rows = table.getByTestId("track-row");
    const n = await rows.count();
    for (let i = 0; i < Math.min(n, 10); i++) await expect(rows.nth(i).getByTestId("offline-badge")).toBeVisible();
    for (const theme of ["light", "dark"] as const) {
      await setTheme(page, theme);
      await expectNoViolations(page, `${theme} downloads / available offline`);
    }
    // The tabs switch with the keyboard.
    await page.getByRole("tab", { name: "Available offline" }).focus();
    await page.keyboard.press("ArrowLeft");
    await expect(page.getByRole("tab", { name: "Downloads" })).toHaveAttribute("aria-selected", "true");
    await page.keyboard.press("ArrowRight");
    await expect(page.getByTestId("available-offline-table")).toBeVisible();
    // Clearing the stream cache leaves only downloads.
    await page.getByRole("button", { name: "Clear stream cache" }).click();
    await expect.poll(count).toBeLessThan(all);
    await expect(table.locator('[data-state="cached"]')).toHaveCount(0);
  });

  test("offline, the notices show as a banner that leads to what's available", async ({ hocket }) => {
    const { page, app } = hocket;
    await completeSetup(page);
    const noneAlbum = await albumWhere(page, "none");
    const sid = await serverId(page);
    await page.evaluate(() => window.hocket.dispatch({ type: "setAutoplay", data: { enabled: false } }));
    // Go offline the way the app learns it (the renderer's online/offline events).
    await page.context().setOffline(true).catch(async () => {
      await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]?.webContents.session.enableNetworkEmulation({ offline: true }));
    });
    await expect(page.getByTestId("offline-banner")).toBeVisible();
    // Nothing on this album plays offline: skipped, then nothing left.
    await page.evaluate(({ sid, id }) => window.hocket.dispatch({ type: "playContext", data: { args: { context: { serverId: sid, kind: { type: "album", data: { id } }, label: "e2e", sort: "default", tracks: [] }, startIndex: 0, shuffle: false, saveOutgoing: false } } }), { sid, id: noneAlbum });
    await expect(page.getByTestId("offline-banner-text")).toHaveText(/Offline: skipping tracks that aren't downloaded or cached|Nothing in the queue is available offline/);
    await expect(page.getByTestId("offline-banner-text")).toHaveText("Nothing in the queue is available offline", { timeout: 10_000 });
    await expect(page.getByTestId("offline-live")).toHaveAttribute("role", "status");
    // The player bar doesn't repeat it.
    await expect(page.getByTestId("player-bar").getByText("Nothing in the queue is available offline")).toHaveCount(0);
    await expectNoViolations(page, "offline banner");
    await page.getByTestId("offline-banner-show").click();
    await expect(page.getByTestId("available-offline")).toBeVisible();
    // Playing from what's available works offline: no skipping notice.
    await page.getByTestId("available-offline-table").getByTestId("track-row").first().locator(".td.title").dblclick();
    await expect(page.getByTestId("queue-row-current")).toBeVisible();
    await expect(page.getByTestId("offline-banner-text")).toHaveText("Offline");
    // Back online: the core's NetworkChanged takes the banner away.
    await page.context().setOffline(false).catch(async () => {
      await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]?.webContents.session.enableNetworkEmulation({ offline: false }));
    });
    await expect(page.getByTestId("offline-banner")).toHaveCount(0);
  });
});

test.describe("settings: storage", () => {
  test("cache usage, automatic vs custom budget, data saved and prefetch on mobile data", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await page.getByTestId("nav-settings").click();
    await page.getByTestId("settings-nav-storage").click();
    const usage = page.getByTestId("cache-usage");
    await expect(usage).toContainText(/of 2\.0 GB: .* complete songs, .* partial/);
    const mode = page.getByTestId("cache-budget-mode");
    await expect(mode).toHaveValue("auto");
    await expect(mode.locator("option[value=auto]")).toHaveText("Automatic (currently 2.0 GB)");
    await expect(page.getByTestId("data-saved")).toHaveText(/\d/);
    const prefetch = page.getByRole("checkbox", { name: "Prefetch upcoming songs on mobile data" });
    await expect(prefetch).not.toBeChecked();
    await prefetch.check();
    await expect(prefetch).toBeChecked();
    const setting = await page.evaluate(async () => (await window.hocket.query({ type: "setting", data: { key: "storage.prefetchOnMobileData" } })) as { data?: { value: string } });
    expect(setting.data?.value).toBe("true");
    await shot(page, "storage");
    for (const theme of ["light", "dark"] as const) {
      await setTheme(page, theme);
      await expectNoViolations(page, `${theme} settings/storage`);
    }
    const budget = () => page.evaluate(async () => {
      const r = (await window.hocket.query({ type: "storage" })) as { data?: { cacheBudgetAuto?: boolean } };
      const s = (await window.hocket.query({ type: "setting", data: { key: "storage.cacheMaxBytes" } })) as { data?: { value: string } };
      return { auto: r.data?.cacheBudgetAuto, value: s.data?.value };
    });
    expect(await budget()).toEqual({ auto: true, value: "null" });
    // Custom starts at the current size: 2 GB is a size of its own (null is automatic).
    await mode.selectOption("custom");
    const gb = page.getByTestId("cache-budget-gb");
    await expect(gb).toHaveValue("2");
    await expect.poll(budget).toEqual({ auto: false, value: String(2 * 1024 ** 3) });
    await expect(usage).toContainText("of 2.0 GB");
    await gb.fill("4");
    await gb.press("Enter");
    await expect(usage).toContainText("of 4.0 GB");
    // A small budget evicts complete entries.
    const before = await usage.textContent();
    await gb.fill("0.25");
    await gb.press("Enter");
    await expect(usage).toContainText("of 256.0 MB");
    await expect(usage).not.toHaveText(before ?? "");
    await expect(mode).toHaveValue("custom");
    // Back to automatic.
    await mode.selectOption("auto");
    await expect(usage).toContainText("of 2.0 GB");
    await expect(gb).toHaveCount(0);
    await expect.poll(budget).toEqual({ auto: true, value: "null" });
    await shot(page, "storage-after");
  });
});
