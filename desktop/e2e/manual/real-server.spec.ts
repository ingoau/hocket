// Manual check against a real Navidrome with the NATIVE core. Skipped unless
// HOCKET_TEST_URL / HOCKET_TEST_USER / HOCKET_TEST_PASS are set in the
// environment (load them from a private env file at run time; never commit
// them). Adds the server, waits for the sync, plays the track named by
// HOCKET_TEST_TRACK (default "Tally"), opens lyrics, checks the word-level
// (syllable) sweep and background sub-lines, and screenshots the library, the
// sidebar at two widths, the lyrics (in-window and fullscreen) and settings
// into HOCKET_SHOTS_DIR (default test-results/shots). The server row in
// settings is masked so no screenshot shows the URL or user name. Run with a
// display:
//   set -a; . /path/to/creds.env; set +a; xvfb-run -a pnpm exec playwright test e2e/manual
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

// Nothing that could capture the typed credentials is recorded.
test.use({ trace: "off", screenshot: "off", video: "off" });

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

async function dragSidebarTo(page: Page, width: number): Promise<void> {
  const handle = page.getByTestId("sidebar-resize");
  const sb = (await page.getByTestId("sidebar").boundingBox())!;
  const h = (await handle.boundingBox())!;
  const y = h.y + h.height / 2;
  await page.mouse.move(h.x + h.width / 2, y);
  await page.mouse.down();
  await page.mouse.move(h.x + h.width / 2 + (sb.x + width - (sb.x + sb.width)), y, { steps: 8 });
  await page.mouse.up();
  const after = (await page.getByTestId("sidebar").boundingBox())!;
  const content = (await page.getByTestId("content").boundingBox())!;
  expect(Math.round(content.x)).toBe(Math.round(after.x + after.width));
}

test.describe("real server (manual)", () => {
  test.skip(!url || !user || !pass, "HOCKET_TEST_URL/USER/PASS not set");
  test.skip(!addonBuilt, "native addon not built (run pnpm gen)");
  test.setTimeout(600_000);

  test("add server, sync, play the test track, word-synced lyrics, screenshots", async () => {
    const userData = mkdtempSync(join(tmpdir(), "hocket-manual-"));
    mkdirSync(shots, { recursive: true });
    const { app, page } = await launch(userData);
    const shot = (name: string) => page.screenshot({ path: join(shots, `desktop-${name}.png`) });
    try {
      await expect(page.getByTestId("setup")).toBeVisible();
      await page.getByTestId("setup-url").fill(url!);
      await page.getByTestId("setup-username").fill(user!);
      await page.getByTestId("setup-password").fill(pass!);
      await page.getByTestId("setup-connect").click();
      await expect(page.getByTestId("app")).toBeVisible({ timeout: 60_000 });
      await expect(page.getByTestId("dev-banner")).toHaveCount(0);

      // Library sync: wait for albums to arrive and give the sync time to settle.
      await page.getByTestId("nav-albums").click();
      await expect(page.getByTestId("grid-tile").first()).toBeVisible({ timeout: 300_000 });
      await page.waitForTimeout(5000);
      await shot("library");

      // The sidebar at two widths: the content pane follows the splitter.
      await dragSidebarTo(page, 170);
      await page.waitForTimeout(500);
      await shot("sidebar-narrow");
      await dragSidebarTo(page, 320);
      await page.waitForTimeout(500);
      await shot("sidebar-wide");
      await dragSidebarTo(page, 220);

      // Find the track by title and play it.
      const search = page.getByTestId("search-input");
      await search.click();
      await search.fill(trackTitle);
      const result = page.getByTestId("search-result").filter({ hasText: trackTitle }).first();
      await expect(result).toBeVisible({ timeout: 30_000 });
      await result.click();
      await page.keyboard.press("Escape");
      const current = page.getByTestId("queue-row-current");
      if (!(await current.count())) {
        const row = page.getByTestId("track-row").filter({ hasText: trackTitle }).first();
        await expect(row).toBeVisible({ timeout: 30_000 });
        await row.dblclick();
      }
      await expect(current).toContainText(trackTitle, { timeout: 60_000 });

      // Lyrics in the side panel: syllable tier, word-by-word sweep, background sub-lines.
      const lyricsButton = page.getByTestId("toggle-lyrics");
      if ((await lyricsButton.getAttribute("aria-pressed")) !== "true") await lyricsButton.click();
      const view = page.getByTestId("lyrics-view");
      await expect(view).toBeVisible({ timeout: 60_000 });
      await expect(view).toHaveAttribute("data-tier", "syllable", { timeout: 30_000 });
      await expect(page.getByTestId("lyrics-tools")).toContainText("Word-synced");
      const host = page.getByTestId("amll-host");
      const active = host.locator('[class*="lyricLine"][class*="active"]:not([class*="lyricBgLine"])').first();
      await expect(active).toBeVisible({ timeout: 90_000 });
      const syllables = active.locator('[class*="lyricMainLine"] span:not(:has(span))');
      await expect.poll(() => syllables.count(), { timeout: 10_000 }).toBeGreaterThan(1);
      const sweep = await syllables.evaluateAll((els) => els.map((el) => ({ mask: (el as HTMLElement).style.maskImage !== "", animations: el.getAnimations().length })));
      console.log(`[manual] active line: ${sweep.length} syllable spans, ${sweep.filter((s) => s.mask).length} masked, ${sweep.filter((s) => s.animations > 0).length} animated`);
      expect(sweep.every((s) => s.mask)).toBe(true);
      expect(sweep.some((s) => s.animations > 0)).toBe(true);
      const bgLines = await host.locator('[class*="lyricBgLine"]').count();
      console.log(`[manual] background sub-lines: ${bgLines}`);
      expect(bgLines).toBeGreaterThan(0);
      // Let a few lines go by so the sweep is visible in the capture.
      await page.waitForTimeout(8000);
      await shot("nowplaying-lyrics");
      await page.getByTestId("lyrics-pane").screenshot({ path: join(shots, "desktop-lyrics-pane.png") });

      // Fullscreen lyrics.
      // (The button, not F: F in a focused album grid is type-ahead.)
      await page.getByTestId("toggle-fullscreen").click();
      await expect(page.getByTestId("fullscreen-player")).toBeVisible();
      await page.getByTestId("fs-mode-lyrics").click();
      await page.waitForTimeout(4000);
      await shot("fullscreen-lyrics");
      await page.keyboard.press("Escape");
      await expect(page.getByTestId("fullscreen-player")).toHaveCount(0);

      // The songs table, with the track playing.
      await page.getByTestId("nav-songs").click();
      await page.waitForTimeout(2000);
      await shot("songs");

      // Settings. The server row (URL, user name) is masked.
      await page.getByTestId("nav-settings").click();
      await expect(page.getByTestId("view-settings")).toBeVisible();
      await page.waitForTimeout(500);
      await page.screenshot({ path: join(shots, "desktop-settings.png"), mask: [page.getByTestId("server-row").locator(".title")] });
      await page.getByTestId("settings-nav-appearance").click();
      await page.waitForTimeout(500);
      await shot("settings-appearance");
    } finally {
      await app.close().catch(() => undefined);
      rmSync(userData, { recursive: true, force: true });
    }
  });
});
