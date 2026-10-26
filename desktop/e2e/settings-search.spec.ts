// Settings search: typing filters to matching settings, picking one opens its
// section with the setting highlighted and focused; Escape and the clear button
// empty the field; Mod+F focuses it on Settings; every setting in the index has
// its anchor on its section's page.
import { completeSetup, expect, test } from "./fixtures";
import { SETTINGS_INDEX } from "../src/renderer/lib/settings-index";

test.describe("settings search", () => {
  test.beforeEach(async ({ hocket }) => {
    await completeSetup(hocket.page);
    await hocket.page.getByTestId("nav-settings").click();
    await expect(hocket.page.getByTestId("view-settings")).toBeVisible();
  });

  test("filters to matching settings and jumps to the one picked", async ({ hocket }) => {
    const { page } = hocket;
    const field = page.getByTestId("settings-search");
    await field.fill("dark mode");
    // The sections and account card give way to the results.
    await expect(page.getByTestId("settings-account")).toHaveCount(0);
    await expect(page.getByTestId("settings-nav-audio")).toHaveCount(0);
    const results = page.getByTestId("settings-search-results");
    await expect(results.getByRole("button")).toHaveCount(1);
    await expect(page.getByTestId("settings-hit-theme")).toContainText("Appearance");
    await page.getByTestId("settings-hit-theme").click();
    await expect(field).toHaveValue("");
    await expect(page.getByTestId("settings-nav-appearance")).toHaveAttribute("aria-current", "page");
    await expect(page.locator("#setting-theme")).toHaveClass(/setting-flash/);
    await expect(page.getByTestId("setting-theme")).toBeFocused();
    await expect(page.locator("#setting-theme")).toBeInViewport();
  });

  test("keyboard: Enter picks the first result, arrows move through them, Escape clears", async ({ hocket }) => {
    const { page } = hocket;
    const field = page.getByTestId("settings-search");
    await field.fill("cache");
    await field.press("ArrowDown");
    await expect(page.getByTestId("settings-search-results").getByRole("button").first()).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(field).toHaveValue("");
    await expect(field).toBeFocused();
    await expect(page.getByTestId("settings-account")).toBeVisible();
    await field.fill("gapless");
    await field.press("Enter");
    await expect(page.getByTestId("settings-nav-audio")).toHaveAttribute("aria-current", "page");
    await expect(page.locator("#setting-gapless [role=switch], #setting-gapless input").first()).toBeFocused();
  });

  test("shows an empty state, and the clear button resets", async ({ hocket }) => {
    const { page } = hocket;
    await page.getByTestId("settings-search").fill("zzzz nothing");
    await expect(page.getByTestId("settings-search-empty")).toContainText("zzzz nothing");
    await page.getByTestId("settings-search-clear").click();
    await expect(page.getByTestId("settings-search")).toHaveValue("");
    await expect(page.getByTestId("settings-nav-general")).toBeVisible();
  });

  test("Mod+F focuses the settings search on Settings", async ({ hocket }) => {
    const { page } = hocket;
    await page.getByTestId("settings-nav-audio").click();
    await page.keyboard.press("ControlOrMeta+f");
    await expect(page.getByTestId("settings-search")).toBeFocused();
  });

  test("every indexed setting has its anchor on its section's page", async ({ hocket }) => {
    const { page } = hocket;
    const sections = [...new Set(SETTINGS_INDEX.map((e) => e.section))];
    for (const section of sections) {
      await page.getByTestId(`settings-nav-${section}`).click();
      await expect(page.getByTestId(`settings-nav-${section}`)).toHaveAttribute("aria-current", "page");
      for (const e of SETTINGS_INDEX.filter((x) => x.section === section)) {
        await expect(page.locator(`#setting-${e.id}`), `${section}: ${e.id}`).toHaveCount(1);
      }
    }
  });
});

// For review: the results for a query, and the setting a result jumps to. Only with an output directory:
//   pnpm build && HOCKET_SCREENSHOT_DIR=/tmp/shots xvfb-run -a pnpm test:e2e settings-search
test("screenshots: settings search", async ({ hocket }) => {
  const out = process.env.HOCKET_SCREENSHOT_DIR;
  test.skip(!out, "set HOCKET_SCREENSHOT_DIR to write screenshots");
  const { page } = hocket;
  await completeSetup(page);
  await page.getByTestId("nav-settings").click();
  await page.getByTestId("settings-search").fill("cache");
  await page.screenshot({ path: `${out}/desktop-settings-search.png` });
  await page.getByTestId("settings-hit-cacheBudget").click();
  await page.waitForTimeout(400);
  await page.screenshot({ path: `${out}/desktop-settings-search-jump.png` });
});
