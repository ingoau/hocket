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
import { completeSetup, expect, launchFake, playFirstAlbum, resizeWindow, test, openPlace } from "./fixtures";
import { readPng } from "./png-read";

/** Plays the `nth` album from its page, as a user does (which keeps the outgoing queue in Recent). */
async function playAlbum(page: Page, nth: number): Promise<string> {
  await openPlace(page, "albums");
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
  test("the volume control drives the device playing, each with its own level and mute", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playFirstAlbum(page);
    const volume = page.getByTestId("volume");
    const mute = page.getByTestId("mute");
    await expect(volume).toHaveAttribute("aria-label", "Volume");
    const own = Number(await volume.inputValue());
    // Another device plays: the slider is that device's, under its name.
    await handoff(page, "remote");
    await expect(volume).toHaveAttribute("aria-label", /^Volume – .+/);
    await volume.fill("0.3");
    await expect.poll(async () => Number(await volume.inputValue())).toBe(0.3);
    // Mute keeps the remote device's level for the unmute.
    await mute.click();
    await expect(mute).toHaveAttribute("aria-pressed", "true");
    await expect(volume).toHaveAttribute("aria-valuetext", "Muted");
    await mute.click();
    await expect(mute).toHaveAttribute("aria-pressed", "false");
    await expect.poll(async () => Number(await volume.inputValue())).toBe(0.3);
    // Back here: this device's own level, untouched.
    await handoff(page, "self");
    await expect(volume).toHaveAttribute("aria-label", "Volume");
    await expect.poll(async () => Number(await volume.inputValue())).toBe(own);
  });

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
    // L and Q do what the buttons do (with focus off the track table, whose letters type ahead).
    await page.getByTestId("album-head").click();
    await page.keyboard.press("l");
    await expect(panel).toHaveAttribute("data-view", "lyrics");
    await page.keyboard.press("q");
    await expect(panel).toHaveAttribute("data-view", "queue");
    await page.keyboard.press("q");
    await expect(panel).toHaveCount(0);
  });

  test("the queue's name opens the Queues page: rename, pin, delete and restore", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    const first = await playAlbum(page, 0);
    const second = await playAlbum(page, 1);
    await page.getByTestId("toggle-queue").click();
    const panel = page.getByTestId("right-panel");
    await expect(panel.getByTestId("queue-selector-label")).toHaveText(second);
    await panel.getByTestId("queue-selector").click();
    await expect(panel).toHaveAttribute("data-view", "recent");
    const pageEl = panel.getByTestId("saved-queues");
    await expect(pageEl.getByTestId("saved-queues-current")).toContainText(second);
    // Renaming the playing queue: Enter saves, and the queue's name follows.
    await pageEl.getByTestId("saved-queues-current-rename").click();
    await page.getByTestId("saved-queue-rename-input").fill("Late night");
    await page.keyboard.press("Enter");
    await expect(pageEl.getByTestId("saved-queues-current")).toContainText("Late night");
    // A saved queue: Escape cancels a rename, Enter saves one.
    const row = pageEl.getByTestId("saved-queue-row").filter({ hasText: first });
    await row.getByTestId("saved-queue-rename").click();
    await page.getByTestId("saved-queue-rename-input").fill("Nope");
    await page.keyboard.press("Escape");
    await expect(row).toContainText(first);
    await row.getByTestId("saved-queue-rename").click();
    await page.getByTestId("saved-queue-rename-input").fill("Morning");
    await page.keyboard.press("Enter");
    const morning = pageEl.getByTestId("saved-queue-row").filter({ hasText: "Morning" });
    await expect(morning).toBeVisible();
    // Pinning moves it under Pinned; its context menu offers Unpin.
    await morning.getByTestId("saved-queue-pin").click();
    await expect(morning.getByTestId("saved-queue-pin")).toHaveAttribute("aria-pressed", "true");
    await morning.click({ button: "right" });
    await expect(page.getByTestId("context-menu").locator('[data-action="unpinSavedQueue"]')).toBeVisible();
    await page.keyboard.press("Escape");
    // Restoring returns to the queue, now playing that one.
    await morning.getByTestId("saved-queue-restore").click();
    await expect(panel).toHaveAttribute("data-view", "queue");
    await expect(panel.getByTestId("queue-selector-label")).toHaveText("Morning");
    await expect(page.getByTestId("player-bar").locator(".title")).not.toHaveText("");
    // Delete is immediate.
    await panel.getByTestId("queue-selector").click();
    const late = pageEl.getByTestId("saved-queue-row").filter({ hasText: "Late night" });
    await expect(late).toBeVisible();
    await late.getByTestId("saved-queue-delete").click();
    await expect(late).toHaveCount(0);
    await page.getByTestId("sidebar-back").click();
    await expect(panel).toHaveAttribute("data-view", "queue");
  });

  test("lyrics options live in a popover from the lyrics view's header, and from a right-click on the lyrics toggles", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await page.evaluate(async () => {
      const s = (await window.hocket.query({ type: "servers" })) as { data: { id: string }[] };
      window.hocket.dispatch({ type: "playTracks", data: { server_id: s.data[0]!.id, track_ids: ["tr-1"], start_index: 0, label: "e2e", shuffle: false } });
    });
    await page.getByTestId("toggle-lyrics").click();
    const panel = page.getByTestId("right-panel");
    await expect(panel.getByTestId("lyrics-view")).toBeVisible();
    // Nothing is laid over the lyrics; the header's button opens the options, by keyboard too.
    await expect(panel.getByTestId("lyrics-view").getByTestId("lyrics-tools")).toHaveCount(0);
    const button = panel.getByTestId("lyrics-options");
    await button.focus();
    await page.keyboard.press("Enter");
    const options = page.getByTestId("lyrics-tools");
    await expect(options).toBeVisible();
    await expect(button).toHaveAttribute("aria-expanded", "true");
    await expect(options).toContainText("Word-synced");
    await expect(page.getByTestId("lyrics-offset-earlier")).toBeFocused();
    await page.keyboard.press("Enter");
    await expect(page.getByTestId("lyrics-offset-value")).toHaveText("-0.1s");
    await page.getByTestId("lyrics-offset-reset").click();
    await expect(page.getByTestId("lyrics-offset-value")).toHaveText("0.0s");
    await expect(page.getByTestId("lyrics-offset-reset")).toHaveCount(0);
    await page.getByTestId("lyrics-size-large").click();
    await expect(panel.getByTestId("amll-host")).toHaveAttribute("data-size", "large");
    await page.keyboard.press("Escape");
    await expect(options).toHaveCount(0);
    await expect(button).toBeFocused();
    // The bar's lyrics toggle: a right-click opens the options above it (a click still toggles the
    // lyrics), and so do Shift+F10 and the context-menu key; it says so.
    const toggle = page.getByTestId("toggle-lyrics");
    await expect(toggle).not.toHaveAttribute("aria-haspopup", /./);
    await expect(toggle).toHaveAccessibleDescription("Right-click for lyrics options");
    await toggle.click({ button: "right" });
    await expect(options).toBeVisible();
    await expect(panel).toHaveAttribute("data-view", "lyrics");
    const [barPop, barToggle] = await Promise.all([options.boundingBox(), toggle.boundingBox()]);
    expect(barPop!.y + barPop!.height).toBeLessThanOrEqual(barToggle!.y);
    await page.keyboard.press("Escape");
    await expect(options).toHaveCount(0);
    await expect(toggle).toBeFocused();
    await page.keyboard.press("Shift+F10");
    await expect(options).toBeVisible();
    await expect(page.getByTestId("lyrics-offset-earlier")).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(toggle).toBeFocused();
    await page.keyboard.press("ContextMenu");
    await expect(options).toBeVisible();
    await page.keyboard.press("Escape");
    // Fullscreen: nothing is added to the pill for the lyrics, so its buttons stay put between views.
    await page.getByTestId("toggle-fullscreen").click();
    const fs = page.getByTestId("fullscreen-player");
    await expect(fs).toHaveAttribute("data-mode", "queue");
    await expect(fs.getByTestId("fs-pill").getByTestId("lyrics-options")).toHaveCount(0);
    // (Once the player's opening motion has settled.)
    const settled = () => fs.evaluate((el) => Promise.race([Promise.all(el.getAnimations({ subtree: true }).filter((a) => a.effect?.getTiming().iterations !== Number.POSITIVE_INFINITY).map((a) => a.finished.catch(() => undefined))), new Promise((r) => setTimeout(r, 3000))]));
    const positions = () => fs.getByTestId("fs-pill").locator("button").evaluateAll((els) => els.map((el) => { const r = el.getBoundingClientRect(); return `${el.getAttribute("data-testid")}@${Math.round(r.x)},${Math.round(r.y)}`; }));
    await settled();
    const before = await positions();
    await fs.getByTestId("fs-mode-lyrics").click();
    await expect(fs).toHaveAttribute("data-mode", "lyrics");
    await settled();
    expect(await positions()).toEqual(before);
    await expect(fs.getByTestId("fs-pill").getByTestId("lyrics-options")).toHaveCount(0);
    // A right-click on the pill's lyrics toggle opens the options, rising above the pill.
    const fsToggle = fs.getByTestId("fs-mode-lyrics");
    await fsToggle.click({ button: "right" });
    await expect(fs).toHaveAttribute("data-mode", "lyrics");
    await expect(fs.getByTestId("lyrics-tools")).toBeVisible();
    const [pop, pill] = await Promise.all([fs.getByTestId("lyrics-tools").boundingBox(), fs.getByTestId("fs-pill").boundingBox()]);
    expect(pop!.y + pop!.height).toBeLessThanOrEqual(pill!.y);
    // Escape closes the options, not the player.
    await page.keyboard.press("Escape");
    await expect(fs.getByTestId("lyrics-tools")).toHaveCount(0);
    await expect(fs).toBeVisible();
    await expect(fsToggle).toBeFocused();
    // Shift+F10 on the focused toggle too, with the lyrics view closed.
    await fs.getByTestId("fs-mode-queue").click();
    await fsToggle.focus();
    await page.keyboard.press("Shift+F10");
    await expect(fs.getByTestId("lyrics-tools")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(fs.getByTestId("lyrics-tools")).toHaveCount(0);
    await expect(fs).toBeVisible();
  });

  test("another device's resume offer is not shown; the fullscreen player has no 'Playing from' header", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    // The fake core offers to resume what "Pixel 8" was playing once it appears.
    await expect.poll(async () => ((await page.evaluate(() => window.hocket.query({ type: "snapshot" }))) as { data: { resumeOffer?: unknown } }).data.resumeOffer !== undefined, { timeout: 10_000 }).toBe(true);
    await expect(page.getByTestId("resume-offer")).toHaveCount(0);
    await expect(page.getByTestId("player-bar")).not.toContainText("was playing");
    await page.getByTestId("toggle-fullscreen").click();
    const fs = page.getByTestId("fullscreen-player");
    await expect(fs).toBeVisible();
    await expect(fs.getByText("Playing from")).toHaveCount(0);
    await expect(fs.getByTestId("fs-window-fullscreen")).toBeVisible();
    await expect(fs.getByTestId("fullscreen-exit")).toBeVisible();
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

test.describe("the artist in the now playing subtitle", () => {
  test("a song with several artists shows the artist string as given and lists every artist, each opening its page", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await page.evaluate(async () => {
      const s = (await window.hocket.query({ type: "servers" })) as { data: { id: string }[] };
      window.hocket.dispatch({ type: "playTracks", data: { server_id: s.data[0]!.id, track_ids: ["tr-1"], start_index: 0, label: "e2e", shuffle: false } });
    });
    await expect.poll(async () => ((await page.evaluate(() => window.hocket.query({ type: "snapshot" }))) as { data: { queue: { current?: { track: { id: string } } } } }).data.queue.current?.track.id).toBe("tr-1");
    const track = ((await page.evaluate(() => window.hocket.query({ type: "snapshot" }))) as { data: { queue: { current?: { track: { artist: string; artists?: { id?: string; name: string }[] } } } } }).data.queue.current!.track;
    expect(track.artists?.length).toBe(2);
    await expect(page.getByTestId("bar-artist")).toHaveText(track.artist);
    expect(track.artist).toContain(" feat. ");
    await page.getByTestId("bar-artist").click();
    const menu = page.getByTestId("artist-menu");
    await expect(menu.getByTestId("artist-menu-artist")).toHaveText(track.artists!.map((a) => a.name));
    // The guest opens the guest's page.
    await menu.getByTestId("artist-menu-artist").nth(1).click();
    await expect(page.getByTestId("view-artist").locator("h1").first()).toHaveText(track.artists![1]!.name);
  });

  test("shows the artist alone and opens a menu of the track's artists and its album, from the bar, the side panel and the fullscreen player", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playFirstAlbum(page);
    const track = ((await page.evaluate(() => window.hocket.query({ type: "snapshot" }))) as { data: { queue: { current?: { track: { artist?: string; album?: string } } } } }).data.queue.current!.track;
    const menu = page.getByTestId("artist-menu");
    // The bar: the artist string as given, no album.
    const bar = page.getByTestId("bar-artist");
    await expect(page.getByTestId("player-bar").locator(".now .artist")).toHaveText(track.artist!);
    await bar.click();
    await expect(menu).toBeVisible();
    await expect(menu.getByTestId("artist-menu-artist")).toHaveText([track.artist!]);
    await expect(menu.getByTestId("artist-menu-album")).toContainText(track.album!);
    // Escape closes it, back on the artist.
    await page.keyboard.press("Escape");
    await expect(menu).toHaveCount(0);
    await expect(bar).toBeFocused();
    // From the keyboard: Enter opens it on the first artist, the arrows move, Enter opens the album.
    await page.keyboard.press("Enter");
    await expect(menu.getByTestId("artist-menu-artist").first()).toBeFocused();
    await page.keyboard.press("ArrowDown");
    await expect(menu.getByTestId("artist-menu-album")).toBeFocused();
    await page.keyboard.press("Enter");
    await expect(menu).toHaveCount(0);
    await expect(page.getByTestId("view-album").locator("h1").first()).toHaveText(track.album!);
    // The side panel's overview: an artist opens the artist's page.
    const panel = page.getByTestId("right-panel");
    if ((await panel.getAttribute("data-view")) !== "overview") await page.getByTestId("sidebar-back").click();
    await expect(panel).toHaveAttribute("data-view", "overview");
    await expect(page.getByTestId("sidebar-artist")).toHaveText(track.artist!);
    await page.getByTestId("sidebar-artist").click();
    await menu.getByTestId("artist-menu-artist").first().click();
    await expect(page.getByTestId("view-artist")).toBeVisible();
    // The fullscreen card: the artist alone; an artist leaves the player for the artist's page.
    await openPlace(page, "albums");
    await page.getByTestId("toggle-fullscreen").click();
    const fs = page.getByTestId("fullscreen-player");
    await expect(fs.locator(".np-text .t2")).toHaveText(track.artist!);
    await fs.getByTestId("fs-artist").click();
    await expect(fs.getByTestId("artist-menu")).toBeVisible();
    await expect(fs.getByTestId("artist-menu-album")).toContainText(track.album!);
    // Escape closes the menu, not the player.
    await page.keyboard.press("Escape");
    await expect(menu).toHaveCount(0);
    await expect(fs).toBeVisible();
    await fs.getByTestId("fs-artist").click();
    await menu.getByTestId("artist-menu-artist").first().click();
    await expect(fs).toHaveCount(0);
    await expect(page.getByTestId("view-artist")).toBeVisible();
  });
});

