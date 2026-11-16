// Screenshots of the song-menu preview player. Runs only with an output folder:
//
//   HOCKET_SCREENSHOT_DIR=/tmp/shots xvfb-run -a pnpm exec playwright test preview-screenshots
import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test, type Page } from "@playwright/test";
import { completeSetup, expect, launchFake, openPlace, resizeWindow } from "./fixtures";

const out = process.env.HOCKET_SCREENSHOT_DIR;

test.use({ trace: "off" });

async function openPlaylist(page: Page, name: string): Promise<void> {
  await openPlace(page, "playlists");
  await page.getByTestId("playlists-grid").getByTestId("grid-tile").filter({ hasText: name }).first().dblclick();
  await expect(page.getByTestId("view-playlist")).toBeVisible();
  await expect(page.getByTestId("track-row").first()).toBeVisible();
}

test("song preview in the context menu", async () => {
  test.skip(!out, "set HOCKET_SCREENSHOT_DIR");
  test.setTimeout(180_000);
  mkdirSync(out!, { recursive: true });
  for (const theme of ["light", "dark"] as const) {
    const userData = mkdtempSync(join(tmpdir(), "hocket-preview-shots-"));
    console.log("launch", theme);
    const { app, page } = await launchFake(userData);
    page.on("console", (msg) => console.log("renderer", msg.type(), msg.text()));
    const shot = async (name: string) => {
      await page.screenshot({ path: join(out!, `desktop-${theme}-${name}.png`) });
      console.log("shot", name);
    };
    try {
      console.log("setup");
      await completeSetup(page);
      await resizeWindow(app, page, 1280, 820);
      await page.evaluate((value) => window.hocket.dispatch({ type: "setSetting", data: { key: "display.theme", value: JSON.stringify(value) } }), theme);
      console.log("playlist");
      await openPlaylist(page, "Slow Sundays");
      const menu = page.getByTestId("context-menu");
      await page.getByTestId("track-row").first().click({ button: "right" });
      await expect(menu).toBeVisible();
      await expect(menu.locator('[data-action="preview"]')).toBeVisible();
      await page.mouse.move(8, 8);
      await shot("preview-menu");
      await menu.locator('[data-action="preview"]').click();
      await expect(menu.locator('[data-action="preview"]')).toHaveAttribute("aria-label", /playing/);
      await page.waitForTimeout(500);
      await shot("preview-playing");
      await menu.locator('[data-action="preview"]').click();
      await expect(menu.locator('[data-action="preview"]')).toHaveAttribute("aria-label", /paused/);
      await shot("preview-paused");
    } finally {
      await app.close().catch(() => undefined);
      rmSync(userData, { recursive: true, force: true });
    }
  }
});
