import { completeSetup, expect, playFirstAlbum, test } from "./fixtures";

test.describe("undo", () => {
  test("replacing the queue shows a toast with the single Undo action; other mutations stay quiet", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playFirstAlbum(page);
    await expect(page.getByTestId("queue-row-current")).toHaveCount(1);
    const toast = page.getByTestId("toast").last();
    await expect(toast).toContainText("Play");
    await expect(toast.getByTestId("toast-action")).toHaveText("Undo");
    await toast.getByTestId("toast-action").click();
    await expect(page.getByTestId("queue-row-current")).toHaveCount(0);
    await expect(page.getByTestId("toast").last()).toContainText("Undone");
    await page.getByTestId("toast-action").last().click();
    await expect(page.getByTestId("queue-row-current")).toHaveCount(1);
    // Shuffle leaves the queue's contents alone: no toast, but still undoable.
    await page.getByTestId("shuffle").click();
    await expect(page.getByTestId("shuffle")).toHaveAttribute("aria-pressed", "true");
    await expect(page.getByTestId("toasts")).not.toContainText("Shuffle");
    // Keyboard undo/redo.
    await page.getByTestId("content").click();
    await page.keyboard.press("Control+z");
    await expect(page.getByTestId("shuffle")).toHaveAttribute("aria-pressed", "false");
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
