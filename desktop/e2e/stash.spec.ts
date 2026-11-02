// The song stash (#150): stash songs from the context menu and the keyboard, the chip that grows in
// beside the search with their covers and count, the popover's actions (the stash stays after
// queueing it), reordering by drag, removing with × (with Undo) and Clear (with Undo).
import { completeSetup, expect, html5DragOver, html5DragStart, html5Drop, openPlace, showSideView, test } from "./fixtures";

const onTitle = { x: 130, y: 12 };

test.describe("stash", () => {
  test("stash songs, act on them, and the stash stays until cleared", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    const chip = page.getByTestId("stash-chip");
    await expect(chip).toHaveCount(0);

    await openPlace(page, "albums");
    await page.getByTestId("grid-tile").first().dblclick();
    await expect(page.getByTestId("view-album")).toBeVisible();
    const rows = page.getByTestId("track-row");
    const titles = await rows.locator('[role="gridcell"].title').evaluateAll((els) => els.map((e) => e.getAttribute("title") ?? ""));

    // From the context menu: one song.
    await rows.nth(0).click({ position: onTitle, button: "right" });
    await page.getByTestId("context-menu").locator('[data-action="stash"]').click();
    await expect(page.getByTestId("toast").filter({ hasText: "Stashed 1 song" })).toBeVisible();
    await expect(chip).toBeVisible();
    await expect(page.getByTestId("stash-count")).toHaveText("1");

    // From the keyboard: Ctrl+S on a selection of two (the first is already in, so one more).
    await rows.nth(0).click({ position: onTitle });
    await rows.nth(2).click({ position: onTitle, modifiers: ["Control"] });
    await page.keyboard.press("Control+s");
    await expect(page.getByTestId("stash-count")).toHaveText("2");
    await expect(page.getByTestId("toast").filter({ hasText: "Stashed 1 song" }).last()).toBeVisible();

    // Stashing the same song again is a no-op.
    await rows.nth(2).click({ position: onTitle, button: "right" });
    await page.getByTestId("context-menu").locator('[data-action="stash"]').click();
    await expect(page.getByTestId("toast").filter({ hasText: "Already in the stash" })).toBeVisible();
    await expect(page.getByTestId("stash-count")).toHaveText("2");
    await rows.nth(1).click({ position: onTitle, button: "right" });
    await page.getByTestId("context-menu").locator('[data-action="stash"]').click();
    await expect(page.getByTestId("stash-count")).toHaveText("3");

    // The popover lists them in order.
    await chip.click();
    const popover = page.getByTestId("stash-popover");
    await expect(popover).toBeVisible();
    const stashRows = popover.getByTestId("stash-row");
    await expect(stashRows).toHaveCount(3);
    if (titles.length >= 3) {
      await expect(stashRows.nth(0)).toContainText(titles[0] ?? "");
      await expect(stashRows.nth(1)).toContainText(titles[2] ?? "");
    }

    // Add to queue: confirmed, and the stash is still there.
    await popover.getByTestId("stash-play-later").click();
    await expect(page.getByTestId("toast").filter({ hasText: "Added 3 songs to the queue" })).toBeVisible();
    await expect(stashRows).toHaveCount(3);
    await showSideView(page, "queue");
    await expect(page.getByTestId("right-panel")).toContainText(titles[1] ?? "");

    // Reorder: drag the last row to the top.
    await chip.click();
    await expect(popover).toBeVisible();
    const lastText = await stashRows.nth(2).locator(".stash-text > span").first().innerText();
    await html5DragStart(page, stashRows.nth(2));
    await html5DragOver(page, stashRows.nth(0));
    await html5Drop(page, stashRows.nth(0));
    await expect(stashRows.nth(0)).toContainText(lastText);

    // Remove one with ×, then Undo.
    await stashRows.nth(0).hover();
    await stashRows.nth(0).getByTestId("stash-remove").click();
    await expect(stashRows).toHaveCount(2);
    const removed = page.getByTestId("toast").filter({ hasText: "Removed 1 song from the stash" });
    await removed.getByTestId("toast-action").click();
    await expect(stashRows).toHaveCount(3);
    await expect(stashRows.nth(0)).toContainText(lastText);

    // Clear: the chip goes; Undo brings it all back.
    await popover.getByTestId("stash-clear").click();
    await expect(chip).toHaveCount(0);
    const cleared = page.getByTestId("toast").filter({ hasText: "Stash cleared" });
    await cleared.getByTestId("toast-action").click();
    await expect(page.getByTestId("stash-count")).toHaveText("3");
  });

  test("Ctrl+Shift+S opens the stash and Escape closes it", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await openPlace(page, "albums");
    await page.getByTestId("grid-tile").first().dblclick();
    await page.getByTestId("track-row").first().click({ position: onTitle, button: "right" });
    await page.getByTestId("context-menu").locator('[data-action="stash"]').click();
    await expect(page.getByTestId("stash-chip")).toBeVisible();
    await page.getByTestId("content").focus();
    await page.keyboard.press("Control+Shift+S");
    await expect(page.getByTestId("stash-popover")).toBeVisible();
    await expect(page.getByTestId("stash-row").first()).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("stash-popover")).toHaveCount(0);
    await expect(page.getByTestId("stash-chip")).toBeFocused();
  });
});
