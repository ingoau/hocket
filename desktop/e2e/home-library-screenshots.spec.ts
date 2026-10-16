// Screenshots of the customisable Home (its sections, a section's menu, the Customise panel) and
// the Library (the hub, the sidebar's pins and a pin's menu with "When clicked"), for reviewing by
// eye. Not an assertion test: it runs only with an output folder.
//
//   pnpm build && HOCKET_SCREENSHOT_DIR=/tmp/shots xvfb-run -a -s "-screen 0 2000x1600x24" pnpm test:e2e home-library-screenshots

import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test, type Page } from "@playwright/test";
import { completeSetup, expect, launchFake, resizeWindow } from "./fixtures";

const out = process.env.HOCKET_SCREENSHOT_DIR;

test.use({ trace: "off" });

async function settle(page: Page): Promise<void> {
  await page.waitForTimeout(400);
  await page.evaluate(() => Promise.race([
    Promise.all(document.getAnimations().filter((a) => a.effect?.getTiming().iterations !== Number.POSITIVE_INFINITY).map((a) => a.finished.catch(() => undefined))),
    new Promise((r) => setTimeout(r, 3000)),
  ]));
  await page.waitForTimeout(150);
}

async function setSetting(page: Page, key: string, value: unknown): Promise<void> {
  await page.evaluate(({ key, value }) => window.hocket.dispatch({ type: "setSetting", data: { key, value: JSON.stringify(value) } }), { key, value });
}

test("home and library", async () => {
  test.skip(!out, "set HOCKET_SCREENSHOT_DIR");
  test.setTimeout(0);
  mkdirSync(out!, { recursive: true });
  for (const theme of ["dark", "light"] as const) {
    const userData = mkdtempSync(join(tmpdir(), "hocket-home-shots-"));
    const { app, page } = await launchFake(userData);
    const shot = async (name: string) => {
      await settle(page);
      await page.screenshot({ path: join(out!, `desktop-${theme}-${name}.png`) });
    };
    try {
      await completeSetup(page);
      await resizeWindow(app, page, 1440, 900);
      await setSetting(page, "display.theme", theme);
      // Pins: a playlist, an album that shuffles when clicked, an artist, a genre and a filter.
      const genres = (await page.evaluate(() => window.hocket.query({ type: "genres", data: { server_id: "" } }).catch(() => undefined))) as { data?: { name: string }[] } | undefined;
      const genre = genres?.data?.[0]?.name ?? "Ambient";
      const playlists = (await page.evaluate(async () => {
        const s = (await window.hocket.query({ type: "servers" })) as { data: { id: string }[] };
        return window.hocket.query({ type: "playlists", data: { server_id: s.data[0]!.id } });
      })) as { data: { id: string }[] };
      await setSetting(page, "library.pins", [
        { kind: "playlist", id: playlists.data[0]?.id ?? "pl-1", tap: "open" },
        { kind: "album", id: "al-2", tap: "shuffle" },
        { kind: "artist", id: "ar-1", tap: "open" },
        { kind: "genre", id: genre, tap: "open" },
        { kind: "filter", id: "builtin:available-offline", tap: "open" },
      ]);
      await expect(page.getByTestId("view-home")).toBeVisible();
      await expect(page.getByTestId("sidebar-pin").first()).toBeVisible();
      await shot("01-home");

      await page.getByTestId("home-section-more-rediscover").or(page.getByTestId("home-section-more-filters")).first().click({ force: true });
      await expect(page.getByTestId("context-menu")).toBeVisible();
      await shot("02-section-menu");
      await page.keyboard.press("Escape");

      await page.getByTestId("home-customise").click();
      await expect(page.getByTestId("home-customise-panel")).toBeVisible();
      await shot("03-customise");
      await page.getByTestId("home-customise").click();

      await page.getByTestId("nav-library").click();
      await expect(page.getByTestId("view-library")).toBeVisible();
      await shot("04-library");

      await page.getByTestId("sidebar-pin").nth(1).click({ button: "right" });
      await expect(page.getByTestId("context-menu")).toBeVisible();
      await shot("05-pin-menu");
      await page.keyboard.press("Escape");

      await page.getByTestId("library-filters").click();
      await expect(page.getByTestId("view-filters")).toBeVisible();
      await shot("06-filters");
    } finally {
      await app.close();
      rmSync(userData, { recursive: true, force: true });
    }
  }
});
