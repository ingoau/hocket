// Renderer isolation: the page can't name filesystem paths, artwork is served
// by opaque token from the image cache only, and no window can navigate away.
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { completeSetup, expect, test } from "./fixtures";

test.describe("renderer isolation", () => {
  test("hocket-art:// serves only tokens main handed out; userData files are refused", async ({ hocket }) => {
    const { app, page, userData } = hocket;
    await completeSetup(page);
    await page.getByTestId("nav-albums").click();
    const img = page.locator("img.art").first();
    await expect(img).toBeVisible();
    await expect.poll(() => img.evaluate((el) => (el as HTMLImageElement).naturalWidth)).toBeGreaterThan(0);
    const src = (await img.getAttribute("src"))!;
    expect(src).toMatch(/^hocket-art:\/\/art\/[0-9a-f]{32}$/);

    const status = (url: string) => app.evaluate(async ({ net }, u) => (await net.fetch(u)).status, url);
    expect(await status(src)).toBe(200);
    // A path, even one that was valid under the old scheme, is not a token.
    writeFileSync(join(userData, "credentials.json"), "[]");
    expect(await status(`hocket-art://file/${encodeURIComponent(join(userData, "credentials.json"))}`)).toBe(403);
    expect(await status(`hocket-art://art/${encodeURIComponent(join(userData, "credentials.json"))}`)).toBe(404);
    expect(await status(`hocket-art://art/${"0".repeat(32)}`)).toBe(404);
    expect(await status(`hocket-art://art/..%2F..%2Fcredentials.json`)).toBe(404);

    // The bridge has no path-taking file helpers at all.
    const dialogKeys = await page.evaluate(() => Object.keys(window.hocket.dialog).sort());
    expect(dialogKeys).toEqual(["open" + "Text", "save", "saveText"].sort());
    // And the page can't fetch artwork bytes (connect-src no longer lists the scheme).
    const fetched = await page.evaluate(async (u) => { try { await fetch(u); return "ok"; } catch { return "blocked"; } }, src);
    expect(fetched).toBe("blocked");
  });

  test("the mini player cannot be navigated away from the app", async ({ hocket }) => {
    const { app, page } = hocket;
    await completeSetup(page);
    await page.getByTestId("content").click();
    await page.keyboard.press("m");
    const mini = await app.waitForEvent("window", { predicate: (w) => w.url().includes("window=mini") });
    await mini.waitForLoadState("domcontentloaded");
    const before = mini.url();
    await mini.evaluate(() => { location.assign("https://example.com/"); });
    await mini.waitForTimeout(500);
    expect(mini.url()).toBe(before);
    expect(app.windows().every((w) => w.url().startsWith("app://"))).toBe(true);
  });

  test("the credential warning tracks whether the OS keystore is usable", async ({ hocket }) => {
    const { app, page } = hocket;
    const backend = await app.evaluate(({ safeStorage }) => (process.platform === "linux" ? safeStorage.getSelectedStorageBackend() : "os"));
    const warning = page.getByTestId("credential-warning");
    if (backend === "basic_text" || backend === "unknown") await expect(warning).toBeVisible();
    else await expect(warning).toHaveCount(0);
  });
});
