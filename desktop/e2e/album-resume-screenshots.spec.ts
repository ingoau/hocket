// Album resume: the album page's Resume button and progress ring, Start over in More,
// and the setting. Runs only with HOCKET_SCREENSHOT_DIR (the fake core, like the other shots).
//
//   pnpm build && HOCKET_SCREENSHOT_DIR=/tmp/shots xvfb-run -a pnpm exec playwright test e2e/album-resume-screenshots.spec.ts
import { mkdirSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test, type Page } from "@playwright/test";
import type { AccountInfo } from "../src/core/api";
import { completeSetup, expect, launchFake, resizeWindow } from "./fixtures";

const out = process.env.HOCKET_SCREENSHOT_DIR;

test.use({ trace: "off" });

async function settle(page: Page): Promise<void> {
  await page.waitForTimeout(300);
  await page.evaluate(() => Promise.race([
    Promise.all(document.getAnimations().filter((a) => a.effect?.getTiming().iterations !== Number.POSITIVE_INFINITY).map((a) => a.finished.catch(() => undefined))),
    new Promise((r) => setTimeout(r, 2000)),
  ]));
}

test("album resume", async () => {
  test.skip(!out, "set HOCKET_SCREENSHOT_DIR");
  test.setTimeout(120_000);
  mkdirSync(out!, { recursive: true });
  const userData = mkdtempSync(join(tmpdir(), "hocket-album-resume-"));
  const { app, page } = await launchFake(userData);
  try {
    await completeSetup(page);
    await page.addStyleTag({ content: '[data-testid="toasts"], [data-testid="dev-banner"] { display: none !important; }' });
    await resizeWindow(app, page, 1280, 800);
    const servers = (await page.evaluate(() => window.hocket.query({ type: "accounts" }))) as { data: AccountInfo[] };
    const serverId = servers.data[0]!.id;
    const albumId = "al-1";
    await page.evaluate(({ serverId, albumId }) => window.hocket.dispatch({ type: "playContext", data: { args: { context: { source: serverId, kind: { type: "album", data: { id: albumId } }, label: "Album", sort: "default", tracks: [] }, startIndex: 0, shuffle: false, saveOutgoing: false } } }), { serverId, albumId });
    await page.evaluate(() => window.hocket.dispatch({ type: "next" }));
    await page.evaluate(() => window.hocket.dispatch({ type: "seekTo", data: { position_ms: 75000 } }));
    await page.evaluate(() => window.hocket.dispatch({ type: "pause" }));
    await expect(page.getByTestId("player-bar")).toBeVisible();
    await page.getByTestId("bar-artist").click();
    await page.getByTestId("artist-menu-album").click();
    await expect(page.getByTestId("view-album")).toBeVisible();
    await expect(page.getByTestId("album-play")).toContainText("Resume");
    // Leave the album playing something else, so the page offers Resume and the ring
    // (while this album is what is playing, the button stays Play).
    await page.evaluate(({ serverId }) => window.hocket.dispatch({ type: "playContext", data: { args: { context: { source: serverId, kind: { type: "album", data: { id: "al-2" } }, label: "Other", sort: "default", tracks: [] }, startIndex: 0, shuffle: false, saveOutgoing: false } } }), { serverId });
    await expect(page.getByTestId("album-play")).toContainText("Resume");
    await page.getByTestId("track-row").nth(1).scrollIntoViewIfNeeded();
    await settle(page);
    await page.screenshot({ path: join(out!, "desktop-album-resume.png") });
    await page.getByTestId("album-more").click();
    await expect(page.getByRole("menuitem", { name: "Start over" })).toBeVisible();
    await settle(page);
    await page.screenshot({ path: join(out!, "desktop-album-start-over.png") });
    await page.keyboard.press("Escape");
    await page.getByTestId("nav-settings").click();
    await expect(page.getByTestId("view-settings")).toBeVisible();
    await page.locator("#setting-resumeAlbums").scrollIntoViewIfNeeded();
    await settle(page);
    await page.screenshot({ path: join(out!, "desktop-resume-setting.png") });
  } finally {
    await app.close();
  }
});
