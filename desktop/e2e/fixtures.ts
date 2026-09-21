// Launches the built app (out/main/index.cjs) with the fake core and a fresh
// user-data dir per test, and drives it through Playwright's Electron API.
import { _electron as electron, test as base, type ElectronApplication, type Page } from "@playwright/test";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

export interface AppFixture {
  app: ElectronApplication;
  page: Page;
  userData: string;
}

export const test = base.extend<{ hocket: AppFixture }>({
  hocket: async ({}, use) => { // eslint-disable-line no-empty-pattern
    const root = resolve(__dirname, "..");
    const userData = mkdtempSync(join(tmpdir(), "hocket-e2e-"));
    const app = await electron.launch({
      args: [root, "--no-sandbox", "--disable-gpu"],
      env: { ...process.env, HOCKET_FAKE_CORE: "1", HOCKET_USER_DATA: userData, HOCKET_FAKE_TIMESCALE: "8", HOCKET_DEVICE_NAME: "e2e-desktop", ELECTRON_ENABLE_LOGGING: "1" },
    });
    const page = await app.firstWindow();
    await page.waitForLoadState("domcontentloaded");
    await use({ app, page, userData });
    await app.close().catch(() => undefined);
    rmSync(userData, { recursive: true, force: true });
  },
});

export const expect = test.expect;

/** Complete first-run setup so the library UI is shown. */
export async function completeSetup(page: Page): Promise<void> {
  await expect(page.getByTestId("setup")).toBeVisible();
  await page.getByTestId("setup-url").fill("https://music.example.org");
  await page.getByTestId("setup-username").fill("alice");
  await page.getByTestId("setup-password").fill("secret");
  await page.getByTestId("setup-connect").click();
  await expect(page.getByTestId("app")).toBeVisible({ timeout: 15_000 });
}

export async function playFirstAlbum(page: Page): Promise<void> {
  await page.getByTestId("nav-albums").click();
  const tile = page.getByTestId("grid-tile").first();
  await expect(tile).toBeVisible();
  await tile.dblclick();
  await expect(page.getByTestId("view-album")).toBeVisible();
  await page.getByTestId("album-play").click();
  await expect(page.getByTestId("queue-row-current")).toBeVisible();
}
