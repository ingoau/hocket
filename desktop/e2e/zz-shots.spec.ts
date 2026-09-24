// Before/after screenshots for the accessibility pass (not part of the suite).
import type { ElectronApplication, Page } from "@playwright/test";
import { completeSetup, expect, test } from "./fixtures";

const DIR = "/tmp/claude-0/-home-user-hocket/a7f78d95-ccdd-5704-8c05-2cd7592f7b5a/scratchpad/shots";
const TAG = process.env.SHOT_TAG ?? "after";
const shot = (page: Page, name: string) => page.screenshot({ path: `${DIR}/a11y-desktop-${TAG}-${name}.png` });
/** The whole window as the compositor draws it (page.screenshot crops to the CSS viewport under zoom). */
async function windowShot(app: ElectronApplication, name: string) {
  const b64 = await app.evaluate(async ({ BrowserWindow }) => {
    const w = BrowserWindow.getAllWindows().find((x) => x.getBounds().width > 100)!;
    return (await w.webContents.capturePage()).toPNG().toString("base64");
  });
  const { writeFileSync } = await import("node:fs");
  writeFileSync(`${DIR}/a11y-desktop-${TAG}-${name}.png`, Buffer.from(b64, "base64"));
}

async function zoom(app: ElectronApplication, f: number) {
  await app.evaluate(({ BrowserWindow }, f) => { for (const w of BrowserWindow.getAllWindows()) if (w.getBounds().width > 100) w.webContents.setZoomFactor(f); }, f);
}
async function playShowcase(page: Page) {
  const servers = await page.evaluate(async () => (await window.hocket.query({ type: "servers" })) as { type: string; data: { id: string }[] });
  const sid = servers.data[0]!.id;
  await page.evaluate((s) => window.hocket.dispatch({ type: "playTracks", data: { server_id: s, track_ids: ["tr-1"], start_index: 0, label: "e2e", shuffle: false } }), sid);
  await page.waitForTimeout(4500);
}

test.setTimeout(120_000);

test("focus rings", async ({ hocket }) => {
  const { page } = hocket;
  await completeSetup(page);
  await playShowcase(page);
  await page.getByTestId("nav-songs").click();
  await expect(page.getByTestId("track-row").first()).toBeVisible();
  await page.getByTestId("track-row").first().locator(".td").nth(1).click();
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowDown");
  await shot(page, "focus-table");
  await page.getByTestId("play-pause").focus();
  await page.keyboard.press("Shift+Tab");
  await page.keyboard.press("Tab");
  await shot(page, "focus-player");
});

test("reduced motion", async ({ hocket }) => {
  const { page } = hocket;
  await page.emulateMedia({ reducedMotion: "reduce" });
  await completeSetup(page);
  await playShowcase(page);
  await shot(page, "reduced-motion-panel");
  await page.getByTestId("content").click();
  await page.keyboard.press("f");
  await page.waitForTimeout(1500);
  await shot(page, "reduced-motion-fullscreen");
});

test("forced colours", async ({ hocket }) => {
  const { page } = hocket;
  await page.emulateMedia({ forcedColors: "active" });
  await completeSetup(page);
  await playShowcase(page);
  await page.getByTestId("nav-songs").click();
  await page.getByTestId("track-row").nth(1).locator(".td").nth(1).click();
  await page.keyboard.press("ArrowDown");
  await shot(page, "forced-colors");
  await page.keyboard.press("f");
  await page.waitForTimeout(1500);
  await shot(page, "forced-colors-fullscreen");
});

test("200% zoom", async ({ hocket }) => {
  const { app, page } = hocket;
  await completeSetup(page);
  await playShowcase(page);
  await zoom(app, 2);
  await page.waitForTimeout(800);
  await windowShot(app, "zoom-home");
  await page.getByTestId("nav-songs").click();
  await page.waitForTimeout(800);
  await windowShot(app, "zoom-songs");
  await page.getByTestId("nav-settings").click();
  await page.getByTestId("settings-nav-audio").click();
  await page.waitForTimeout(500);
  await windowShot(app, "zoom-settings");
  await page.keyboard.press("f");
  await page.waitForTimeout(1500);
  await windowShot(app, "zoom-fullscreen");
});
