// Screenshots of the context menu and its add to playlist submenu (one song from a playlist, three
// songs, an album; the submenu, searching, Remove both / Remove one, the hover +) and Edit pins, in both themes,
// for reviewing by eye against the menu design. Not an assertion test: it runs only with an output folder.
//
//   pnpm build && HOCKET_SCREENSHOT_DIR=/tmp/shots xvfb-run -a -s "-screen 0 2000x1600x24" pnpm test:e2e menu-screenshots

import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test, type Page } from "@playwright/test";
import { completeSetup, expect, launchFake, openPlace, resizeWindow } from "./fixtures";

const out = process.env.HOCKET_SCREENSHOT_DIR;

test.use({ trace: "off" });

async function settle(page: Page): Promise<void> {
  await page.waitForTimeout(300);
  await page.evaluate(() => Promise.race([
    Promise.all(document.getAnimations().filter((a) => a.effect?.getTiming().iterations !== Number.POSITIVE_INFINITY).map((a) => a.finished.catch(() => undefined))),
    new Promise((r) => setTimeout(r, 3000)),
  ]));
  await page.waitForTimeout(150);
}

async function setSetting(page: Page, key: string, value: unknown): Promise<void> {
  await page.evaluate(({ key, value }) => window.hocket.dispatch({ type: "setSetting", data: { key, value: JSON.stringify(value) } }), { key, value });
}

/** Opens a playlist from the Playlists page by name. */
export async function openPlaylist(page: Page, name: string): Promise<void> {
  await openPlace(page, "playlists");
  await page.getByTestId("playlists-grid").getByTestId("grid-tile").filter({ hasText: name }).first().dblclick();
  await expect(page.getByTestId("view-playlist")).toBeVisible();
  await expect(page.getByTestId("track-row").first()).toBeVisible();
}

