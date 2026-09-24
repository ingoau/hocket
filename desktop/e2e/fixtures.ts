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

/** Launch the built app with the fake core against `userData` (a spec may launch several instances in turn). */
export async function launchFake(userData: string, extraEnv: Record<string, string> = {}): Promise<{ app: ElectronApplication; page: Page }> {
  const root = resolve(__dirname, "..");
  const app = await electron.launch({
    args: [root, "--no-sandbox", "--disable-gpu"],
    env: { ...process.env, HOCKET_FAKE_CORE: "1", HOCKET_USER_DATA: userData, HOCKET_FAKE_TIMESCALE: "8", HOCKET_DEVICE_NAME: "e2e-desktop", ELECTRON_ENABLE_LOGGING: "1", ...extraEnv },
  });
  const page = await app.firstWindow();
  await page.waitForLoadState("domcontentloaded");
  return { app, page };
}

export const test = base.extend<{ hocket: AppFixture }>({
  hocket: async ({}, use) => { // eslint-disable-line no-empty-pattern
    const userData = mkdtempSync(join(tmpdir(), "hocket-e2e-"));
    const { app, page } = await launchFake(userData);
    await use({ app, page, userData });
    await app.close().catch(() => undefined);
    rmSync(userData, { recursive: true, force: true });
  },
});

/**
 * Replace the native dialogs in main so a spec can drive save/open/confirm
 * flows: every save and open dialog answers `path`, every message box picks
 * button `response`.
 */
export async function stubDialogs(app: ElectronApplication, path: string, response = 0): Promise<void> {
  await app.evaluate(({ dialog }, opts) => {
    dialog.showSaveDialog = (async () => ({ canceled: false, filePath: opts.path })) as typeof dialog.showSaveDialog;
    dialog.showOpenDialog = (async () => ({ canceled: false, filePaths: [opts.path] })) as typeof dialog.showOpenDialog;
    dialog.showMessageBox = (async () => ({ response: opts.response, checkboxChecked: false })) as typeof dialog.showMessageBox;
  }, { path, response });
}

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
