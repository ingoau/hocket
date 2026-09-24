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
    await fs.getByTestId("fs-mode-queue").click();
    // History is scrolled out of view above; the current item sits at the top.
    await expect.poll(() => fs.getByTestId("queue-scroll").evaluate((el) => el.scrollTop)).toBeGreaterThan(0);
    const [listTop, currentTop] = await Promise.all([fs.getByTestId("queue-scroll").evaluate((el) => el.getBoundingClientRect().top), fs.getByTestId("queue-row-current").evaluate((el) => el.getBoundingClientRect().top)]);
    expect(currentTop - listTop).toBeLessThan(80);
    // The header toggles are the queue's shuffle / repeat / autoplay.
    await fs.getByTestId("queue-repeat").click();
    await expect(fs.getByTestId("queue-repeat")).toHaveAttribute("aria-pressed", "true");
    await expect(fs.getByTestId("fs-repeat")).toHaveAttribute("aria-pressed", "true");
    await fs.getByTestId("queue-clear-history").click();
    await expect(page.getByTestId("queue-row-history")).toHaveCount(0);
    await expect(fs.getByTestId("queue-row-current")).toHaveCount(1);
  });

  test("the fullscreen player opens with F and shows lyrics, related and up next", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playFirstAlbum(page);
    await page.getByTestId("content").click();
    await page.keyboard.press("f");
    const fs = page.getByTestId("fullscreen-player");
    await expect(fs).toBeVisible();
    // It covers the whole window, sidebar and player bar included.
    // (Polled: the window itself may still be entering fullscreen.)
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
    // Wide windows get the desktop layout: the controls column carries volume (the player bar is covered).
    const vol = fs.getByTestId("fs-volume");
    await expect(vol).toBeVisible();
    const v0 = Number(await vol.inputValue());
    await vol.focus();
    await page.keyboard.press("ArrowLeft");
    await expect.poll(async () => Math.round(Number(await vol.inputValue()) * 100)).toBe(Math.round((v0 - 0.05) * 100));
    // It opens on the artwork; Lyrics / Queue / About replace it in place and the artwork shrinks next to the title.
    await expect(fs.getByTestId("np-artwork")).toBeVisible();
    await expect(fs.getByTestId("np-source")).not.toHaveText("");
    await fs.getByTestId("fs-mode-queue").click();
    await expect(fs.getByTestId("fs-mode-queue")).toHaveAttribute("aria-pressed", "true");
    await expect(fs.getByTestId("np-artwork")).toHaveCount(0);
    await expect(fs.getByTestId("np-thumb")).toBeVisible();
    await expect(fs.getByTestId("queue-timeline")).toBeVisible();
    await expect(fs.getByTestId("queue-header")).toBeVisible();
    await fs.getByTestId("fs-mode-about").click();
    await expect(fs.getByTestId("np-about")).toContainText("Duration");
    await expect(fs.getByTestId("related-list")).toBeVisible();
    // The chosen pane is remembered when the player closes and reopens.
    await page.keyboard.press("Escape");
    await expect(fs).toHaveCount(0);
    await page.getByTestId("toggle-fullscreen").click();
    await expect(fs.getByTestId("np-about")).toBeVisible();
    // Choosing it again (or the thumbnail) brings the artwork back.
    await fs.getByTestId("fs-mode-about").click();
    await expect(fs.getByTestId("np-artwork")).toBeVisible();
    await expect(fs.getByTestId("fs-mode-about")).toHaveAttribute("aria-pressed", "false");
    // L and Q switch the stage while the player is open.
    await page.keyboard.press("l");
    await expect(fs.getByTestId("np-pane-lyrics")).toBeVisible();
    await fs.getByTestId("np-thumb").click();
    await expect(fs.getByTestId("np-artwork")).toBeVisible();
    await page.getByTestId("fullscreen-exit").click();
    await expect(page.getByTestId("fullscreen-player")).toHaveCount(0);
  });
});
