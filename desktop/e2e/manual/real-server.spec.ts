// Manual check against a real Navidrome with the NATIVE core. Skipped unless
// HOCKET_TEST_URL / HOCKET_TEST_USER / HOCKET_TEST_PASS are set in the
// environment (load them from a private env file at run time; never commit
// them). Adds the server, waits for the sync, plays the track named by
// HOCKET_TEST_TRACK (default "Tally"), opens lyrics and screenshots the
// lyrics view and the album library into HOCKET_SHOTS_DIR (default
// test-results/shots). Run with a display:
//   set -a; . /path/to/creds.env; set +a; xvfb-run -a pnpm test:e2e e2e/manual
import { _electron as electron, expect, test, type ElectronApplication, type Page } from "@playwright/test";
import { existsSync, mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const root = resolve(__dirname, "..", "..");
const addonBuilt = existsSync(join(root, "native", "index.js"));
const url = process.env.HOCKET_TEST_URL;
const user = process.env.HOCKET_TEST_USER;
const pass = process.env.HOCKET_TEST_PASS;
const trackTitle = process.env.HOCKET_TEST_TRACK ?? "Tally";
const shots = process.env.HOCKET_SHOTS_DIR ?? join(root, "test-results", "shots");

async function launch(userData: string): Promise<{ app: ElectronApplication; page: Page }> {
  const env: Record<string, string> = {};
  for (const [k, v] of Object.entries(process.env)) if (v !== undefined && k !== "HOCKET_FAKE_CORE" && !k.startsWith("HOCKET_TEST_")) env[k] = v;
  Object.assign(env, { HOCKET_USER_DATA: userData, HOCKET_DEVICE_NAME: "manual-desktop", HOCKET_LOG: process.env.HOCKET_LOG ?? "warn", HOCKET_INSECURE_CREDENTIAL_STORE: "1" });
  const app = await electron.launch({ args: [root, "--no-sandbox", "--disable-gpu"], env });
  const page = await app.firstWindow();
  await page.waitForLoadState("domcontentloaded");
  await page.setViewportSize({ width: 1400, height: 900 });
  return { app, page };
}

test.describe("real server (manual)", () => {
  test.skip(!url || !user || !pass, "HOCKET_TEST_URL/USER/PASS not set");
  test.skip(!addonBuilt, "native addon not built (run pnpm gen)");
  test.setTimeout(600_000);

  test("add server, sync, play the test track, open lyrics, screenshot", async () => {
    const userData = mkdtempSync(join(tmpdir(), "hocket-manual-"));
    mkdirSync(shots, { recursive: true });
    const { app, page } = await launch(userData);
    try {
      await expect(page.getByTestId("setup")).toBeVisible();
      await page.getByTestId("setup-url").fill(url!);
      await page.getByTestId("setup-username").fill(user!);
      await page.getByTestId("setup-password").fill(pass!);
      await page.getByTestId("setup-connect").click();
      await expect(page.getByTestId("app")).toBeVisible({ timeout: 60_000 });
      await expect(page.getByTestId("dev-banner")).toHaveCount(0);

      // Library sync: wait for albums to arrive and for the sync job to finish.
      await page.getByTestId("nav-albums").click();
      await expect(page.getByTestId("grid-tile").first()).toBeVisible({ timeout: 300_000 });
      await page.waitForTimeout(3000);
      await page.screenshot({ path: join(shots, "desktop-library.png") });

      // Find the track by title and play it from the search results.
      const search = page.getByTestId("search-input");
      await search.click();
      await search.fill(trackTitle);
      const result = page.getByTestId("search-result").filter({ hasText: trackTitle }).first();
      await expect(result).toBeVisible({ timeout: 30_000 });
      await result.click(); // a track result plays on pick
      await page.keyboard.press("Escape");
      // A track result navigates or plays depending on the row kind: make sure something is playing.
      const current = page.getByTestId("queue-row-current");
      if (!(await current.count())) {
        const row = page.getByTestId("track-row").filter({ hasText: trackTitle }).first();
        await expect(row).toBeVisible({ timeout: 30_000 });
        await row.dblclick();
      }
      await expect(current).toContainText(trackTitle, { timeout: 60_000 });

      // Lyrics in the side panel.
      const lyricsButton = page.getByTestId("toggle-lyrics");
      if ((await lyricsButton.getAttribute("aria-pressed")) !== "true") await lyricsButton.click();
      const view = page.getByTestId("lyrics-view");
      await expect(view).toBeVisible({ timeout: 60_000 });
      const tier = await view.getAttribute("data-tier");
      console.log(`[manual] lyrics tier for "${trackTitle}": ${tier}`);
      // Let a few lines go by so the sweep is visible in the capture.
      await page.waitForTimeout(16_000);
      await page.screenshot({ path: join(shots, "desktop-lyrics.png") });
      await page.getByTestId("lyrics-pane").screenshot({ path: join(shots, "desktop-lyrics-pane.png") });
      const active = view.locator('[class*="lyricLine"][class*="active"]:not([class*="lyricBgLine"])').first();
      const syllables = await active.locator('[class*="lyricMainLine"] span:not(:has(span))').count().catch(() => 0);
      console.log(`[manual] active line syllable spans: ${syllables}`);

      // Fullscreen lyrics too.
      await page.getByTestId("content").click();
      await page.keyboard.press("f");
      await expect(page.getByTestId("fullscreen-player")).toBeVisible();
      await page.getByTestId("fs-tab-lyrics").click();
      await page.waitForTimeout(4000);
      await page.screenshot({ path: join(shots, "desktop-fullscreen-lyrics.png") });
      expect(tier).toBeTruthy();
    } finally {
      await app.close().catch(() => undefined);
      rmSync(userData, { recursive: true, force: true });
    }
  });
});
