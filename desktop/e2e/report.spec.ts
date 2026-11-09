// Save to Coordinator (docs/reports.md) against the fake core: the button only once the
// coordinator has said this account may (never over the LAN), the dialog (note, picked and pasted
// screenshots, a recording, cancel deleting what it staged), and the upload bar across the top:
// progress, the report's id with Copy ID, a failed upload with Retry. axe over the dialog and bar.
//
//   pnpm build && xvfb-run -a -s "-screen 0 2000x1600x24" pnpm test:e2e report

import { mkdirSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { ElectronApplication, Page } from "@playwright/test";
import { expectNoViolations } from "./a11y-helpers";
import { completeSetup, expect, resizeWindow, test } from "./fixtures";

const out = process.env.HOCKET_SCREENSHOT_DIR;
const mod = process.platform === "darwin" ? "Meta" : "Control";
/** A 4×4 opaque PNG. */
const PNG = Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAQAAAAECAIAAAAmkwkpAAAAEElEQVR4nGMwaLgARwzEcQBmUhgBRJ7megAAAABJRU5ErkJggg==", "base64");

/** Every open dialog in main answers `paths`. */
async function stubOpen(app: ElectronApplication, paths: string[]): Promise<void> {
  await app.evaluate(({ dialog }, filePaths) => {
    dialog.showOpenDialog = (async () => ({ canceled: false, filePaths })) as typeof dialog.showOpenDialog;
  }, paths);
}

function staged(userData: string): string[] {
  try {
    return readdirSync(join(userData, "report-staging"));
  } catch {
    return [];
  }
}

async function openDiagnostics(page: Page): Promise<void> {
  await page.getByTestId("nav-settings").click();
  await page.getByTestId("settings-nav-advanced").click();
  const toggle = page.getByTestId("setting-developer.mode");
  if (!(await toggle.isChecked())) await toggle.click();
  await page.getByTestId("nav-home").click();
  await page.keyboard.press(`${mod}+Shift+D`);
  await expect(page.getByTestId("view-developer")).toBeVisible();
}

test("Save to Coordinator: offered via the coordinator only; the dialog stages files; the bar follows the upload", async ({ hocket }) => {
  const { app, page, userData } = hocket;
  await completeSetup(page);
  await resizeWindow(app, page, 1280, 820);
  const dir = mkdtempSync(join(tmpdir(), "hocket-report-"));
  try {
    const shot = join(dir, "shot.png");
    const video = join(dir, "screen.webm");
    writeFileSync(shot, PNG);
    writeFileSync(video, Buffer.alloc(3 * 1024 * 1024, 1));

    await openDiagnostics(page);
    await expect(page.getByTestId("developer-save")).toHaveText(/Save to file/);
    // Over the LAN there is no coordinator to save to.
    await expect(page.getByTestId("developer-room-status")).toContainText("Bundles from 1 of 1");
    await expect(page.getByTestId("developer-save-coordinator")).toHaveCount(0);

    await page.evaluate(() => window.hocket.dispatch({ type: "connectCoordinator" }));
    await page.getByTestId("developer-refresh").click();
    await expect(page.getByTestId("developer-source-coordinator")).toHaveAttribute("data-state", "received");
    const button = page.getByTestId("developer-save-coordinator");
    await expect(button).toBeVisible();
    await expect(button).toBeEnabled();

    // Cancelling deletes what the dialog staged. A WebP (main can't decode one) goes through the page.
    const webp = join(dir, "shot.webp");
    const webpUrl = await page.evaluate(() => {
      const c = document.createElement("canvas");
      c.width = 6;
      c.height = 4;
      c.getContext("2d")!.fillRect(0, 0, 6, 4);
      return c.toDataURL("image/webp");
    });
    expect(webpUrl).toMatch(/^data:image\/webp;base64,/);
    writeFileSync(webp, Buffer.from(webpUrl.split(",")[1]!, "base64"));
    await button.click();
    await expect(page.getByTestId("dialog-report")).toBeVisible();
    await expect(page.getByTestId("report-note")).toBeFocused();
    await stubOpen(app, [shot, webp]);
    await page.getByTestId("report-add-screenshot").click();
    await expect(page.getByTestId("report-screenshot")).toHaveCount(2);
    await expect(page.getByTestId("report-screenshot").nth(1).locator("img")).toHaveAttribute("alt", "shot.webp");
    expect(staged(userData)).toHaveLength(2);
    await stubOpen(app, [shot]);
    await page.getByTestId("report-cancel").click();
    await expect(page.getByTestId("dialog-report")).toHaveCount(0);
    await expect.poll(() => staged(userData).length).toBe(0);

    // A note, a picked screenshot, a pasted one (one removed again) and a recording.
    await button.click();
    await page.getByTestId("report-note").fill("Queue jumped back two songs after unlocking");
    await page.getByTestId("report-add-screenshot").click();
    await expect(page.getByTestId("report-screenshot")).toHaveCount(1);
    await app.evaluate(({ clipboard, ClipboardItem }, b64) => clipboard.write([new ClipboardItem({ "image/png": new Blob([Buffer.from(b64, "base64")], { type: "image/png" }) })]), PNG.toString("base64"));
    await page.getByTestId("report-note").focus();
    await page.keyboard.press(`${mod}+V`);
    await expect(page.getByTestId("report-screenshot")).toHaveCount(2);
    await expect(page.getByTestId("report-screenshot").nth(1).locator("img")).toHaveAttribute("alt", "Pasted image");
    await expect(page.getByTestId("report-note")).toHaveValue("Queue jumped back two songs after unlocking");
    expect(staged(userData)).toHaveLength(2);
    await page.getByTestId("report-screenshot").nth(1).getByTestId("report-remove-screenshot").click();
    await expect(page.getByTestId("report-screenshot")).toHaveCount(1);
    await expect.poll(() => staged(userData).length).toBe(1);
    await stubOpen(app, [video]);
    await page.getByTestId("report-add-recording").click();
    await expect(page.getByTestId("report-recording")).toContainText("screen.webm");
    await expect(page.getByTestId("report-recording")).toContainText("3");
    await expectNoViolations(page, "report dialog");
    if (out) {
      mkdirSync(out, { recursive: true });
      await page.screenshot({ path: join(out, "desktop-report-dialog.png") });
    }

    // Send closes the dialog at once; the bar follows the upload.
    await page.getByTestId("report-send").click();
    await expect(page.getByTestId("dialog-report")).toHaveCount(0);
    const bar = page.getByTestId("report-upload-bar");
    await expect(bar).toHaveAttribute("data-state", "uploading");
    await expect(page.getByTestId("report-upload-text")).toContainText(/Uploading report… \d+%/);
    await expect(page.getByTestId("report-upload-progress")).toHaveAttribute("aria-valuenow", /\d+/);
    if (out) await page.screenshot({ path: join(out, "desktop-report-uploading.png") });
    await expect(page.getByTestId("report-upload-live")).toHaveAttribute("role", "status");
    // Not offered again while one goes up.
    await expect(button).toHaveCount(0);
    await expect(bar).toHaveAttribute("data-state", "done");
    await expect(page.getByTestId("report-upload-text")).toHaveText("Report R-20261101-test saved to the coordinator");
    await expect(button).toBeVisible();
    // The core deleted the staged copy once the coordinator had it; the recording is the user's.
    expect(staged(userData)).toHaveLength(0);
    expect(readdirSync(dir)).toContain("screen.webm");
    await expectNoViolations(page, "report upload bar");
    if (out) await page.screenshot({ path: join(out, "desktop-report-done.png") });
    await page.getByTestId("report-upload-copy").click();
    expect(await app.evaluate(({ clipboard }) => clipboard.readText())).toBe("R-20261101-test");
    // It goes by itself.
    await expect(bar).toHaveCount(0, { timeout: 12_000 });

    // A failed upload (the fake core fails a "fail:" note once) offers Retry and Dismiss.
    await button.click();
    await page.getByTestId("report-note").fill("fail: the network drops");
    await page.getByTestId("report-send").click();
    await expect(bar).toHaveAttribute("data-state", "failed");
    await expect(page.getByTestId("report-upload-text")).toHaveText("The report upload stopped: check your connection");
    await expect(page.getByTestId("report-upload-dismiss")).toBeVisible();
    if (out) await page.screenshot({ path: join(out, "desktop-report-failed.png") });
    await page.getByTestId("report-upload-retry").click();
    await expect(bar).toHaveAttribute("data-state", "done");
    await page.evaluate(() => window.hocket.dispatch({ type: "dismissReportUpload" }));
    await expect(bar).toHaveCount(0);

    // Cancel stops one that is still going.
    await button.click();
    await page.getByTestId("report-send").click();
    await expect(bar).toHaveAttribute("data-state", "uploading");
    await page.getByTestId("report-upload-cancel").click();
    await expect(bar).toHaveCount(0);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("Save to Coordinator: an upload already running when the window opens shows in the bar", async ({ hocket }) => {
  const { page } = hocket;
  await completeSetup(page);
  await page.evaluate(() => window.hocket.dispatch({ type: "startReportUpload", data: { note: "fail: resumed", bundle: "bundle", attachments: [] } }));
  await expect(page.getByTestId("report-upload-bar")).toHaveAttribute("data-state", "failed");
  // A reload is a fresh window: it asks the core where the upload stands.
  await page.reload();
  await expect(page.getByTestId("report-upload-bar")).toHaveAttribute("data-state", "failed", { timeout: 15_000 });
  await page.getByTestId("report-upload-dismiss").click();
  await expect(page.getByTestId("report-upload-bar")).toHaveCount(0);
});