test("context menu and add to playlist", async () => {
  test.skip(!out, "set HOCKET_SCREENSHOT_DIR");
  test.setTimeout(0);
  mkdirSync(out!, { recursive: true });
  for (const theme of ["light", "dark"] as const) {
    const userData = mkdtempSync(join(tmpdir(), "hocket-menu-shots-"));
    const { app, page } = await launchFake(userData);
    const shot = async (name: string) => {
      await settle(page);
      await page.screenshot({ path: join(out!, `desktop-${theme}-${name}.png`) });
    };
    try {
      await completeSetup(page);
      await resizeWindow(app, page, 1280, 820);
      await setSetting(page, "display.theme", theme);
      // Two playlists pinned, so the submenu shows all three groups.
      await setSetting(page, "library.pins", [{ kind: "playlist", id: "pl-2", tap: "open" }, { kind: "playlist", id: "pl-1", tap: "open" }]);
      // A few more playlists (they become Recent), one with a name long enough to truncate.
      await page.evaluate(async () => {
        const servers = (await window.hocket.query({ type: "servers" })) as { data: { id: string }[] };
        for (const name of ["Road trip 2024 (the extended summer edition)", "Run club", "Focus"]) window.hocket.dispatch({ type: "createPlaylist", data: { server_id: servers.data[0]!.id, name, track_ids: [] } });
      });
      await openPlaylist(page, "Slow Sundays");
      // The new playlists toast for 5s; wait them out so they don't sit on the menu.
      await expect(page.getByTestId("toast")).toHaveCount(0, { timeout: 12_000 });
      const menu = page.getByTestId("context-menu");
      const row = page.getByTestId("track-row").first();
      await row.click({ button: "right" });
      await expect(menu).toBeVisible();
      await page.mouse.move(5, 5);
      await shot("01-song-menu");
      await menu.locator('[data-action="share"]').hover();
      await expect(page.getByTestId("share-submenu")).toBeVisible();
      await shot("01c-share-submenu");

      const sub = page.getByTestId("playlist-submenu");
      await menu.locator('[data-action="addToPlaylist"]').hover();
      await expect(sub).toBeVisible();
      await page.mouse.move(5, 5);
      await shot("02-submenu");

      await sub.getByTestId("playlist-row").nth(3).hover();
      await shot("03-hover-plus");

      await page.getByTestId("playlist-search").fill("s");
      await shot("04-searching");
      await page.getByTestId("playlist-search").fill("jazz");
      await shot("05-no-match");
      await page.getByTestId("playlist-search").fill("");

      await sub.getByTestId("playlist-row").filter({ hasText: "Slow Sundays" }).click();
      await expect(page.getByTestId("playlist-ask")).toBeVisible();
      await shot("06-remove-both-one");
      await page.keyboard.press("Escape");
      await page.keyboard.press("Escape");
      await page.keyboard.press("Escape");
      await expect(menu).toHaveCount(0);

      // Three songs.
      // Clicked by their title, clear of the play button and the artist and album links.
      await page.getByTestId("track-row").nth(1).click({ position: { x: 130, y: 12 } });
      await page.getByTestId("track-row").nth(3).click({ position: { x: 130, y: 12 }, modifiers: ["Shift"] });
      await page.getByTestId("track-row").nth(2).click({ position: { x: 130, y: 12 }, button: "right" });
      await expect(menu).toBeVisible();
      await page.mouse.move(5, 5);
      await shot("07-multi-menu");
      await page.keyboard.press("Escape");

      // An album.
      await openPlace(page, "albums");
      await page.getByTestId("grid-tile").nth(1).click({ button: "right" });
      await expect(menu).toBeVisible();
      await page.mouse.move(5, 5);
      await shot("08-album-menu");
      await menu.locator('[data-action="share"]').hover();
      await expect(page.getByTestId("share-submenu")).toBeVisible();
      await shot("08b-album-share");
      await menu.locator('[data-action="play"]').hover();
      await expect(page.getByTestId("share-submenu")).toHaveCount(0);
      await menu.getByRole("menuitem", { name: "Album info", exact: true }).click();
      await expect(page.getByTestId("dialog-albumInfo")).toBeVisible();
      await shot("09-album-info");
      await page.keyboard.press("Escape");

      // The player bar's + opens the same picker, beside it.
      await page.getByTestId("grid-tile").nth(1).dblclick();
      await page.getByTestId("album-play").click();
      await page.getByTestId("bar-add").click();
      await expect(page.getByTestId("playlist-submenu")).toBeVisible();
      await page.mouse.move(640, 5);
      await shot("10-player-bar-picker");
      await page.keyboard.press("Escape");

      // The app's own menus keep their icons: a Home section's, a filter's.
      await page.getByTestId("nav-home").click();
      await page.locator('[data-testid^="home-section-head-"]').first().click({ button: "right" });
      await expect(menu).toBeVisible();
      await page.mouse.move(5, 5);
      await shot("11-home-section-menu");
      await page.keyboard.press("Escape");
      await openPlace(page, "filters");
      const filter = page.getByTestId("filter-row").first();
      if (await filter.count()) {
        await filter.click({ button: "right" });
        await expect(menu).toBeVisible();
        await page.mouse.move(5, 5);
        await shot("12-filter-menu");
        await page.keyboard.press("Escape");
      }

      // Edit pins (the phone's pin edit mode): from a pin's menu, a chip cycled, a tile picked up, one unpinned.
      const genres = (await page.evaluate(async () => {
        const s = (await window.hocket.query({ type: "servers" })) as { data: { id: string }[] };
        return window.hocket.query({ type: "genres", data: { server_id: s.data[0]!.id } });
      })) as { data?: { name: string }[] };
      await setSetting(page, "library.pins", [
        { kind: "album", id: "al-2", tap: "play" },
        { kind: "playlist", id: "pl-1", tap: "shuffle" },
        { kind: "artist", id: "ar-1", tap: "open" },
        { kind: "track", id: "tr-1", tap: "play" },
        { kind: "genre", id: genres.data?.[0]?.name ?? "Ambient", tap: "shuffle" },
        { kind: "filter", id: "builtin:available-offline", tap: "open" },
      ]);
      await page.getByTestId("nav-library").click();
      const pinRow = page.getByTestId("sidebar-pin").first();
      await expect(pinRow).toBeVisible();
      await pinRow.click({ button: "right" });
      await expect(menu).toBeVisible();
      await page.mouse.move(5, 5);
      await shot("13-pin-menu-edit-pins");
      await menu.getByRole("menuitem", { name: "Edit pins…" }).click();
      const editor = page.getByTestId("edit-pins-panel");
      await expect(editor.getByTestId("edit-pin").first()).toBeVisible();
      await shot("14-edit-pins");
      await editor.getByTestId("edit-pin-chip").nth(2).click();
      await editor.getByTestId("edit-pin-chip").nth(2).hover();
      await shot("15-edit-pins-chip-cycled");
      await editor.getByTestId("edit-pin-handle").nth(1).focus();
      await page.keyboard.press("Space");
      await page.keyboard.press("ArrowRight");
      await page.mouse.move(5, 5);
      await shot("16-edit-pins-picked-up");
      await page.keyboard.press("Enter");
      await editor.getByTestId("edit-pin-unpin").nth(3).click();
      await expect(page.getByTestId("toast").filter({ hasText: "Unpinned" })).toBeVisible();
      await page.mouse.move(5, 5);
      await shot("17-edit-pins-unpinned");
      await resizeWindow(app, page, 760, 820);
      await shot("18-edit-pins-narrow");
      await resizeWindow(app, page, 1280, 820);
      await editor.getByRole("button", { name: "Cancel editing" }).click();
    } finally {
      await app.close();
      rmSync(userData, { recursive: true, force: true });
    }
  }
});
