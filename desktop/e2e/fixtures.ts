// Launches the built app (out/main/index.cjs) with the fake core and a fresh
// user-data dir per test, and drives it through Playwright's Electron API.
import { _electron as electron, test as base, type ElectronApplication, type Locator, type Page } from "@playwright/test";
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

/**
 * Resize the window's content area to `width`×`height` and wait until the renderer sees it.
 * Through the window's own handle; Playwright's main-process evaluation can lose its result to a
 * garbage collection in the main process ("Resulting promise was garbage collected"), which
 * leaves the resize done or not, so it is retried (setting the same size again is harmless), and
 * the renderer's size, not the call, is what the spec waits on.
 */
export async function resizeWindow(app: ElectronApplication, page: Page, width: number, height: number): Promise<void> {
  let size: [number, number] = [width, height];
  for (let attempt = 0; ; attempt++) {
    try {
      const bw = await app.browserWindow(page);
      // What the window settled on (its minimum size may have the last word).
      size = (await bw.evaluate((w, s) => {
        w.setContentSize(s[0], s[1]);
        return w.getContentSize();
      }, [width, height] as const)) as [number, number];
      await bw.dispose().catch(() => undefined);
      break;
    } catch (error) {
      if (attempt >= 4 || !/garbage collected|Target closed|Execution context/i.test(String(error))) throw error;
      await page.waitForTimeout(100);
    }
  }
  await page.waitForFunction((s) => window.innerWidth === s[0] && window.innerHeight === s[1], size, { timeout: 10_000 });
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
  // The summary of what was found, while the first sync runs; Continue goes in.
  await page.getByTestId("setup-continue").click({ timeout: 15_000 });
  await expect(page.getByTestId("app")).toBeVisible({ timeout: 15_000 });
}

/** A place in the Library, reached as a user would: the sidebar's Library, then its tile or link. */
export type Place = "albums" | "artists" | "playlists" | "songs" | "genres" | "filters" | "queues" | "offline" | "downloads" | "stats";

/** Opens `place` from the Library page (the sidebar lists Home and Library, plus pins). */
export async function openPlace(page: Page, place: Place): Promise<void> {
  await page.getByTestId("nav-library").click();
  await expect(page.getByTestId("view-library")).toBeVisible();
  await page.getByTestId(`library-${place}`).click();
}

/** Plays the first album and shows the queue in the side panel (its overview is the default view). */
export async function playFirstAlbum(page: Page): Promise<void> {
  await openPlace(page, "albums");
  const tile = page.getByTestId("grid-tile").first();
  await expect(tile).toBeVisible();
  await tile.dblclick();
  await expect(page.getByTestId("view-album")).toBeVisible();
  await page.getByTestId("album-play").click();
  await showSideView(page, "queue");
  await expect(page.getByTestId("queue-row-current")).toBeVisible();
}

/** Shows the side panel on `view` with the player bar's buttons, as a user would. */
export async function showSideView(page: Page, view: "lyrics" | "queue"): Promise<void> {
  const panel = page.getByTestId("right-panel");
  if ((await panel.count()) && (await panel.getAttribute("data-view")) === view) return;
  await page.getByTestId(view === "queue" ? "toggle-queue" : "toggle-lyrics").click();
  await expect(panel).toHaveAttribute("data-view", view);
}

/**
 * HTML5 drag-and-drop. Playwright's pointer `dragTo` does not fire dragstart/drop,
 * so library-to-queue drags have to be dispatched (the row's onDragStart fills the payload).
 */
export async function html5DragStart(page: Page, source: Locator): Promise<void> {
  const el = await source.elementHandle();
  if (!el) throw new Error("drag source missing");
  await page.evaluate((src) => {
    const dt = new DataTransfer();
    (window as unknown as { __hocketLibraryDrag?: DataTransfer }).__hocketLibraryDrag = dt;
    src.dispatchEvent(new DragEvent("dragstart", { bubbles: true, cancelable: true, composed: true, dataTransfer: dt }));
  }, el);
}

export async function html5DragOver(page: Page, target: Locator): Promise<void> {
  const el = await target.elementHandle();
  if (!el) throw new Error("drag over target missing");
  await page.evaluate((dst) => {
    const dt = (window as unknown as { __hocketLibraryDrag?: DataTransfer }).__hocketLibraryDrag;
    const r = dst.getBoundingClientRect();
    const clientX = r.left + r.width / 2;
    const clientY = r.top + Math.min(8, r.height / 2);
    const init: DragEventInit = { bubbles: true, cancelable: true, composed: true, dataTransfer: dt, clientX, clientY };
    document.dispatchEvent(new DragEvent("dragover", init));
    dst.dispatchEvent(new DragEvent("dragenter", init));
    dst.dispatchEvent(new DragEvent("dragover", init));
  }, el);
}

export async function html5Drop(page: Page, target: Locator): Promise<void> {
  const el = await target.elementHandle();
  if (!el) throw new Error("drop target missing");
  await page.evaluate((dst) => {
    const w = window as unknown as { __hocketLibraryDrag?: DataTransfer };
    const dt = w.__hocketLibraryDrag;
    const r = dst.getBoundingClientRect();
    const init: DragEventInit = { bubbles: true, cancelable: true, composed: true, dataTransfer: dt, clientX: r.left + r.width / 2, clientY: r.top + Math.min(8, r.height / 2) };
    dst.dispatchEvent(new DragEvent("drop", init));
    document.dispatchEvent(new DragEvent("dragend", init));
    delete w.__hocketLibraryDrag;
  }, el);
}

export async function html5DragEnd(page: Page): Promise<void> {
  await page.evaluate(() => {
    const w = window as unknown as { __hocketLibraryDrag?: DataTransfer };
    document.dispatchEvent(new DragEvent("dragend", { bubbles: true, cancelable: true, composed: true, dataTransfer: w.__hocketLibraryDrag }));
    delete w.__hocketLibraryDrag;
  });
}

/** Pick `value` in one of the app's Select comboboxes (components/controls.tsx) by pointer, like a user. */
export async function chooseOption(select: Locator, value: string): Promise<void> {
  await select.click();
  await select.page().locator(`[role="listbox"] [role="option"][data-value="${value}"]`).click();
  await expect(select).toHaveAttribute("aria-expanded", "false");
  await expect(select).toHaveAttribute("data-value", value);
}
