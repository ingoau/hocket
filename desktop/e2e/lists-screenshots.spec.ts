// Screenshots of the library lists (Albums, the Artists grid, Playlists with Pinned and Recent,
// Songs, Genres) and the sort popover, in both themes, for reviewing by eye. Not an assertion test:
// it runs only with an output folder.
//
//   pnpm build && HOCKET_SCREENSHOT_DIR=/tmp/shots xvfb-run -a -s "-screen 0 2000x1600x24" pnpm test:e2e lists-screenshots

import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test, type Page } from "@playwright/test";
import { completeSetup, expect, launchFake, openPlace, resizeWindow } from "./fixtures";

const out = process.env.HOCKET_SCREENSHOT_DIR;

test.use({ trace: "off" });

async function settle(page: Page): Promise<void> {
  await page.waitForTimeout(500);
  await page.evaluate(() => Promise.race([
    Promise.all(document.getAnimations().filter((a) => a.effect?.getTiming().iterations !== Number.POSITIVE_INFINITY).map((a) => a.finished.catch(() => undefined))),
    new Promise((r) => setTimeout(r, 3000)),
  ]));
  await page.waitForTimeout(150);
}

async function setSetting(page: Page, key: string, value: unknown): Promise<void> {
  await page.evaluate(({ key, value }) => window.hocket.dispatch({ type: "setSetting", data: { key, value: JSON.stringify(value) } }), { key, value });
}

test("library lists and the sort popover", async () => {
  test.skip(!out, "set HOCKET_SCREENSHOT_DIR");
  test.setTimeout(0);
  mkdirSync(out!, { recursive: true });
  for (const theme of ["light", "dark"] as const) {
    const userData = mkdtempSync(join(tmpdir(), "hocket-lists-shots-"));
    const { app, page } = await launchFake(userData);
    const shot = async (name: string) => {
      await settle(page);
      await page.screenshot({ path: join(out!, `desktop-${theme}-${name}.png`) });
    };
    try {
      await completeSetup(page);
      await resizeWindow(app, page, 1440, 900);
      await setSetting(page, "display.theme", theme);
      await setSetting(page, "library.pins", [
        { kind: "playlist", id: "pl-3", tap: "open" },
        { kind: "playlist", id: "pl-6", tap: "open" },
        { kind: "album", id: "al-2", tap: "open" },
        { kind: "artist", id: "ar-1", tap: "open" },
        { kind: "track", id: "tr-5", tap: "play" },
      ]);
      await openPlace(page, "albums");
      await expect(page.getByTestId("grid-tile").first()).toBeVisible();
      await shot("list-albums");
      await page.getByTestId("albums-sort").click();
      await expect(page.getByTestId("albums-sort-menu")).toBeVisible();
      await shot("list-sort-popover");
      await page.keyboard.press("Escape");
      await openPlace(page, "artists");
      await page.getByTestId("artists-sort").click();
      await page.getByTestId("artists-sort-menu").getByTestId("sort-option").filter({ hasText: "Albums" }).click();
      await page.getByTestId("sort-descending").click();
      await shot("list-artists-sort-popover");
      await page.keyboard.press("Escape");
      await shot("list-artists");
      await openPlace(page, "playlists");
      await expect(page.getByTestId("playlists-pinned")).toBeVisible();
      await shot("list-playlists");
      await openPlace(page, "songs");
      await expect(page.getByTestId("track-row").first()).toBeVisible();
      await shot("list-songs");
      await openPlace(page, "genres");
      await expect(page.getByTestId("genre-tile").first()).toBeVisible();
      await shot("list-genres");
      await page.getByTestId("nav-library").click();
      await shot("library");
    } finally {
      await app.close().catch(() => undefined);
      rmSync(userData, { recursive: true, force: true });
    }
  }
});
