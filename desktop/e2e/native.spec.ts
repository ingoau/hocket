// End-to-end against the REAL core (native napi addon) and a fake Navidrome.
// Requires `pnpm gen` (addon) and `pnpm build`. The suite is skipped when the
// addon isn't built so the fake-core suite still runs everywhere.
import { _electron as electron, expect, test, type ElectronApplication, type Page } from "@playwright/test";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { FakeNavidrome } from "./fake-navidrome";

const root = resolve(__dirname, "..");
const addonBuilt = existsSync(join(root, "native", "index.js"));

async function launch(userData: string): Promise<{ app: ElectronApplication; page: Page }> {
  const env: Record<string, string> = {};
  for (const [k, v] of Object.entries(process.env)) if (v !== undefined && k !== "HOCKET_FAKE_CORE") env[k] = v;
  // A headless display server has no keyring, so safeStorage falls back to
  // the fixed-key basic_text backend, which the app refuses to persist with;
  // opt in explicitly so the credential replay after restart is exercised.
  Object.assign(env, { HOCKET_USER_DATA: userData, HOCKET_DEVICE_NAME: "e2e-native", HOCKET_LOG: "warn", ELECTRON_ENABLE_LOGGING: "1", HOCKET_INSECURE_CREDENTIAL_STORE: "1" });
  const app = await electron.launch({ args: [root, "--no-sandbox", "--disable-gpu"], env });
  const page = await app.firstWindow();
  await page.waitForLoadState("domcontentloaded");
  return { app, page };
}

test.describe("real core against a fake Navidrome", () => {
  test.skip(!addonBuilt, "native addon not built (run pnpm gen)");
  test.setTimeout(180_000);

  let server: FakeNavidrome;
  let userData: string;
  test.beforeAll(async () => {
    server = new FakeNavidrome();
    await server.start();
  });
  test.afterAll(async () => {
    await server.stop();
  });
  test.beforeEach(() => {
    userData = mkdtempSync(join(tmpdir(), "hocket-native-"));
  });
  test.afterEach(() => {
    rmSync(userData, { recursive: true, force: true });
  });

  test("setup, sync, play, queue, rate, undo, lyrics, settings and credential replay", async () => {
    let { app, page } = await launch(userData);
    try {
      // First run: setup is the whole screen; the core is the native one.
      await expect(page.getByTestId("setup")).toBeVisible();
      await expect(page.getByTestId("dev-banner")).toHaveCount(0);
      await page.getByTestId("setup-url").fill(server.url);
      await page.getByTestId("setup-username").fill("alice");
      await page.getByTestId("setup-password").fill("secret");
      await page.getByTestId("setup-connect").click();
      await expect(page.getByTestId("app")).toBeVisible({ timeout: 30_000 });
      await expect(page.getByTestId("dev-banner")).toHaveCount(0);
      expect(server.callsTo("ping").length).toBeGreaterThan(0);
      expect(server.callsTo("getOpenSubsonicExtensions").length).toBeGreaterThan(0);

      // Library sync fills Albums.
      await page.getByTestId("nav-albums").click();
      await expect(page.getByTestId("grid-tile").first()).toBeVisible({ timeout: 60_000 });
      await expect(page.getByTestId("grid-tile")).toHaveCount(8, { timeout: 30_000 });
      expect(server.callsTo("search3").length).toBeGreaterThan(0);

      // Play an album: queue timeline shows current + upcoming from the context.
      await page.getByTestId("grid-tile").first().dblclick();
      await expect(page.getByTestId("view-album")).toBeVisible();
      await expect(page.getByTestId("track-row")).toHaveCount(6);
      await page.getByTestId("album-play").click();
      await expect(page.getByTestId("queue-row-current")).toBeVisible({ timeout: 20_000 });
      await expect(page.getByTestId("queue-row-upcoming")).toHaveCount(5);
      await expect(page.getByTestId("queue-timeline")).toContainText("Continuing from");
      const firstTitle = await page.getByTestId("queue-row-current").locator(".t1").textContent();

      // Position extrapolates from TransportChanged stamps while playing.
      await expect(page.getByTestId("seek-position").first()).not.toHaveText("0:00", { timeout: 20_000 });
      expect(server.callsTo("stream").length).toBeGreaterThan(0);

      // Next / previous: nothing consumed, history above.
      await page.getByTestId("next").click();
      await expect(page.getByTestId("queue-row-history")).toHaveCount(1, { timeout: 10_000 });
      await expect(page.getByTestId("queue-row-current").locator(".t1")).not.toHaveText(firstTitle ?? "");
      await page.getByTestId("previous").click();
      await page.getByTestId("previous").click();
      await expect(page.getByTestId("queue-row-history")).toHaveCount(0, { timeout: 10_000 });
      await expect(page.getByTestId("queue-row-current").locator(".t1")).toHaveText(firstTitle ?? "");

      // Rate the current track from the album table; the outbox reaches the server; undo reverts.
      const row = page.getByTestId("track-row").first();
      await row.locator(".stars .star").nth(3).click();
      await expect(row.locator(".stars .star.on")).toHaveCount(4, { timeout: 10_000 });
      await expect.poll(() => server.callsTo("setRating").length, { timeout: 15_000 }).toBeGreaterThan(0);
      // Rating leaves the queue alone, so no toast: undo from the keyboard.
      await expect(page.getByTestId("toast")).toHaveCount(0);
      await page.getByTestId("content").click();
      await page.keyboard.press("Control+z");
      await expect(row.locator(".stars .star.on")).toHaveCount(0, { timeout: 10_000 });

      // Lyrics from getLyricsBySongId render (line tier) in the right panel.
      await expect(page.getByTestId("lyrics-view")).toBeVisible({ timeout: 20_000 });
      await expect(page.getByTestId("lyrics-view")).toHaveAttribute("data-tier", "line");
      expect(server.callsTo("getLyricsBySongId").length).toBeGreaterThan(0);

      // Change a registry setting: theme → dark.
      await page.getByTestId("nav-settings").click();
      await page.getByTestId("settings-nav-appearance").click();
      await page.getByTestId("setting-theme").selectOption("dark");
      await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
      // No settling delay: quitting waits for the core's flush (before-quit → Core::shutdown).
    } finally {
      await app.close();
    }

    ({ app, page } = await launch(userData));
    try {
      // Credentials replayed from safeStorage: no setup screen, library still there, theme persisted.
      await expect(page.getByTestId("app")).toBeVisible({ timeout: 30_000 });
      await expect(page.getByTestId("setup")).toHaveCount(0);
      await expect(page.locator("html")).toHaveAttribute("data-theme", "dark", { timeout: 15_000 });
      await page.getByTestId("nav-albums").click();
      await expect(page.getByTestId("grid-tile").first()).toBeVisible({ timeout: 30_000 });
    } finally {
      await app.close();
    }
  });

  test("a wrong password is reported inline and setup stays", async () => {
    const { app, page } = await launch(userData);
    try {
      await page.getByTestId("setup-url").fill(server.url);
      await page.getByTestId("setup-username").fill("mallory");
      await page.getByTestId("setup-password").fill("x");
      await page.getByTestId("setup-connect").click();
      await expect(page.getByTestId("setup-error")).toBeVisible({ timeout: 30_000 });
      await expect(page.getByTestId("setup")).toBeVisible();
    } finally {
      await app.close();
    }
  });
});