test.describe("the side panel overview from the keyboard", () => {
  test("Enter on an Up next row plays it; the About card's link has its own name", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playFirstAlbum(page);
    await page.getByTestId("toggle-side-panel").click();
    if (!(await page.getByTestId("right-panel").count())) await page.getByTestId("toggle-side-panel").click();
    const panel = page.getByTestId("right-panel");
    if ((await panel.getAttribute("data-view")) !== "overview") await page.getByTestId("sidebar-back").click();
    await expect(panel).toHaveAttribute("data-view", "overview");
    await expect(page.getByTestId("sidebar-about-card-link")).toHaveAccessibleName("More about this song");
    const row = page.getByTestId("sidebar-upnext-row").first();
    const title = (await row.locator(".t1").textContent())!;
    // A single click leaves it alone; Enter plays it.
    await row.click();
    await page.waitForTimeout(300);
    expect(await page.getByTestId("player-bar").locator(".now .title").textContent()).not.toBe(title);
    await row.focus();
    await page.keyboard.press("Enter");
    await expect(page.getByTestId("player-bar").locator(".now .title")).toHaveText(title);
  });
});

test.describe("the player bar's wave", () => {
  test("moves while the window has focus, and holds still under the battery saver", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playFirstAlbum(page);
    const wave = page.getByTestId("seek-slider");
    await expect(wave).toHaveAttribute("data-wave", "moving");
    await page.evaluate(() => window.hocket.dispatch({ type: "setBatterySaver", data: { enabled: true } }));
    await expect(wave).toHaveAttribute("data-wave", "still");
    await page.evaluate(() => window.hocket.dispatch({ type: "setBatterySaver", data: { enabled: false } }));
    await expect(wave).toHaveAttribute("data-wave", "moving");
  });
});

