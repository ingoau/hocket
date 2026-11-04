import { completeSetup, expect, test } from "./fixtures";

test.describe("command palette", () => {
  test("Ctrl+K lists library results before actions and runs an action", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await page.getByTestId("content").click();
    await page.keyboard.press("Control+k");
    const palette = page.getByTestId("palette");
    await expect(palette).toBeVisible();
    await page.getByTestId("palette-input").fill("s");
    await expect(page.getByTestId("palette-item").first()).toBeVisible();
    await expect(palette.locator(".group", { hasText: "Actions" })).toBeVisible();
    // Results group appears before the Actions group.
    const groups = await palette.locator(".group").allTextContents();
    expect(groups.indexOf("Results")).toBeLessThan(groups.indexOf("Actions"));
    await page.getByTestId("palette-input").fill("settings");
    await page.keyboard.press("Enter");
    await expect(page.getByTestId("view-settings")).toBeVisible();
    await expect(palette).toHaveCount(0);
  });

  test("a library result opens its context menu from the pointer and the keyboard", async ({ hocket }) => {
    // R-20261104-9rrx: right-clicking a result in the palette did nothing.
    const { page } = hocket;
    await completeSetup(page);
    await page.getByTestId("content").click();
    await page.keyboard.press("Control+k");
    const palette = page.getByTestId("palette");
    const input = page.getByTestId("palette-input");
    await input.fill("s");
    const results = palette.locator('[data-testid="palette-item"]', { has: page.locator(".art") });
    await expect(results.first()).toBeVisible();
    const menu = page.getByTestId("context-menu");
    await results.first().click({ button: "right" });
    await expect(menu).toBeVisible();
    await expect(menu.getByRole("menuitem").first()).toBeVisible();
    // Escape closes the menu only; focus goes back to the palette.
    await page.keyboard.press("Escape");
    await expect(menu).toHaveCount(0);
    await expect(palette).toBeVisible();
    await expect(input).toBeFocused();
    // Shift+F10 in the input opens the active result's menu.
    await page.keyboard.press("Shift+F10");
    await expect(menu).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(input).toBeFocused();
    // A menu action that navigates takes the palette away with it.
    const track = palette.locator('[data-testid="palette-item"]', { has: page.locator(".art") }).first();
    await track.click({ button: "right" });
    const goTo = menu.getByRole("menuitem", { name: /^Go to (album|artist)$/ }).first();
    await goTo.click();
    await expect(palette).toHaveCount(0);
  });
});
