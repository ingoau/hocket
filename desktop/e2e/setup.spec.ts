import { completeSetup, expect, test } from "./fixtures";

test.describe("first run", () => {
  test("server setup is the entire first screen and leads to the library", async ({ hocket }) => {
    const { page } = hocket;
    await expect(page.getByTestId("setup")).toBeVisible();
    await expect(page.getByTestId("sidebar")).toHaveCount(0);
    await expect(page.getByTestId("player-bar")).toHaveCount(0);
    // Validation names the fix.
    await page.getByTestId("setup-connect").click();
    await expect(page.getByTestId("setup-error")).toBeVisible();
    await completeSetup(page);
    await expect(page.getByTestId("dev-banner")).toBeVisible();
    await expect(page.getByTestId("sidebar")).toBeVisible();
    await expect(page.getByTestId("player-bar")).toBeVisible();
    // Library sync shows up as activity, not a blocking screen.
    await page.getByTestId("jobs-button").click();
    await expect(page.getByTestId("jobs-popover")).toBeVisible();
    await expect(page.getByTestId("job-row").first()).toBeVisible();
  });

  test("a wrong password surfaces as an inline error", async ({ hocket }) => {
    const { page } = hocket;
    await page.getByTestId("setup-url").fill("https://music.example.org");
    await page.getByTestId("setup-username").fill("wrong");
    await page.getByTestId("setup-password").fill("x");
    await page.getByTestId("setup-connect").click();
    await expect(page.getByTestId("setup-error")).toContainText("Wrong username or password");
    await expect(page.getByTestId("setup")).toBeVisible();
  });
});
