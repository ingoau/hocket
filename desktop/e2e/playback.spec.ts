import { completeSetup, expect, playFirstAlbum, test } from "./fixtures";

test.describe("playback and queue", () => {
  test("playing an album fills the queue timeline with the model visible", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playFirstAlbum(page);
    const timeline = page.getByTestId("queue-timeline");
    await expect(timeline).toBeVisible();
    await expect(page.getByTestId("queue-row-current")).toHaveCount(1);
    await expect(page.getByTestId("queue-row-upcoming").first()).toBeVisible();
    await expect(timeline).toContainText("Continue playing");
    // Next moves the current item into history (nothing consumed).
    await page.getByTestId("next").click();
    await expect(page.getByTestId("queue-row-history")).toHaveCount(1);
    await page.getByTestId("previous").click();
    await page.getByTestId("previous").click();
    await expect(page.getByTestId("queue-row-history")).toHaveCount(0);
    // Play next inserts into "Playing next".
    await page.getByTestId("nav-songs").click();
    const row = page.getByTestId("track-row").nth(3);
    await row.click({ button: "right" });
    await page.getByTestId("context-menu").locator('[data-action="playNext"]').click();
    await expect(page.getByTestId("queue-row-next")).toHaveCount(1);
    await expect(page.getByTestId("queue-timeline")).toContainText("Playing next");
    // Position ticks while playing.
    const pos = page.getByTestId("seek-position").first();
    await expect(pos).not.toHaveText("0:00", { timeout: 8000 });
  });

  test("the queue rests on the current item with History above it, and each section clears", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playFirstAlbum(page);
    await page.getByTestId("next").click();
    await page.getByTestId("next").click();
    await expect(page.getByTestId("queue-row-history")).toHaveCount(2);
    await page.getByTestId("toggle-fullscreen").click();
    const fs = page.getByTestId("fullscreen-player");
    // It opens on the queue the first time.
    await expect(fs).toHaveAttribute("data-mode", "queue");
    // History is scrolled out of view above; the current item sits at the top.
    await expect.poll(() => fs.getByTestId("queue-scroll").evaluate((el) => el.scrollTop)).toBeGreaterThan(0);
    const [listTop, currentTop] = await Promise.all([fs.getByTestId("queue-scroll").evaluate((el) => el.getBoundingClientRect().top), fs.getByTestId("queue-row-current").evaluate((el) => el.getBoundingClientRect().top)]);
    expect(currentTop - listTop).toBeLessThan(80);
    // The header's toggle is the queue's autoplay (shuffle and repeat are in the transport).
    const autoplay = fs.getByTestId("queue-autoplay");
    const was = await autoplay.getAttribute("aria-pressed");
    await autoplay.click();
    await expect(autoplay).toHaveAttribute("aria-pressed", was === "true" ? "false" : "true");
    await expect(fs.getByTestId("queue-autoplay-note")).toHaveCount(was === "true" ? 0 : 1);
    await fs.getByTestId("fs-repeat").click();
    await expect(fs.getByTestId("fs-repeat")).toHaveAttribute("aria-pressed", "true");
    await fs.getByTestId("queue-clear-history").click();
    await expect(page.getByTestId("queue-row-history")).toHaveCount(0);
    await expect(fs.getByTestId("queue-row-current")).toHaveCount(1);
  });

  test("the fullscreen player opens with F on the queue and shows lyrics, related and up next", async ({ hocket }) => {
    const { app, page } = hocket;
    const windowFullscreen = () => app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows().some((w) => w.isFullScreen()));
    await completeSetup(page);
    await playFirstAlbum(page);
    await page.getByTestId("content").click();
    await page.keyboard.press("f");
    const fs = page.getByTestId("fullscreen-player");
    await expect(fs).toBeVisible();
    // Opening it leaves the window as it was.
    expect(await windowFullscreen()).toBe(false);
    await expect(fs.getByTestId("fs-window-fullscreen")).toHaveAttribute("aria-pressed", "false");
    // It covers the whole window, sidebar and player bar included.
    // (Polled: the layout may still be settling as it opens.)
    await expect.poll(async () => {
      const box = (await fs.boundingBox())!;
      const win = await page.evaluate(() => ({ w: window.innerWidth, h: window.innerHeight }));
      return [Math.round(box.x), Math.round(box.y), Math.round(box.width) - win.w, Math.round(box.height) - win.h];
    }).toEqual([0, 0, 0, 0]);
    // The play/pause glyph is visible against its round button.
    const colours = await fs.getByTestId("fs-play-pause").evaluate((el) => { const cs = getComputedStyle(el); return [cs.color, cs.backgroundColor]; });
    expect(colours[0]).not.toBe(colours[1]);
    // The fluid background is fed the cover art, not the fallback gradient.
    const bg = fs.getByTestId("fluid-bg");
    await expect(bg).toHaveAttribute("data-source", "artwork");
    // Volume lives in the pill (the player bar is covered): a popover with the slider.
    await fs.getByTestId("fs-pill-volume").click();
    const vol = fs.getByTestId("fs-volume");
    await expect(vol).toBeVisible();
    await expect(vol).toBeFocused();
    const v0 = Number(await vol.inputValue());
    await page.keyboard.press("ArrowLeft");
    await expect.poll(async () => Math.round(Number(await vol.inputValue()) * 100)).toBe(Math.round((v0 - 0.05) * 100));
    // Escape closes the popover, not the player.
    await page.keyboard.press("Escape");
    await expect(fs.getByTestId("fs-volume-popover")).toHaveCount(0);
    await expect(fs).toBeVisible();
    // It opens on the queue the first time; the toggle that is on goes back to the artwork alone.
    await expect(fs.getByTestId("fs-mode-queue")).toHaveAttribute("aria-pressed", "true");
    await expect(fs.getByTestId("queue-timeline")).toBeVisible();
    await expect(fs.getByTestId("queue-header")).toBeVisible();
    // The queue's name is its selector's (there is no "Playing from" header).
    await expect(fs.getByTestId("queue-selector-label")).not.toHaveText("");
    await expect(fs.getByText("Playing from")).toHaveCount(0);
    await fs.getByTestId("fs-mode-queue").click();
    await expect(fs.getByTestId("np-artwork")).toBeVisible();
    // Lyrics / Queue / About take the right; the artwork stays beside them, now a button back to
    // it alone (np-thumb: the big artwork here, in the columns layout; a thumbnail by the title
    // only in the stacked one).
    await fs.getByTestId("fs-mode-queue").click();
    await expect(fs.getByTestId("fs-mode-queue")).toHaveAttribute("aria-pressed", "true");
    await expect(fs.getByTestId("np-artwork")).toHaveCount(0);
    await expect(fs.getByTestId("np-thumb")).toBeVisible();
    await fs.getByTestId("fs-mode-about").click();
    await expect(fs.getByTestId("np-about")).toContainText("Duration");
    await expect(fs.getByTestId("related-list")).toBeVisible();
    // The chosen view is remembered when the player closes and reopens.
    await page.keyboard.press("Escape");
    await expect(fs).toHaveCount(0);
    await page.getByTestId("toggle-fullscreen").click();
    await expect(fs.getByTestId("np-about")).toBeVisible();
    // Choosing it again (or the artwork) brings the artwork back alone.
    await fs.getByTestId("fs-mode-about").click();
    await expect(fs.getByTestId("np-artwork")).toBeVisible();
    await expect(fs.getByTestId("fs-mode-about")).toHaveAttribute("aria-pressed", "false");
    // L and Q switch the stage while the player is open.
    await page.keyboard.press("l");
    await expect(fs.getByTestId("np-pane-lyrics")).toBeVisible();
    await fs.getByTestId("np-thumb").click();
    await expect(fs.getByTestId("np-artwork")).toBeVisible();
    // Its full screen button puts the window into fullscreen; closing the player takes it back out.
    await fs.getByTestId("fs-window-fullscreen").click();
    await expect.poll(windowFullscreen).toBe(true);
    await expect(fs.getByTestId("fs-window-fullscreen")).toHaveAttribute("aria-pressed", "true");
    await page.getByTestId("fullscreen-exit").click();
    await expect(page.getByTestId("fullscreen-player")).toHaveCount(0);
    await expect.poll(windowFullscreen).toBe(false);
  });
});
