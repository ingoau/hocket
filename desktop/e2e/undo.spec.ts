import { completeSetup, expect, playFirstAlbum, test } from "./fixtures";

test.describe("undo", () => {
  test("a queue mutation shows a toast with the single Undo action and Ctrl+Z undoes it", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playFirstAlbum(page);
    await expect(page.getByTestId("queue-row-current")).toHaveCount(1);
    await page.getByTestId("shuffle").click();
    const toast = page.getByTestId("toast").last();
    await expect(toast).toContainText("Shuffle on");
    await expect(toast.getByTestId("toast-action")).toHaveText("Undo");
    await expect(page.getByTestId("shuffle")).toHaveAttribute("aria-pressed", "true");
    await toast.getByTestId("toast-action").click();
    await expect(page.getByTestId("shuffle")).toHaveAttribute("aria-pressed", "false");
    await expect(page.getByTestId("toast").last()).toContainText("Undone");
    // Keyboard redo/undo.
    await page.getByTestId("content").click();
    await page.keyboard.press("Control+Shift+z");
    await expect(page.getByTestId("shuffle")).toHaveAttribute("aria-pressed", "true");
    await page.keyboard.press("Control+z");
    await expect(page.getByTestId("shuffle")).toHaveAttribute("aria-pressed", "false");
  });

  test("Ctrl+Z inside a text field belongs to the field", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playFirstAlbum(page);
    await page.getByTestId("shuffle").click();
    await expect(page.getByTestId("shuffle")).toHaveAttribute("aria-pressed", "true");
    const input = page.getByTestId("search-input");
    await input.click();
    await page.keyboard.type("abc");
    await page.keyboard.press("Control+z");
    await expect(page.getByTestId("shuffle")).toHaveAttribute("aria-pressed", "true");
  });
});
