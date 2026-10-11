// The now playing surfaces (#47): the side panel's overview and drill-ins, the
// player bar's lyrics/queue/side panel buttons, the queue selector, the
// Connect pill, the rating setting, and the fullscreen player's remembered
// view and idle fade.
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import type { Page } from "@playwright/test";
import type { DeviceInfo } from "../src/core/api";
import { encodePng } from "../src/main/png";
import { completeSetup, expect, launchFake, playFirstAlbum, test } from "./fixtures";

/** Plays the `nth` album from its page, as a user does (which keeps the outgoing queue in Recent). */
async function playAlbum(page: Page, nth: number): Promise<string> {
  await page.getByTestId("nav-albums").click();
  await page.getByTestId("grid-tile").nth(nth).dblclick();
  await expect(page.getByTestId("view-album")).toBeVisible();
  const heading = page.getByTestId("view-album").locator("h1").first();
  await expect(heading).not.toHaveText("");
  const name = (await heading.textContent())!.trim();
  await page.getByTestId("album-play").click();
  await expect.poll(async () => ((await page.evaluate(() => window.hocket.query({ type: "snapshot" }))) as { data: { queue: { contextLabel?: string } } }).data.queue.contextLabel).toBe(name);
  return name;
}

async function handoff(page: Page, to: "remote" | "self"): Promise<void> {
  await expect.poll(async () => ((await page.evaluate(() => window.hocket.query({ type: "devices" }))) as { data: DeviceInfo[] }).data.length).toBeGreaterThan(1);
  const devices = ((await page.evaluate(() => window.hocket.query({ type: "devices" }))) as { data: DeviceInfo[] }).data;
  const target = devices.find((d) => (to === "self" ? d.isSelf : !d.isSelf))!;
  await page.evaluate((id) => window.hocket.dispatch({ type: "handoffTo", data: { device_id: id } }), target.id);
}

