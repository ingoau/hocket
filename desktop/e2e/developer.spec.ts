// Developer mode (Settings → Advanced) and the developer diagnostics page: the switch, the
// Mod+Shift+D shortcut it gates, what the bundle carries (the renderer's console lines and the
// core's diagnostics), saving it, and axe over the page in both themes. With HOCKET_SCREENSHOT_DIR
// it also writes screenshots of the Advanced section and the page.
//
//   pnpm build && HOCKET_SCREENSHOT_DIR=/tmp/shots xvfb-run -a -s "-screen 0 2000x1600x24" pnpm test:e2e developer

import { mkdirSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { Page } from "@playwright/test";
import { expectNoViolations, setTheme, settleAnimations } from "./a11y-helpers";
import { completeSetup, expect, resizeWindow, stubDialogs, test } from "./fixtures";

const out = process.env.HOCKET_SCREENSHOT_DIR;
const mod = process.platform === "darwin" ? "Meta" : "Control";

async function openAdvanced(page: Page): Promise<void> {
  await page.getByTestId("nav-settings").click();
  await page.getByTestId("settings-nav-advanced").click();
  await expect(page.getByTestId("settings-nav-advanced")).toHaveAttribute("aria-current", "page");
}

test("developer mode gates the shortcut; the page previews, saves and passes axe", async ({ hocket }) => {
  const { app, page, userData } = hocket;
  await completeSetup(page);
  await resizeWindow(app, page, 1280, 820);

  // Off by default: the shortcut does nothing, the page is still reachable from Advanced.
  await page.keyboard.press(`${mod}+Shift+D`);
  await page.waitForTimeout(200);
  await expect(page.getByTestId("view-developer")).toHaveCount(0);
  await openAdvanced(page);
  const toggle = page.getByTestId("setting-developer.mode");
  await expect(toggle).not.toBeChecked();
  await expect(page.getByText(/open developer diagnostics with Ctrl\+Shift\+D|open developer diagnostics with ⌘⇧D/)).toBeVisible();

  await toggle.click();
  await expect(toggle).toBeChecked();
  // A renderer line and a main line both reach the bundle.
  await page.evaluate(() => console.warn("e2e-renderer-marker"));
  await app.evaluate(() => console.error("e2e-main-marker"));
  await page.getByTestId("nav-home").click();
  await page.keyboard.press(`${mod}+Shift+D`);
  await expect(page.getByTestId("view-developer")).toBeVisible();
  await expect(page.getByTestId("developer-mode-switch")).toBeChecked();
  await expect(page.getByTestId("developer-head")).toContainText("Hocket diagnostics");
  await expect(page.getByTestId("developer-head")).toContainText("developer mode  on");
  await expect(page.getByTestId("developer-head")).toContainText("(fake core)");
  await expect(page.getByTestId("developer-log")).toContainText("e2e-renderer-marker");
  await expect(page.getByTestId("developer-log")).toContainText("ERROR main: e2e-main-marker");
  await expect(page.getByTestId("developer-stats")).toContainText("last 60 min");

  if (out) {
    mkdirSync(out, { recursive: true });
    for (const theme of ["light", "dark"] as const) {
      await setTheme(page, theme);
      await page.getByTestId("nav-settings").click();
      await page.getByTestId("settings-nav-advanced").click();
      await settleAnimations(page);
      await page.screenshot({ path: join(out, `desktop-${theme}-settings-advanced.png`) });
      await page.keyboard.press(`${mod}+Shift+D`);
      await expect(page.getByTestId("view-developer")).toBeVisible();
      await expect(page.getByTestId("developer-log")).toContainText("e2e-renderer-marker");
      await settleAnimations(page);
      await page.screenshot({ path: join(out, `desktop-${theme}-developer-diagnostics.png`) });
    }
  }

  for (const theme of ["light", "dark"] as const) {
    await setTheme(page, theme);
    await page.waitForTimeout(200);
    await expectNoViolations(page, `${theme} developer diagnostics`);
  }

  // Save writes the whole bundle where the dialog says.
  const dir = mkdtempSync(join(tmpdir(), "hocket-dev-save-"));
  try {
    const target = join(dir, "bundle.txt");
    await stubDialogs(app, target);
    await page.getByTestId("developer-save").click();
    await expect(page.getByText("Saved bundle.txt")).toBeVisible();
    const text = readFileSync(target, "utf8");
    expect(text).toMatch(/^Hocket diagnostics\n/);
    expect(text).toContain("e2e-renderer-marker");
    expect(text).toContain(userData.split("/").pop()!);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }

  // Turning it off from the page itself gates the shortcut again.
  await page.getByTestId("developer-mode-switch").click();
  await expect(page.getByTestId("developer-mode-card")).toContainText("Developer mode is off");
  await page.getByTestId("nav-home").click();
  await page.keyboard.press(`${mod}+Shift+D`);
  await page.waitForTimeout(200);
  await expect(page.getByTestId("view-developer")).toHaveCount(0);
});