test.describe("the player bar at middling widths", () => {
  test("the song, the transport and the buttons never overlap, with another device's name showing too", async ({ hocket }) => {
    const { app, page } = hocket;
    await completeSetup(page);
    await playFirstAlbum(page);
    /** The three blocks' painted extents: what each one's children cover, not its grid cell. */
    const blocks = () => page.getByTestId("player-bar").evaluate((bar) => {
      const span = (els: Element[]) => {
        const rs = els.map((e) => e.getBoundingClientRect()).filter((r) => r.width > 0);
        return { left: Math.min(...rs.map((r) => r.left)), right: Math.max(...rs.map((r) => r.right)) };
      };
      const now = bar.querySelector(".now")!;
      return {
        now: span(Array.from(now.querySelectorAll(".art, .text > *, .now-actions"))),
        transport: span(Array.from(bar.querySelectorAll(".transport > *"))),
        right: span(Array.from(bar.querySelector(".right")!.children)),
        width: bar.getBoundingClientRect().width,
      };
    });
    for (const remote of [false, true]) {
      if (remote) {
        await handoff(page, "remote");
        await expect(page.getByTestId("connect-button-name")).toBeVisible();
      }
      for (const w of [960, 1100]) {
        await resizeWindow(app, page, w, 800);
        await page.waitForTimeout(200);
        const b = await blocks();
        const label = `${w} px${remote ? ", remote" : ""}: ${JSON.stringify(b)}`;
        expect(b.now.right, label).toBeLessThanOrEqual(b.transport.left);
        expect(b.transport.right, label).toBeLessThanOrEqual(b.right.left);
        expect(b.right.right, label).toBeLessThanOrEqual(b.width);
      }
    }
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
      await resizeWindow(app, page, 1456, 912);
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
      // An open popover of the player's keeps them up (the artist menu, the lyrics options), with
      // the mouse resting; they fade once it closes.
      await fs.getByTestId("fs-artist").click();
      await expect(fs.getByTestId("artist-menu")).toBeVisible();
      await page.waitForTimeout(4500);
      await expect(fs).not.toHaveAttribute("data-idle", "");
      await page.keyboard.press("Escape");
      await expect(fs.getByTestId("artist-menu")).toHaveCount(0);
      await fs.getByTestId("fs-mode-lyrics").click({ button: "right" });
      await expect(fs.getByTestId("lyrics-tools")).toBeVisible();
      await page.waitForTimeout(4500);
      await expect(fs).not.toHaveAttribute("data-idle", "");
      await page.keyboard.press("Escape");
      await expect(fs.getByTestId("lyrics-tools")).toHaveCount(0);
      await expect(fs).toHaveAttribute("data-idle", "", { timeout: 8000 });
      await page.mouse.move(420, 330);
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

/** Generated covers for the classifier, `256`² RGBA: `paint(x, y)` gives each pixel. */
function writeCover(dir: string, name: string, paint: (x: number, y: number) => [number, number, number]): void {
  const rgba = new Uint8Array(256 * 256 * 4);
  for (let y = 0; y < 256; y++) for (let x = 0; x < 256; x++) rgba.set([...paint(x, y), 255], (y * 256 + x) * 4);
  writeFileSync(join(dir, `${name}.png`), encodePng(256, 256, rgba));
}
/** Deterministic high-contrast blocks: a busy cover, nothing to carry on. */
function busy(x: number, y: number): [number, number, number] {
  let h = (Math.floor(x / 8) * 7919 + Math.floor(y / 8) * 104_729) >>> 0;
  h = Math.imul(h ^ (h >>> 13), 0x5bd1e995) >>> 0;
  return [h & 255, (h >>> 8) & 255, (h >>> 16) & 255];
}

/** Plays album `al-<n>` (which wears cover n - 1) from the fake library. */
async function playCover(page: Page, n: number): Promise<void> {
  await page.evaluate(async (id) => {
    const s = (await window.hocket.query({ type: "servers" })) as { data: { id: string }[] };
    window.hocket.dispatch({ type: "playContext", data: { args: { context: { serverId: s.data[0]!.id, kind: { type: "album", data: { id } }, label: "e2e", sort: "default" }, shuffle: false, saveOutgoing: false } } });
  }, `al-${n}`);
  await expect.poll(async () => ((await page.evaluate(() => window.hocket.query({ type: "snapshot" }))) as { data: { queue: { current?: { track: { albumId?: string } } } } }).data.queue.current?.track.albumId).toBe(`al-${n}`);
}

/**
 * The lowest contrast of the text over the continuation (the title row and the Lyrics card) against
 * what is behind it: sampled across each text element, the backdrop as the stylesheet builds it
 * (the canvas's pixels settling into the base colour, the whole fading back into the pane behind
 * the Lyrics card), the card's fill on top, and the text's colour over that.
 */
async function overviewContrast(page: Page): Promise<{ min: number; worst: string }> {
  return page.getByTestId("side-overview").evaluate((root) => {
    type C = [number, number, number, number];
    const parse = (s: string): C => {
      const m = /rgba?\(([^)]+)\)/.exec(s);
      if (m) {
        const [r, g, b, a] = m[1]!.split(/[\s,/]+/).filter(Boolean).map(Number);
        return [r!, g!, b!, a ?? 1];
      }
      const h = /#([0-9a-f]{6})/i.exec(s)!;
      return [parseInt(h[1]!.slice(0, 2), 16), parseInt(h[1]!.slice(2, 4), 16), parseInt(h[1]!.slice(4, 6), 16), 1];
    };
    const over = (top: C, under: C): C => [0, 1, 2].map((i) => top[i]! * top[3] + under[i]! * (1 - top[3])).concat(1) as C;
    const lum = (c: C) => {
      const ch = (v: number) => ((v /= 255) <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
      return 0.2126 * ch(c[0]) + 0.7152 * ch(c[1]) + 0.0722 * ch(c[2]);
    };
    const ratio = (a: C, b: C) => (Math.max(lum(a), lum(b)) + 0.05) / (Math.min(lum(a), lum(b)) + 0.05);
    const style = getComputedStyle(root);
    const base = parse(`rgb(${(root.closest('[data-testid="sidebar-scroll"]') as HTMLElement).style.getPropertyValue("--side-fill")})`);
    const at = Number.parseFloat(style.getPropertyValue("--side-fade-at"));
    const len = Number.parseFloat(style.getPropertyValue("--side-fade-len"));
    const top0 = root.getBoundingClientRect().top;
    /** A reflection's canvas fades into the overview's colour; an extension's does not. */
    const mirror = root.getAttribute("data-style") === "mirror";
    const ss = (t: number) => {
      const u = Math.max(0, Math.min(1, t));
      return u * u * (3 - 2 * u);
    };
    const canvas = root.querySelector<HTMLCanvasElement>("canvas.side-continuation:last-of-type")!;
    const ctx = canvas.getContext("2d")!;
    const box = canvas.getBoundingClientRect();
    const sx = canvas.width / box.width;
    let min = Infinity;
    let worst = "";
    const texts = root.querySelectorAll<HTMLElement>(":scope > .side-title-row :is(.side-title, .side-artist, .side-icon-btn), :scope > .side-card :is(.side-card-title, .side-card-link, .peek-line, .upnext-row .t1, .upnext-row .t2, .upnext-row .dur, .side-fact dt, .side-fact dd)");
    for (const el of Array.from(texts)) {
      const r = el.getBoundingClientRect();
      if (!r.width || !r.height) continue;
      const fg = parse(getComputedStyle(el).color);
      const card = el.closest(".side-card");
      const fill = card ? parse(getComputedStyle(card).backgroundColor) : undefined;
      for (let i = 1; i <= 5; i++) {
        for (let j = 1; j <= 3; j++) {
          const x = r.left + (r.width * i) / 6;
          const y = r.top + (r.height * j) / 4;
          const local = y - top0;
          let back = base;
          if (y < box.bottom) {
            const d = ctx.getImageData(Math.floor((x - box.left) * sx), Math.floor((y - box.top) * sx), 1, 1).data;
            back = over([d[0]!, d[1]!, d[2]!, (d[3]! / 255) * (mirror ? 1 - ss((local - at) / len) : 1)], base);
          }
          let bg = back;
          if (fill) bg = over(fill, bg);
          const c = ratio(over(fg, bg), bg);
          if (c < min) {
            min = c;
            worst = `${el.className || el.tagName}: ${c.toFixed(2)}`;
          }
        }
      }
    }
    return { min, worst };
  });
}

/** Skips to a track of the current context with (or without) lyrics. */
async function toTrackWithLyrics(page: Page, want: boolean): Promise<void> {
  for (let i = 0; i < 12; i++) {
    const has = await page.evaluate(async () => {
      const snap = (await window.hocket.query({ type: "snapshot" })) as { data: { queue: { current?: { track: { id: string } } } } };
      const id = snap.data.queue.current?.track.id;
      const r = (await window.hocket.query({ type: "lyrics", data: { track_id: id! } })) as { data?: { tier?: string } | null };
      return !!r.data?.tier;
    });
    if (has === want) return;
    await page.evaluate(() => window.hocket.dispatch({ type: "next" }));
    await page.waitForTimeout(300);
  }
  throw new Error(`no track ${want ? "with" : "without"} lyrics`);
}

/**
 * A reflection's fade into the overview's colour, measured on screen: a column in the overview's
 * margin (outside the cards) from the top of the fade to below the top of Up next, and the middle
 * of the gap above Up next, with no step above 6 levels per pixel; past the fade, down to the
 * pane's bottom, the overview's colour itself.
 */
async function checkFade(page: Page, label: string): Promise<void> {
  const overview = page.getByTestId("side-overview");
  const geo = await overview.evaluate((el) => {
    const pane = el.closest(".side-pane")!.getBoundingClientRect();
    const upNext = el.querySelector('[data-card="upnext"]')!.getBoundingClientRect();
    const top = el.getBoundingClientRect().top;
    const s = getComputedStyle(el);
    const fill = (el.closest('[data-testid="sidebar-scroll"]') as HTMLElement).style.getPropertyValue("--side-fill").split(",").map(Number);
    const at = Number.parseFloat(s.getPropertyValue("--side-fade-at"));
    return { pane: { x: pane.x, y: pane.y, w: pane.width, h: pane.height }, upTop: upNext.top, fadeAt: top + at, fadeEnd: top + at + Number.parseFloat(s.getPropertyValue("--side-fade-len")), fill };
  });
  const shot = readPng(await page.screenshot({ clip: { x: geo.pane.x, y: geo.pane.y, width: geo.pane.w, height: geo.pane.h } }));
  const px = (x: number, y: number) => {
    const i = (Math.round(y - geo.pane.y) * shot.width + Math.round(x - geo.pane.x)) * 4;
    return [shot.data[i]!, shot.data[i + 1]!, shot.data[i + 2]!];
  };
  const level = (c: number[]) => (c[0]! + c[1]! + c[2]!) / 3;
  const bottom = geo.pane.y + geo.pane.h - 32; // clear of the pane's rounded corners
  const run = (x: number, from: number, to: number, what: string) => {
    let prev: number | undefined;
    for (let y = Math.ceil(from); y < Math.min(to, bottom); y++) {
      const v = level(px(x, y));
      if (prev !== undefined) expect(Math.abs(v - prev), `${label}, ${what}: step of ${(v - prev).toFixed(1)} at ${Math.round(y - from)} px`).toBeLessThanOrEqual(6);
      prev = v;
    }
  };
  const margin = geo.pane.x + 6;
  run(margin, geo.fadeAt - 8, Math.max(geo.upTop, geo.fadeEnd) + 24, "margin");
  // Spread over the whole fade, one way: no stretch of 12 px carries more than 30% of the change
  // (a fade squeezed into a gap reads as a band), and it never turns back.
  const v: number[] = [];
  for (let y = Math.ceil(geo.fadeAt); y <= Math.floor(geo.fadeEnd) && y < bottom; y++) v.push(level(px(margin, y)));
  const total = v.at(-1)! - v[0]!;
  if (Math.abs(total) >= 6) {
    for (let i = 12; i < v.length; i++) expect(Math.abs(v[i]! - v[i - 12]!), `${label}: ${(v[i]! - v[i - 12]!).toFixed(1)} of ${total.toFixed(1)} levels within 12 px at ${i} px`).toBeLessThanOrEqual(0.3 * Math.abs(total) + 2);
    for (let i = 1; i < v.length; i++) expect((v[i]! - v[i - 1]!) * Math.sign(total), `${label}: the fade turns back at ${i} px`).toBeGreaterThanOrEqual(-1.5);
  }
  run(geo.pane.x + geo.pane.w / 2, geo.upTop - 11, geo.upTop - 1, "gap above Up next");
  // Past the fade, the overview's colour to the bottom of the pane.
  for (let y = Math.ceil(geo.fadeEnd) + 2; y < bottom; y += 6) {
    const c = px(margin, y);
    for (let k = 0; k < 3; k++) expect(Math.abs(c[k]! - geo.fill[k]!), `${label}: the overview's colour ${Math.round(y - geo.fadeEnd)} px past the fade`).toBeLessThanOrEqual(3);
  }
}

test.describe("immersive artwork from the classifier", () => {
  test.skip(!addonBuilt, "native addon not built (run pnpm gen): no immersive artwork without its classifier");
  test.setTimeout(120_000);
  let covers: string;
  test.beforeAll(() => {
    covers = mkdtempSync(join(tmpdir(), "hocket-covers-"));
    // al-1: one dark colour, carried on every way. al-2: busy, with a plain band down the right
    // only. al-3: busy all over. al-4: one light colour.
    writeCover(covers, "0-both", () => [24, 58, 72]);
    writeCover(covers, "1-right", (x, y) => (x >= 196 ? [40, 70, 130] : busy(x, y)));
    writeCover(covers, "2-card", busy);
    writeCover(covers, "3-light", () => [236, 228, 210]);
    // al-6: white with a block of coloured pixel squares, a plain white bottom (a white cover
    // carried on in white, in the dark theme too). al-7: columns of colour, carried on column by column.
    writeCover(covers, "5-white", (x, y) => (x >= 56 && x < 200 && y >= 48 && y < 176 && ((Math.floor(x / 16) + Math.floor(y / 16)) % 3 !== 0) ? busy(x, y) : [250, 250, 250]));
    // (A caption just above the edge rules a reflection out.)
    const glyph = (x: number, y: number) => {
      if (y < 226 || y >= 240 || x < 40 || x >= 180) return false;
      const gx = 40 + Math.floor((x - 40) / 14) * 14;
      return x < gx + 3 || (x < gx + 9 && y < 229) || (x >= gx + 6 && x < gx + 9 && y >= 232);
    };
    writeCover(covers, "6-columns", (x, y) => (glyph(x, y) ? [20, 20, 24] : [Math.round(40 + x * 0.6), 90, Math.round(200 - x * 0.5)]));
    // al-8, al-9: soft textures, dark and light, mirrored too.
    const soft = (lo: number, tint: [number, number, number]) => (x: number, y: number): [number, number, number] => {
      const v = (Math.sin(x / 9) * Math.cos(y / 11) * 0.5 + 0.5) * 80;
      return [Math.min(255, Math.round(lo + v * tint[0])), Math.min(255, Math.round(lo + v * tint[1])), Math.min(255, Math.round(lo + v * tint[2]))];
    };
    writeCover(covers, "7-darksoft", soft(28, [0.5, 0.7, 1]));
    writeCover(covers, "8-lightsoft", soft(170, [1, 0.85, 0.7]));
    // al-5: a soft, mid-tone texture, mirrored.
    writeCover(covers, "4-soft", (x, y) => {
      const v = (Math.sin(x / 9) * Math.cos(y / 11) * 0.5 + 0.5) * 120;
      return [Math.round(60 + v), Math.round(90 + v * 0.6), Math.round(110 + v * 0.8)];
    });
  });
  test.afterAll(() => rmSync(covers, { recursive: true, force: true }));

  test("wide fullscreen: centred when both sides carry on, on the left when only the right does; a view slides it left and back", async () => {
    const userData = mkdtempSync(join(tmpdir(), "hocket-e2e-"));
    const { app, page } = await launchFake(userData, { HOCKET_FAKE_COVERS: covers });
    try {
      await completeSetup(page);
      await resizeWindow(app, page, 1456, 912);
      await playCover(page, 1);
      await page.getByTestId("toggle-fullscreen").click();
      const fs = page.getByTestId("fullscreen-player");
      if ((await fs.getAttribute("data-mode")) !== "art") await fs.getByTestId(`fs-mode-${await fs.getAttribute("data-mode")}`).click();
      await expect(fs).toHaveAttribute("data-layout", "immersive");
      await expect(fs).toHaveAttribute("data-sides", "both");
      await expect(fs).toHaveAttribute("data-centred", "");
      await expect(fs.getByTestId("np-continuation-left")).toHaveCount(1);
      const artX = () => fs.locator(".np-art-slot").evaluate((el) => Math.round(el.getBoundingClientRect().left));
      const cardX = () => fs.getByTestId("fs-controls").evaluate((el) => Math.round(el.getBoundingClientRect().left + el.getBoundingClientRect().width / 2));
      const { w, h } = await page.evaluate(() => ({ w: window.innerWidth, h: window.innerHeight }));
      await expect.poll(artX).toBe(Math.round((w - h) / 2));
      // The card is centred under the centred artwork.
      await expect.poll(cardX).toBe(Math.round(w / 2));
      // A view slides the artwork (and its card) to the left edge; closing it brings them back.
      await fs.getByTestId("fs-mode-queue").click();
      await expect(fs).not.toHaveAttribute("data-centred", "");
      const during = await artX();
      expect(during).toBeGreaterThan(0);
      await expect.poll(artX).toBe(0);
      await expect.poll(cardX).toBe(Math.round(h / 2));
      // Slid off the edge, the left continuation lets its bitmap go.
      await expect(fs.getByTestId("np-continuation-left")).not.toHaveAttribute("data-drawn", "");
      expect(await fs.getByTestId("np-continuation-left").evaluate((c) => (c as HTMLCanvasElement).width)).toBe(0);
      await fs.getByTestId("fs-mode-queue").click();
      await expect(fs).toHaveAttribute("data-centred", "");
      await expect.poll(artX).toBe(Math.round((w - h) / 2));
      expect(await fs.getByTestId("np-continuation-left").evaluate((c) => (c as HTMLCanvasElement).width)).toBeGreaterThan(0);
      // A track change mid-slide leaves no stale continuation: the right-only cover's layout wins.
      await fs.getByTestId("fs-mode-queue").click();
      await playCover(page, 2);
      await fs.getByTestId("fs-mode-queue").click();
      await expect(fs).toHaveAttribute("data-mode", "art");
      await expect(fs).toHaveAttribute("data-layout", "immersive");
      await expect(fs).not.toHaveAttribute("data-sides", "both");
      await expect(fs.getByTestId("np-continuation-left")).toHaveCount(0);
      await expect(fs.getByTestId("np-continuation")).toHaveCount(1);
      await expect.poll(artX).toBe(0);
      // A busy cover is a card.
      await playCover(page, 3);
      await expect(fs).toHaveAttribute("data-layout", "columns");
      await expect(fs.getByTestId("np-continuation")).toHaveCount(0);
    } finally {
      await app.close().catch(() => undefined);
      rmSync(userData, { recursive: true, force: true });
    }
  });

  test("the side panel's overview: edge to edge over a continuation when the bottom edge carries on, readable; a card otherwise", async () => {
    const userData = mkdtempSync(join(tmpdir(), "hocket-e2e-"));
    const { app, page } = await launchFake(userData, { HOCKET_FAKE_COVERS: covers });
    try {
      await completeSetup(page);
      await resizeWindow(app, page, 1456, 912);
      const panel = page.getByTestId("right-panel");
      const overview = page.getByTestId("side-overview");
      const art = page.getByTestId("sidebar-art");
      for (const theme of ["light", "dark"] as const) {
        await page.evaluate((v) => window.hocket.dispatch({ type: "setSetting", data: { key: "display.theme", value: JSON.stringify(v) } }), theme);
        // Extensions: dark flat, light flat, white with pixel squares, columns; and a reflection.
        for (const [n, style, tone] of [[1, "extend", "dark"], [4, "extend", "light"], [6, "extend", "light"], [7, "extend", undefined], [5, "mirror", undefined], [8, "mirror", "dark"], [9, "mirror", undefined]] as const) {
          await playCover(page, n);
          if (!(await panel.count())) await page.getByTestId("toggle-side-panel").click();
          await expect(panel).toHaveAttribute("data-view", "overview");
          await expect(overview).toHaveClass(/immersive/);
          await expect(overview).toHaveAttribute("data-style", style);
          await expect(page.getByTestId("sidebar-continuation")).toHaveCount(1);
          await expect(page.getByTestId("sidebar-continuation")).toHaveAttribute("data-style", style);
          if (tone) await expect(overview).toHaveAttribute("data-tone", tone);
          await page.waitForTimeout(700);
          // Flush to the pane's top and sides, square (the pane's own corners clip it).
          const pane = await panel.locator(".side-pane").boundingBox();
          const box = await art.boundingBox();
          expect(Math.abs(box!.y - pane!.y)).toBeLessThan(1.5);
          expect(Math.abs(box!.x - pane!.x)).toBeLessThan(1.5);
          expect(await art.evaluate((el) => getComputedStyle(el).borderTopLeftRadius)).toBe("0px");
          // The overview wears the cover's colour to the end; every card is glass over it.
          await expect(overview).toHaveAttribute("data-fill", "");
          await expect(page.getByTestId("sidebar-upnext-card")).not.toHaveCSS("backdrop-filter", "none");
          if (style === "mirror") {
            // A reflection fades into that colour behind the first card under the title row,
            // from the title row to that card's bottom.
            const fade = await overview.evaluate((el) => {
              const title = el.querySelector<HTMLElement>(".side-title-row")!;
              const first = title.nextElementSibling as HTMLElement;
              const start = title.offsetTop + title.offsetHeight;
              return { at: el.style.getPropertyValue("--side-fade-at"), len: el.style.getPropertyValue("--side-fade-len"), want: `${start}px`, wantLen: `${first.offsetTop + first.offsetHeight - start}px` };
            });
            expect(fade.at).toBe(fade.want);
            expect(fade.len).toBe(fade.wantLen);
          } else {
            // An extension's colour reaches the bottom of the pane, the canvas ending in it.
            const pane = await panel.locator(".side-pane").boundingBox();
            const shot = readPng(await page.screenshot({ clip: { x: pane!.x, y: pane!.y, width: pane!.width, height: pane!.height } }));
            const want = (await page.getByTestId("sidebar-scroll").evaluate((el) => (el as HTMLElement).style.getPropertyValue("--side-fill"))).split(",").map(Number);
            // Below the canvas (where columns carry on in their own colours), the base colour to the end.
            const canvasEnd = await page.getByTestId("sidebar-continuation").evaluate((el) => el.getBoundingClientRect().bottom);
            const from = Math.max(Math.ceil(canvasEnd - pane!.y) + 2, 0);
            expect(from, "the canvas ends above the pane's bottom").toBeLessThan(shot.height - 30);
            // And nothing marks where the canvas ends.
            const level = (y: number) => {
              const i = (y * shot.width + 6) * 4;
              return (shot.data[i]! + shot.data[i + 1]! + shot.data[i + 2]!) / 3;
            };
            for (let y = from - 14; y < from + 10; y++) expect(Math.abs(level(y + 1) - level(y)), `${theme} al-${n}: a step where the canvas ends (${y} px)`).toBeLessThanOrEqual(3);
            for (let y = from; y < shot.height - 30; y += 10) {
              const i = (y * shot.width + 6) * 4;
              for (let k = 0; k < 3; k++) expect(Math.abs(shot.data[i + k]! - want[k]!), `${theme} al-${n}: the pane's bottom wears the extension (${y} px)`).toBeLessThanOrEqual(3);
            }
          }
          // No scrollbar: the content is as wide as the pane whether or not it overflows.
          expect(await page.getByTestId("sidebar-scroll").evaluate((el) => (el as HTMLElement).offsetWidth - el.clientWidth)).toBe(0);
          const { min, worst } = await overviewContrast(page);
          expect(min, `${theme} theme, ${tone} continuation: ${worst}`).toBeGreaterThanOrEqual(4.5);
        }
        // A busy cover keeps the rounded card, inset.
        await playCover(page, 3);
        await expect(overview).not.toHaveClass(/immersive/);
        await expect(page.getByTestId("sidebar-continuation")).toHaveCount(0);
        expect(await art.evaluate((el) => getComputedStyle(el).borderTopLeftRadius)).not.toBe("0px");
      }
      // "Never" keeps it a card; the drill-ins never change.
      await playCover(page, 1);
      await expect(overview).toHaveClass(/immersive/);
      await page.evaluate(() => window.hocket.dispatch({ type: "setSetting", data: { key: "display.immersiveArtwork", value: JSON.stringify("never") } }));
      await expect(overview).not.toHaveClass(/immersive/);
    } finally {
      await app.close().catch(() => undefined);
      rmSync(userData, { recursive: true, force: true });
    }
  });
  test("the side panel's reflection fades into the cover's colour behind the first card, smoothly, at any width, with lyrics or without", async () => {
    const userData = mkdtempSync(join(tmpdir(), "hocket-e2e-"));
    const { app, page } = await launchFake(userData, { HOCKET_FAKE_COVERS: covers });
    try {
      await completeSetup(page);
      await resizeWindow(app, page, 1456, 912);
      const panel = page.getByTestId("right-panel");
      for (const theme of ["light", "dark"] as const) {
        await page.evaluate((v) => window.hocket.dispatch({ type: "setSetting", data: { key: "display.theme", value: JSON.stringify(v) } }), theme);
        // Mid-tone, dark and light reflections in both themes (a dark one in the light theme, a light one in the dark).
        for (const [n, name] of [[5, "mirror"], [8, "dark mirror"], [9, "light mirror"]] as const) {
          await playCover(page, n);
          await toTrackWithLyrics(page, true);
          if (!(await panel.count())) await page.getByTestId("toggle-side-panel").click();
          await expect(panel).toHaveAttribute("data-view", "overview");
          await expect(page.getByTestId("sidebar-lyrics-card")).toBeVisible();
          await expect(page.getByTestId("sidebar-continuation")).toHaveCount(1);
          // A reflection, or this checks nothing (an extension does not fade).
          await expect(page.getByTestId("sidebar-continuation")).toHaveAttribute("data-style", "mirror");
          await expect(page.getByTestId("side-overview")).toHaveAttribute("data-style", "mirror");
          await page.waitForTimeout(800);
          await checkFade(page, `${theme} ${name}`);
          const { min, worst } = await overviewContrast(page);
          expect(min, `${theme} theme, ${name}: ${worst}`).toBeGreaterThanOrEqual(4.5);
        }
      }
      // Dragging the panel's edge redraws the continuation once the width holds, not every step.
      const canvas = page.getByTestId("sidebar-continuation");
      const drawnAt = Number(await canvas.getAttribute("data-drawn"));
      const handle = page.getByTestId("side-panel-resize");
      await handle.focus();
      for (let i = 0; i < 4; i++) await page.keyboard.press("ArrowLeft");
      expect(Number(await canvas.getAttribute("data-drawn"))).toBe(drawnAt);
      await expect.poll(async () => Number(await canvas.getAttribute("data-drawn"))).toBe(drawnAt + 64);
      for (let i = 0; i < 4; i++) await page.keyboard.press("ArrowRight");
      await expect.poll(async () => Number(await canvas.getAttribute("data-drawn"))).toBe(drawnAt);
      // Narrower and wider, the fade follows.
      for (let i = 0; i < 3; i++) await page.keyboard.press("ArrowRight");
      await page.waitForTimeout(400);
      await checkFade(page, "narrower");
      for (let i = 0; i < 8; i++) await page.keyboard.press("ArrowLeft");
      await page.waitForTimeout(400);
      await checkFade(page, "wider");
      // Without lyrics, behind Up next (the first card then), in both themes and for each reflection.
      for (const theme of ["light", "dark"] as const) {
        await page.evaluate((v) => window.hocket.dispatch({ type: "setSetting", data: { key: "display.theme", value: JSON.stringify(v) } }), theme);
        for (const [n, name] of [[5, "mirror"], [8, "dark mirror"], [9, "light mirror"]] as const) {
          await playCover(page, n);
          await toTrackWithLyrics(page, false);
          await expect(page.getByTestId("sidebar-lyrics-card")).toHaveCount(0);
          await expect(page.getByTestId("side-overview")).toHaveAttribute("data-style", "mirror");
          await page.waitForTimeout(800);
          const fade = await page.getByTestId("side-overview").evaluate((el) => {
            const title = el.querySelector<HTMLElement>(".side-title-row")!;
            const first = title.nextElementSibling as HTMLElement;
            return { len: Number.parseFloat(el.style.getPropertyValue("--side-fade-len")), card: first.getAttribute("data-card"), want: first.offsetTop + first.offsetHeight - (title.offsetTop + title.offsetHeight) };
          });
          expect(fade.card).toBe("upnext");
          expect(fade.len, `${theme} ${name}, no lyrics: the fade spans Up next`).toBe(fade.want);
          await checkFade(page, `${theme} ${name}, no lyrics`);
          const { min, worst } = await overviewContrast(page);
          expect(min, `${theme} theme, ${name}, no lyrics: ${worst}`).toBeGreaterThanOrEqual(4.5);
        }
      }
    } finally {
      await app.close().catch(() => undefined);
      rmSync(userData, { recursive: true, force: true });
    }
  });
  test("the fullscreen views are one width whatever the layout, beside the artwork and its card", async () => {
    const userData = mkdtempSync(join(tmpdir(), "hocket-e2e-"));
    const { app, page } = await launchFake(userData, { HOCKET_FAKE_COVERS: covers });
    try {
      await completeSetup(page);
      await playCover(page, 1);
      await page.getByTestId("toggle-fullscreen").click();
      const fs = page.getByTestId("fullscreen-player");
      for (const [w, h] of [[1456, 912], [1280, 800]] as const) {
        await resizeWindow(app, page, w, h);
        const widths: Record<string, number> = {};
        // A card cover (columns), one carried on both ways (centred) and one to the right only.
        for (const [n, layout, sides] of [[3, "columns", undefined], [1, "immersive", "both"], [2, "immersive", undefined]] as const) {
          await playCover(page, n);
          if ((await fs.getAttribute("data-mode")) !== "queue") await fs.getByTestId("fs-mode-queue").click();
          await expect(fs).toHaveAttribute("data-layout", layout);
          if (sides) await expect(fs).toHaveAttribute("data-sides", sides);
          else await expect(fs).not.toHaveAttribute("data-sides", "both");
          const pane = fs.getByTestId("np-pane-queue");
          await expect(pane).toBeVisible();
          await page.waitForTimeout(900);
          const box = await pane.boundingBox();
          const card = await fs.getByTestId("fs-controls").boundingBox();
          widths[`${layout}${sides ? `-${sides}` : ""}`] = Math.round(box!.width);
          // It fits: inside the window, clear of the controls card.
          expect(box!.x + box!.width).toBeLessThanOrEqual(w);
          expect(card!.x + card!.width, `${w}×${h} ${layout}: the card clears the view`).toBeLessThanOrEqual(box!.x);
        }
        const values = Object.values(widths);
        expect(Math.max(...values) - Math.min(...values), `${w}×${h}: ${JSON.stringify(widths)}`).toBeLessThanOrEqual(1);
      }
    } finally {
      await app.close().catch(() => undefined);
      rmSync(userData, { recursive: true, force: true });
    }
  });
});