test.describe("now playing", () => {
  test("the side panel opens on the overview and drills into lyrics, the queue and about, with a way back", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    const panel = page.getByTestId("right-panel");
    await expect(panel).toHaveAttribute("data-view", "overview");
    await expect(page.getByTestId("sidebar-empty")).toBeVisible();
    await page.evaluate(async () => {
      const s = (await window.hocket.query({ type: "servers" })) as { data: { id: string }[] };
      window.hocket.dispatch({ type: "playTracks", data: { server_id: s.data[0]!.id, track_ids: ["tr-1", "tr-2", "tr-3", "tr-4", "tr-5"], start_index: 0, label: "e2e", shuffle: false } });
    });
    // The overview: artwork, the title, then the lyrics peek, the next three songs and the song's facts.
    await expect(page.getByTestId("sidebar-overview")).toContainText("Word by Word");
    await expect(page.getByTestId("sidebar-art")).toBeVisible();
    await expect(page.getByTestId("sidebar-upnext-row")).toHaveCount(3);
    await expect(page.getByTestId("sidebar-about-card")).toContainText("Plays");
    // The peek follows playback: the current line appears once the first line starts (1.5 s).
    await expect(page.getByTestId("sidebar-lyrics-card").locator('[aria-current="true"]')).toHaveText("I lost my rank and title", { timeout: 10_000 });
    // Show lyrics → the lyrics view; back → the overview.
    await page.getByTestId("sidebar-lyrics-card-link").click();
    await expect(panel).toHaveAttribute("data-view", "lyrics");
    await expect(panel.getByTestId("lyrics-view")).toBeVisible();
    await expect(page.getByTestId("toggle-lyrics")).toHaveAttribute("aria-pressed", "true");
    // In both themes the current line is the on-surface colour and the others a muted one, each
    // at 4.5:1 on the pane (not AMLL's faint mask for dark backdrops), the current line mid-view.
    for (const theme of ["light", "dark"] as const) {
      await page.evaluate((v) => window.hocket.dispatch({ type: "setSetting", data: { key: "display.theme", value: JSON.stringify(v) } }), theme);
      await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
      const active = panel.locator('[class*="_lyricLine"][class*="_active"]:not([class*="_lyricBgLine"])').first();
      await expect(active).toBeVisible({ timeout: 10_000 });
      await page.waitForTimeout(600);
      const look = await panel.evaluate((root) => {
        // rgb(r, g, b), or color(srgb r g b) for a color-mix().
        const rgb = (c: string) => {
          const srgb = /color\(srgb ([\d.]+) ([\d.]+) ([\d.]+)/.exec(c);
          if (srgb) return srgb.slice(1, 4).map((v) => Number(v) * 255);
          return (/rgba?\(([\d.]+),\s*([\d.]+),\s*([\d.]+)/.exec(c) ?? []).slice(1, 4).map(Number);
        };
        const lum = (c: number[]) => { const l = c.map((v) => { const x = v / 255; return x <= 0.04045 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4; }); return 0.2126 * l[0]! + 0.7152 * l[1]! + 0.0722 * l[2]!; };
        const ratio = (a: string, b: string) => { const [x, y] = [lum(rgb(a)), lum(rgb(b))]; return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05); };
        const bg = getComputedStyle(root.querySelector(".side-pane")!).backgroundColor;
        const host = root.querySelector(".amll-host")!.getBoundingClientRect();
        const cur = root.querySelector('[class*="_lyricLine"][class*="_active"]:not([class*="_lyricBgLine"])')!;
        const other = Array.from(root.querySelectorAll('[class*="_lyricLine"]:not([class*="_active"]):not([class*="_lyricLineWrapper"]):not([class*="_bottomLine"])'))[0]!;
        const r = cur.getBoundingClientRect();
        const cs = getComputedStyle(cur);
        return { current: ratio(cs.color, bg), other: ratio(getComputedStyle(other).color, bg), mask: Number(cs.getPropertyValue("--bright-mask-alpha")), otherMask: Number(getComputedStyle(other).getPropertyValue("--dark-mask-alpha")), at: (r.top + r.height / 2 - host.top) / host.height, differs: cs.color !== getComputedStyle(other).color };
      });
      expect(look.current, `${theme}: current line`).toBeGreaterThanOrEqual(4.5);
      expect(look.other, `${theme}: other lines`).toBeGreaterThanOrEqual(4.5);
      expect(look.differs, `${theme}: current line stands out`).toBe(true);
      expect(look.mask).toBe(1);
      expect(look.otherMask).toBe(1);
      expect(look.at, `${theme}: current line near the middle`).toBeGreaterThan(0.3);
      expect(look.at, `${theme}: current line near the middle`).toBeLessThan(0.7);
    }
    await page.getByTestId("sidebar-back").click();
    await expect(panel).toHaveAttribute("data-view", "overview");
    // Open queue → the queue view with its selector, Autoplay and the timeline.
    await page.getByTestId("sidebar-upnext-card-link").click();
    await expect(panel).toHaveAttribute("data-view", "queue");
    await expect(panel.getByTestId("queue-selector")).toBeVisible();
    await expect(panel.getByTestId("queue-autoplay")).toBeVisible();
    await expect(panel.getByTestId("queue-row-current")).toContainText("Word by Word");
    await expect(page.getByTestId("toggle-queue")).toHaveAttribute("aria-pressed", "true");
    await page.getByTestId("sidebar-back").click();
    // More → About this song.
    await page.getByTestId("sidebar-about-card-link").click();
    await expect(panel).toHaveAttribute("data-view", "about");
    await expect(panel.getByTestId("np-about")).toContainText("Duration");
    await page.getByTestId("sidebar-back").click();
    await expect(panel).toHaveAttribute("data-view", "overview");
    // The view is remembered across a reload.
    await page.getByTestId("sidebar-upnext-card-link").click();
    await page.reload();
    await expect(page.getByTestId("right-panel")).toHaveAttribute("data-view", "queue");
  });

  test("the bar's lyrics and queue buttons open the side panel on that view and close it again; the panel button opens the overview", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playFirstAlbum(page);
    const panel = page.getByTestId("right-panel");
    // From the overview, lyrics switches the view rather than closing.
    await page.getByTestId("sidebar-back").click();
    await expect(panel).toHaveAttribute("data-view", "overview");
    await page.getByTestId("toggle-lyrics").click();
    await expect(panel).toHaveAttribute("data-view", "lyrics");
    await page.getByTestId("toggle-queue").click();
    await expect(panel).toHaveAttribute("data-view", "queue");
    // Pressed again while showing: the panel closes.
    await page.getByTestId("toggle-queue").click();
    await expect(panel).toHaveCount(0);
    await expect(page.getByTestId("toggle-queue")).toHaveAttribute("aria-pressed", "false");
    // Closed: lyrics opens it straight on the lyrics.
    await page.getByTestId("toggle-lyrics").click();
    await expect(panel).toHaveAttribute("data-view", "lyrics");
    await page.getByTestId("toggle-lyrics").click();
    await expect(panel).toHaveCount(0);
    // The side panel button opens the overview, and closes it.
    await page.getByTestId("toggle-side-panel").click();
    await expect(panel).toHaveAttribute("data-view", "overview");
    await expect(page.getByTestId("toggle-side-panel")).toHaveAttribute("aria-expanded", "true");
    await page.getByTestId("toggle-side-panel").click();
    await expect(panel).toHaveCount(0);
    // L and Q do what the buttons do.
    await page.getByTestId("content").click();
    await page.keyboard.press("l");
    await expect(panel).toHaveAttribute("data-view", "lyrics");
    await page.keyboard.press("q");
    await expect(panel).toHaveAttribute("data-view", "queue");
    await page.keyboard.press("q");
    await expect(panel).toHaveCount(0);
  });

  test("the queue selector restores a saved queue", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    const first = await playAlbum(page, 0);
    const second = await playAlbum(page, 1);
    await page.getByTestId("toggle-queue").click();
    const panel = page.getByTestId("right-panel");
    await expect(panel.getByTestId("queue-selector-label")).toHaveText(second);
    await panel.getByTestId("queue-selector").click();
    const menu = page.getByTestId("queue-selector-menu");
    await expect(menu).toBeVisible();
    // Focus moves into it; Escape closes it and gives focus back.
    await expect(menu.getByTestId("queue-selector-item").first()).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(menu).toHaveCount(0);
    await expect(panel.getByTestId("queue-selector")).toBeFocused();
    await panel.getByTestId("queue-selector").click();
    const item = menu.getByTestId("queue-selector-item").filter({ hasText: first });
    await expect(item).toBeVisible();
    // Each saved queue keeps its menu (restore, pin, save as playlist, delete).
    await menu.getByTestId("queue-selector-more").first().click();
    await expect(page.getByTestId("context-menu").locator('[data-action="pinSavedQueue"]')).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("context-menu")).toHaveCount(0);
    await item.click();
    await expect(menu).toHaveCount(0);
    await expect(panel.getByTestId("queue-selector-label")).toHaveText(first);
    await expect(page.getByTestId("player-bar").locator(".title")).not.toHaveText("");
    // "All recent queues" opens the full list, whose back arrow returns to the queue.
    await panel.getByTestId("queue-selector").click();
    await page.getByTestId("queue-selector-all").click();
    await expect(panel).toHaveAttribute("data-view", "recent");
    await expect(panel.getByTestId("saved-queues")).toBeVisible();
    await page.getByTestId("sidebar-back").click();
    await expect(panel).toHaveAttribute("data-view", "queue");
  });

  test("the Connect pill names the device only while another one plays", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playFirstAlbum(page);
    const pill = page.getByTestId("connect-button");
    await expect(pill).toBeVisible();
    await expect(page.getByTestId("connect-button-name")).toHaveCount(0);
    await expect(pill).toHaveAccessibleName("Play on…");
    await handoff(page, "remote");
    await expect(page.getByTestId("connect-button-name")).toHaveText("Pixel 8");
    await expect(pill).toHaveAttribute("data-remote", "");
    await expect(pill).toHaveAccessibleName(/Pixel 8/);
    // The fullscreen player's pill does the same.
    await page.getByTestId("toggle-fullscreen").click();
    const fs = page.getByTestId("fullscreen-player");
    await expect(fs.getByTestId("fs-connect-name")).toHaveText("Pixel 8");
    await page.keyboard.press("Escape");
    await handoff(page, "self");
    await expect(page.getByTestId("connect-button-name")).toHaveCount(0);
    await expect(pill).not.toHaveAttribute("data-remote", "");
  });

  test("turning the player's rating off hides the stars in the bar and the fullscreen player, not in About", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playFirstAlbum(page);
    const barStars = page.getByTestId("player-bar").getByRole("radiogroup");
    await expect(barStars).toHaveCount(1);
    await page.getByTestId("toggle-fullscreen").click();
    const fs = page.getByTestId("fullscreen-player");
    await expect(fs.getByTestId("np-stars")).toBeVisible();
    await page.keyboard.press("Escape");
    await page.getByTestId("nav-settings").click();
    await page.getByTestId("settings-nav-appearance").click();
    const toggle = page.getByTestId("setting-display.playerRating");
    await expect(toggle).toBeChecked();
    await toggle.click();
    await expect(toggle).not.toBeChecked();
    await expect(barStars).toHaveCount(0);
    await page.getByTestId("toggle-fullscreen").click();
    await expect(fs.getByTestId("fs-controls")).toBeVisible();
    await expect(fs.getByTestId("np-stars")).toHaveCount(0);
    // The song's details keep it.
    await fs.getByTestId("fs-mode-about").click();
    await expect(fs.getByTestId("np-about").getByRole("radiogroup")).toHaveCount(1);
  });

  test("the fullscreen player opens on the queue the first time, then on the view it was left on", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playFirstAlbum(page);
    await page.getByTestId("toggle-fullscreen").click();
    const fs = page.getByTestId("fullscreen-player");
    await expect(fs).toHaveAttribute("data-mode", "queue");
    await expect(fs.getByTestId("fs-mode-queue")).toHaveAttribute("aria-pressed", "true");
    await expect(fs.getByTestId("np-pane-queue")).toBeVisible();
    // Clicking the view that is on goes back to the artwork alone.
    await fs.getByTestId("fs-mode-queue").click();
    await expect(fs).toHaveAttribute("data-mode", "art");
    await expect(fs.getByTestId("np-artwork")).toBeVisible();
    await fs.getByTestId("fs-mode-lyrics").click();
    await expect(fs.getByTestId("np-pane-lyrics")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(fs).toHaveCount(0);
    await page.getByTestId("toggle-fullscreen").click();
    await expect(fs).toHaveAttribute("data-mode", "lyrics");
    // And after a restart of the window.
    await page.keyboard.press("Escape");
    await page.reload();
    await expect(page.getByTestId("app")).toBeVisible();
    await page.getByTestId("toggle-fullscreen").click();
    await expect(page.getByTestId("fullscreen-player")).toHaveAttribute("data-mode", "lyrics");
  });
});

