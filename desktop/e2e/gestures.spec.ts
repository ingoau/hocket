// The gesture playground (Developer diagnostics → Gesture playground): a sideways two-finger
// swipe on a queue row removes it, a short one springs back, and a vertical scroll is no swipe.
// Playwright's wheel input carries no AppKit phases, so this runs on the idle-gap fallback.
import type { Locator, Page } from "@playwright/test";
import { completeSetup, expect, resizeWindow, test } from "./fixtures";

const out = process.env.HOCKET_SCREENSHOT_DIR;

async function swipeOver(page: Page, target: Locator, fingersPx: number, steps = 16): Promise<void> {
  await target.scrollIntoViewIfNeeded();
  const box = await target.boundingBox();
  if (!box) throw new Error("no box");
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  // Natural scrolling (the default without phases): fingers to the right are negative deltaX.
  for (let i = 0; i < steps; i++) await page.mouse.wheel(-fingersPx / steps, 0);
}

test("swipes on the playground's queue remove a row; short swipes and scrolls do not", async ({ hocket }) => {
  const { app, page } = hocket;
  await completeSetup(page);
  await resizeWindow(app, page, 1400, 1000);
  await page.getByTestId("nav-settings").click();
  await page.getByTestId("settings-nav-advanced").click();
  await page.getByTestId("open-developer").click();
  await page.getByTestId("developer-gestures").click();
  await expect(page.getByTestId("view-gestures")).toBeVisible();

  const rows = page.getByTestId("gp-queue").getByTestId("gp-row");
  await expect(rows).toHaveCount(8);
  const width = (await rows.first().boundingBox())?.width ?? 0;

  // Short of the 30% threshold: springs back.
  await swipeOver(page, rows.first(), width * 0.15);
  await page.waitForTimeout(400);
  await expect(rows).toHaveCount(8);

  // Vertical: a scroll, never a swipe.
  await rows.nth(1).scrollIntoViewIfNeeded();
  const box = await rows.nth(1).boundingBox();
  await page.mouse.move(box!.x + box!.width / 2, box!.y + box!.height / 2);
  for (let i = 0; i < 10; i++) await page.mouse.wheel(4, 30);
  await page.waitForTimeout(400);
  await expect(rows).toHaveCount(8);

  // Past it: the row slides out and the queue drops it.
  await swipeOver(page, rows.first(), width * 0.6);
  if (out) await page.screenshot({ path: `${out}/gestures-armed.png` });
  await expect(rows).toHaveCount(7);
  if (out) await page.screenshot({ path: `${out}/gestures.png`, fullPage: true });
});
