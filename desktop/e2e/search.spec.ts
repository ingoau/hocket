import { completeSetup, expect, test } from "./fixtures";

test.describe("search", () => {
  test("the results page narrows by kind, and recent searches stay on this device", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    const input = page.getByTestId("search-input");
    // Nothing searched yet: focusing the empty box shows nothing.
    await input.click();
    await expect(page.getByTestId("search-popover")).toHaveCount(0);
    await input.fill("the");
    await page.keyboard.press("Enter");
    const view = page.getByTestId("view-search");
    await expect(view).toBeVisible();
    await expect(view.getByTestId("search-kind-all")).toHaveAttribute("aria-pressed", "true");
    // Under All, at most five songs, with a link to the rest.
    await expect(view.getByTestId("search-see-all-songs")).toBeVisible();
    await view.getByTestId("search-see-all-songs").click();
    await expect(view.getByTestId("search-kind-songs")).toHaveAttribute("aria-pressed", "true");
    await expect(view.locator("#search-albums")).toHaveCount(0);
    await view.getByTestId("search-kind-albums").click();
    await expect(view.locator("#search-albums")).toBeVisible();
    await expect(view.locator("#search-tracks")).toHaveCount(0);

    // Back in the box with it empty: the recent search, which reruns on a click.
    await input.fill("");
    await input.blur();
    await input.click();
    const recent = page.getByTestId("search-recent");
    await expect(recent).toHaveCount(1);
    await expect(recent).toHaveText("the");
    await page.keyboard.press("Escape");
    await input.fill("lights");
    await page.keyboard.press("Enter");
    await expect(page.getByTestId("view-search").locator("h1")).toHaveText("lights");
    await input.fill("");
    await input.click();
    await expect(recent).toHaveCount(2);
    await expect(recent.first()).toHaveText("lights");
    // The arrows and Enter pick one.
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("Enter");
    await expect(page.getByTestId("view-search").locator("h1")).toHaveText("the");
    // One can be forgotten, or all of them cleared.
    await input.fill("");
    await input.click();
    await recent.filter({ hasText: "lights" }).hover();
    await recent.filter({ hasText: "lights" }).getByTestId("search-recent-forget").click();
    await expect(recent).toHaveCount(1);
    await page.getByTestId("search-recent-clear").click();
    await expect(page.getByTestId("search-popover")).toHaveCount(0);
  });
});
