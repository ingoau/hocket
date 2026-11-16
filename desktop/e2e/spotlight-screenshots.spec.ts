// Home's Spotlight carousel: the first slide (an album to resume), a later one, and the Customise
// panel with Spotlight pinned first, in both themes. Runs only with HOCKET_SCREENSHOT_DIR.
//
//   pnpm build && HOCKET_SCREENSHOT_DIR=/tmp/shots xvfb-run -a -s "-screen 0 2000x1600x24" pnpm test:e2e spotlight-screenshots
import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test, type Page } from "@playwright/test";
import type { AccountInfo } from "../src/core/api";
import { completeSetup, expect, launchFake, resizeWindow } from "./fixtures";

const out = process.env.HOCKET_SCREENSHOT_DIR;

test.use({ trace: "off" });

async function settle(page: Page): Promise<void> {
  await page.waitForTimeout(600);
  await page.evaluate(() => Promise.race([
    Promise.all(document.getAnimations().filter((a) => a.effect?.getTiming().iterations !== Number.POSITIVE_INFINITY).map((a) => a.finished.catch(() => undefined))),
    new Promise((r) => setTimeout(r, 2000)),
  ]));
  await page.waitForTimeout(200);
}

test("spotlight", async () => {
  test.skip(!out, "set HOCKET_SCREENSHOT_DIR");
  test.setTimeout(0);
  mkdirSync(out!, { recursive: true });
  for (const theme of ["dark", "light"] as const) {
    const userData = mkdtempSync(join(tmpdir(), "hocket-spotlight-"));
    const { app, page } = await launchFake(userData);
    try {
      await completeSetup(page);
      await page.addStyleTag({ content: '[data-testid="toasts"], [data-testid="dev-banner"] { display: none !important; }' });
      await resizeWindow(app, page, 1440, 900);
      await page.evaluate((theme) => window.hocket.dispatch({ type: "setSetting", data: { key: "display.theme", value: JSON.stringify(theme) } }), theme);
      const servers = (await page.evaluate(() => window.hocket.query({ type: "accounts" }))) as { data: AccountInfo[] };
      const serverId = servers.data[0]!.id;
      // Leave an album part-way (its resume point), then play another one.
      const play = (albumId: string) => page.evaluate(({ serverId, albumId }) => window.hocket.dispatch({ type: "playContext", data: { args: { context: { source: serverId, kind: { type: "album", data: { id: albumId } }, label: "Album", sort: "default", tracks: [] }, startIndex: 0, shuffle: false, saveOutgoing: false } } }), { serverId, albumId });
      await play("al-1");
      await page.evaluate(() => window.hocket.dispatch({ type: "next" }));
      await page.evaluate(() => window.hocket.dispatch({ type: "seekTo", data: { position_ms: 75000 } }));
      await play("al-2");
      await page.evaluate(() => window.hocket.dispatch({ type: "pause" }));
      await expect(page.getByTestId("view-home")).toBeVisible();
      await expect(page.getByTestId("home-spotlight")).toBeVisible();
      // Resume comes first; the carousel stays on the slide that was in view when it arrived, so go to it.
      await expect(page.getByTestId("spotlight-slide-resume")).toHaveCount(1);
      await page.locator(".spot-dot").first().click();
      await expect(page.getByTestId("spotlight-slide-resume")).toHaveClass(/active/);
      await settle(page);
      await page.screenshot({ path: join(out!, `desktop-${theme}-spotlight-1.png`) });
      await page.getByTestId("spotlight-next").click();
      await settle(page);
      await page.screenshot({ path: join(out!, `desktop-${theme}-spotlight-2.png`) });
      await page.getByTestId("spotlight-next").click();
      await settle(page);
      await page.screenshot({ path: join(out!, `desktop-${theme}-spotlight-3.png`) });
      if (theme === "dark") {
        await page.getByTestId("home-customise").click();
        await expect(page.getByTestId("home-customise-panel")).toBeVisible();
        await settle(page);
        await page.screenshot({ path: join(out!, `desktop-${theme}-spotlight-customise.png`) });
      }
    } finally {
      await app.close();
      rmSync(userData, { recursive: true, force: true });
    }
  }
});