// Immersive artwork needs the classifier, which the fake core borrows from the native addon.
const addonBuilt = existsSync(join(resolve(__dirname, ".."), "native", "index.js"));

test.describe("fullscreen idle fade over the immersive artwork", () => {
  test.skip(!addonBuilt, "native addon not built (run pnpm gen): no immersive artwork without its classifier");
  test.setTimeout(90_000);

  test("while playing, the controls fade when the mouse rests and come back on movement or a key; never while paused; a setting turns it off", async () => {
    // Plain covers: the classifier carries them on (and "always" makes sure).
    const covers = mkdtempSync(join(tmpdir(), "hocket-covers-"));
    const userData = mkdtempSync(join(tmpdir(), "hocket-e2e-"));
    mkdirSync(covers, { recursive: true });
    for (let i = 0; i < 4; i++) {
      const rgba = new Uint8Array(256 * 256 * 4);
      for (let p = 0; p < rgba.length; p += 4) rgba.set([30 + i * 40, 70, 90, 255], p);
      writeFileSync(join(covers, `${i}.png`), encodePng(256, 256, rgba));
    }
    const { app, page } = await launchFake(userData, { HOCKET_FAKE_COVERS: covers });
    try {
      await completeSetup(page);
      await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]?.setContentSize(1456, 912));
      await page.evaluate(() => window.hocket.dispatch({ type: "setSetting", data: { key: "display.immersiveArtwork", value: JSON.stringify("always") } }));
      await page.evaluate(async () => {
        const s = (await window.hocket.query({ type: "servers" })) as { data: { id: string }[] };
        window.hocket.dispatch({ type: "playContext", data: { args: { context: { serverId: s.data[0]!.id, kind: { type: "album", data: { id: "al-1" } }, label: "e2e", sort: "default" }, shuffle: false, saveOutgoing: false } } });
      });
      await expect(page.getByTestId("play-pause")).toHaveAttribute("aria-label", "Pause");
      await page.getByTestId("toggle-fullscreen").click();
      const fs = page.getByTestId("fullscreen-player");
      await fs.getByTestId("fs-mode-queue").click();
      await expect(fs).toHaveAttribute("data-mode", "art");
      await expect(fs).toHaveAttribute("data-layout", "immersive");
      await expect(fs.getByTestId("np-continuation")).toBeVisible();
      // The mouse rests: the card, the pill and the header fade.
      await page.mouse.move(400, 300);
      await expect(fs).toHaveAttribute("data-idle", "", { timeout: 8000 });
      await expect.poll(() => fs.getByTestId("fs-pill").evaluate((el) => getComputedStyle(el).opacity), { timeout: 3000 }).toBe("0");
      // Moving the mouse brings them back; so does a key.
      await page.mouse.move(420, 320);
      await expect(fs).not.toHaveAttribute("data-idle", "");
      await expect(fs).toHaveAttribute("data-idle", "", { timeout: 8000 });
      await page.keyboard.press("Shift");
      await expect(fs).not.toHaveAttribute("data-idle", "");
      // A view keeps them up.
      await fs.getByTestId("fs-mode-lyrics").click();
      await page.waitForTimeout(4000);
      await expect(fs).not.toHaveAttribute("data-idle", "");
      await fs.getByTestId("fs-mode-lyrics").click();
      // Paused: they stay.
      await fs.getByTestId("fs-play-pause").click();
      await expect(fs.getByTestId("fs-play-pause")).toHaveAttribute("aria-label", "Play");
      await page.mouse.move(400, 300);
      await page.waitForTimeout(4500);
      await expect(fs).not.toHaveAttribute("data-idle", "");
      await fs.getByTestId("fs-play-pause").click();
      await expect(fs).toHaveAttribute("data-idle", "", { timeout: 8000 });
      // The setting turns it off.
      await page.keyboard.press("Escape");
      await page.getByTestId("nav-settings").click();
      await page.getByTestId("settings-nav-appearance").click();
      const toggle = page.getByTestId("setting-fullscreen-idle-hide");
      await expect(toggle).toBeChecked();
      await toggle.click();
      await expect(toggle).not.toBeChecked();
      await page.getByTestId("toggle-fullscreen").click();
      await expect(fs).toHaveAttribute("data-mode", "art");
      await page.mouse.move(400, 300);
      await page.waitForTimeout(4500);
      await expect(fs).not.toHaveAttribute("data-idle", "");
    } finally {
      await app.close().catch(() => undefined);
      rmSync(userData, { recursive: true, force: true });
      rmSync(covers, { recursive: true, force: true });
    }
  });
});
