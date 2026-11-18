// Two-finger trackpad swipes on the real surfaces: a queue row swipes away (remove, by default), a
// song row runs its swipe action (play next), and a swipe over the page goes back. A short swipe
// springs back, a vertical scroll and a mouse wheel's notches never swipe, and Settings turns a
// surface off. Playwright's wheel input carries no AppKit phases, so this runs on Linux's idle-gap
// release, with natural scrolling (fingers to the right are negative deltaX).
import type { Locator, Page } from "@playwright/test";
import { completeSetup, expect, openPlace, playFirstAlbum, resizeWindow, test } from "./fixtures";

const out = process.env.HOCKET_SCREENSHOT_DIR;

async function pointAt(page: Page, target: Locator): Promise<{ width: number }> {
  await target.scrollIntoViewIfNeeded();
  const box = await target.boundingBox();
  if (!box) throw new Error("no box");
  await page.mouse.move(box.x + Math.min(box.width / 2, 160), box.y + box.height / 2);
  return { width: box.width };
}

/** Fingers moving `fingersPx` sideways (positive: right) over `target`, in small trackpad-sized steps. */
async function swipeOver(page: Page, target: Locator, fingersPx: number, steps = 16): Promise<void> {
  await pointAt(page, target);
  for (let i = 0; i < steps; i++) await page.mouse.wheel(-fingersPx / steps, 0);
  // Past the idle gap: the swipe has ended.
  await page.waitForTimeout(300);
}

test.describe("trackpad swipes", () => {
  test.beforeEach(async ({ hocket }) => {
    await completeSetup(hocket.page);
    await resizeWindow(hocket.app, hocket.page, 1400, 1000);
  });

  test("a queue row swipes away; short swipes, scrolls and mouse wheels do not", async ({ hocket }) => {
    const { page } = hocket;
    await playFirstAlbum(page);
    const rows = page.getByTestId("queue-row-rest");
    await expect(rows.first()).toBeVisible();
    const count = await rows.count();
    expect(count).toBeGreaterThan(2);
    const { width } = await pointAt(page, rows.first());

    // Short of the threshold (20% of the row): springs back.
    await swipeOver(page, rows.first(), width * 0.08);
    await expect(rows).toHaveCount(count);
    await expect(rows.first()).not.toHaveClass(/swiping/);

    // Vertical: a scroll, never a swipe.
    await pointAt(page, rows.nth(1));
    for (let i = 0; i < 10; i++) await page.mouse.wheel(4, 30);
    await page.waitForTimeout(300);
    await expect(rows).toHaveCount(count);

    // A tilt wheel's notches, 100 px each: a mouse, never a swipe.
    await pointAt(page, rows.first());
    for (let i = 0; i < 4; i++) await page.mouse.wheel(-100, 0);
    await page.waitForTimeout(300);
    await expect(rows).toHaveCount(count);

    // Past it: the row slides out and the queue drops it.
    const title = (await rows.first().locator(".t1").textContent())!.trim();
    await pointAt(page, rows.first());
    for (let i = 0; i < 16; i++) await page.mouse.wheel(-(width * 0.5) / 16, 0);
    if (out) await page.screenshot({ path: `${out}/gestures-queue-armed.png` });
    await expect(rows).toHaveCount(count - 1);
    await expect(rows.locator(".t1", { hasText: title })).toHaveCount(0);
  });

  test("a song row's swipe right plays it next", async ({ hocket }) => {
    const { page } = hocket;
    await playFirstAlbum(page);
    await openPlace(page, "songs");
    const row = page.getByTestId("track-row").nth(4);
    await expect(row).toBeVisible();
    const title = (await row.locator(".title").textContent())!.trim();
    await expect(page.getByTestId("queue-row-group")).toHaveCount(0);
    const { width } = await pointAt(page, row);
    await swipeOver(page, row, width * 0.4);
    await expect(page.getByTestId("queue-row-group").first()).toContainText(title);
    await expect(page.getByTestId("view-songs")).toBeVisible();
  });

  test("a swipe over the page goes back; with page swipes off it does not", async ({ hocket }) => {
    const { page } = hocket;
    await openPlace(page, "albums");
    await expect(page.getByTestId("view-albums")).toBeVisible();
    const header = page.getByTestId("view-albums").locator(".view-header");
    const { width } = await pointAt(page, header);
    await swipeOver(page, header, width * 0.3);
    await expect(page.getByTestId("view-library")).toBeVisible();

    // Off in Settings → Gestures: the same swipe stays put.
    await page.getByTestId("nav-settings").click();
    await page.getByTestId("settings-nav-gestures").click();
    await page.getByTestId("setting-gesture-page").click();
    await expect(page.getByTestId("setting-gesture-page")).not.toBeChecked();
    await openPlace(page, "albums");
    await swipeOver(page, page.getByTestId("view-albums").locator(".view-header"), width * 0.3);
    await expect(page.getByTestId("view-albums")).toBeVisible();
  });
});
