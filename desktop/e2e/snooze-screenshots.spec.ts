// Screenshots of snooze: the duration menu, a faded playlist row, Unsnooze with its end, and the
// Snoozed list. Runs only with HOCKET_SCREENSHOT_DIR.
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import type { Page } from "@playwright/test";
import { completeSetup, expect, openPlace, test } from "./fixtures";

const out = process.env.HOCKET_SCREENSHOT_DIR;
const onTitle = { x: 130, y: 12 };

async function shot(page: Page, name: string): Promise<void> {
  if (!out) return;
  mkdirSync(out, { recursive: true });
  await page.screenshot({ path: join(out, name), animations: "disabled" });
}

test("snooze screens", async ({ hocket }) => {
  test.skip(!out, "set HOCKET_SCREENSHOT_DIR");
  test.setTimeout(120_000);
  const { page } = hocket;
  await completeSetup(page);
  await openPlace(page, "playlists");
  await page.getByTestId("playlists-grid").getByTestId("grid-tile").first().dblclick();
  await expect(page.getByTestId("view-playlist")).toBeVisible();
  const rows = page.getByTestId("track-row");
  await expect(rows.first()).toBeVisible();

  await rows.nth(0).click({ position: onTitle, button: "right" });
  const menu = page.getByTestId("context-menu");
  await expect(menu).toBeVisible();
  await menu.locator('[data-action="snooze"]').click();
  await expect(page.getByTestId("snooze-submenu")).toBeVisible();
  await shot(page, "desktop-snooze-durations.png");

  await page.getByTestId("snooze-submenu").locator('[data-action="week"]').click();
  await expect(page.getByTestId("toast").filter({ hasText: "Snoozed" })).toBeVisible();
  await expect(rows.nth(0)).toHaveClass(/snoozed/);
  await page.getByTestId("toast").filter({ hasText: "Snoozed" }).waitFor({ state: "hidden", timeout: 8_000 });
  await shot(page, "desktop-snoozed-row.png");

  await rows.nth(0).click({ position: onTitle, button: "right" });
  await expect(menu.locator('[data-action="unsnooze"]')).toBeVisible();
  await shot(page, "desktop-unsnooze.png");
  await page.keyboard.press("Escape");

  await page.getByTestId("nav-library").click();
  await page.getByTestId("library-snoozed").click();
  await expect(page.getByTestId("view-snoozed")).toBeVisible();
  await expect(page.getByTestId("snoozed-row")).toBeVisible();
  await shot(page, "desktop-snoozed-list.png");
});

test("snooze until a custom date and time", async ({ hocket }) => {
  test.setTimeout(120_000);
  const { page } = hocket;
  await completeSetup(page);
  await openPlace(page, "playlists");
  await page.getByTestId("playlists-grid").getByTestId("grid-tile").first().dblclick();
  await expect(page.getByTestId("view-playlist")).toBeVisible();
  const rows = page.getByTestId("track-row");
  await expect(rows.first()).toBeVisible();

  await rows.nth(0).click({ position: onTitle, button: "right" });
  const menu = page.getByTestId("context-menu");
  await menu.locator('[data-action="snooze"]').click();
  await page.getByTestId("snooze-submenu").locator('[data-action="custom"]').click();
  const dialog = page.getByTestId("dialog-snoozeUntil");
  await expect(dialog).toBeVisible();
  const input = page.getByTestId("snooze-until-input");
  await shot(page, "desktop-snooze-custom.png");

  // A time already past can't be chosen.
  await input.fill("2020-01-01T09:00");
  await expect(page.getByTestId("snooze-until-ok")).toBeDisabled();

  const year = new Date().getFullYear() + 1;
  await input.fill(`${year}-03-04T09:30`);
  await page.getByTestId("snooze-until-ok").click();
  await expect(dialog).toBeHidden();
  await expect(page.getByTestId("toast").filter({ hasText: "Snoozed" })).toBeVisible();
  await expect(rows.nth(0)).toHaveClass(/snoozed/);
});
