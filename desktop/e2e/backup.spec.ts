// Config backup round trip: an export with passwords resolved by main is
// enough to restore a second, fresh install straight from the setup screen.
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { completeSetup, expect, launchFake, stubDialogs, test, chooseOption } from "./fixtures";

test.describe("config backup", () => {
  test("export with server passwords → import on a fresh install signs in and restores settings", async () => {
    test.setTimeout(120_000);
    const first = mkdtempSync(join(tmpdir(), "hocket-backup-a-"));
    const second = mkdtempSync(join(tmpdir(), "hocket-backup-b-"));
    const file = join(first, "export", "hocket-config.json");
    mkdirSync(join(first, "export"));
    try {
      let { app, page } = await launchFake(first);
      try {
        await completeSetup(page);
        await page.getByTestId("nav-settings").click();
        await page.getByTestId("settings-nav-appearance").click();
        await chooseOption(page.getByTestId("setting-theme"), "dark");
        await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
        await page.getByTestId("settings-nav-backup").click();
        await stubDialogs(app, file);
        await page.getByTestId("export-secrets").check();
        await page.getByTestId("export-config").click();
        await expect.poll(() => existsSync(file)).toBe(true);
        const doc = JSON.parse(readFileSync(file, "utf8")) as { secrets?: Record<string, string>; servers: { id: string; url: string }[] };
        expect(doc.servers[0]?.url).toBe("https://music.example.org");
        // Main resolved the keystore reference to the password entered at setup.
        expect(doc.secrets).toEqual({ [`server:${doc.servers[0]!.id}:password`]: "secret" });
      } finally {
        await app.close();
      }

      ({ app, page } = await launchFake(second));
      try {
        await expect(page.getByTestId("setup")).toBeVisible();
        await stubDialogs(app, file);
        await page.getByTestId("setup-import").click();
        await expect(page.getByTestId("app")).toBeVisible({ timeout: 15_000 });
        await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
        await page.getByTestId("nav-settings").click();
        await expect(page.getByTestId("server-row")).toHaveCount(1);
        await expect(page.getByTestId("server-row")).toContainText("https://music.example.org");
      } finally {
        await app.close();
      }
    } finally {
      rmSync(first, { recursive: true, force: true });
      rmSync(second, { recursive: true, force: true });
    }
  });

  test("declining the plain-text warning exports nothing", async ({ hocket }) => {
    const { app, page, userData } = hocket;
    await completeSetup(page);
    await page.getByTestId("nav-settings").click();
    await page.getByTestId("settings-nav-backup").click();
    const file = join(userData, "declined.json");
    await stubDialogs(app, file, 1);
    await page.getByTestId("export-secrets").check();
    await page.getByTestId("export-config").click();
    await page.waitForTimeout(800);
    expect(() => readFileSync(file)).toThrow();
    await expect(page.getByTestId("toast")).toHaveCount(0);
  });
});
