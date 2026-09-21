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
});
