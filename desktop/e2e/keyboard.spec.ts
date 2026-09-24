import { completeSetup, expect, playFirstAlbum, test } from "./fixtures";

test.describe("keyboard", () => {
  test("space toggles play, Q toggles the queue, shortcuts stay out of text fields", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playFirstAlbum(page);
    const play = page.getByTestId("play-pause");
    await expect(play).toHaveAttribute("aria-label", "Pause");
    await page.getByTestId("content").click();
    await page.keyboard.press("Space");
    await expect(play).toHaveAttribute("aria-label", "Play");
    await page.keyboard.press("Space");
    await expect(play).toHaveAttribute("aria-label", "Pause");
    // Q collapses the queue pane; again re-opens it.
    await page.keyboard.press("q");
    await expect(page.getByTestId("queue-pane")).toHaveClass(/collapsed/);
    await page.keyboard.press("q");
    await expect(page.getByTestId("queue-pane")).not.toHaveClass(/collapsed/);
    // Typing in search must not trigger single-key shortcuts.
    await page.getByTestId("search-input").click();
    await page.keyboard.type("q f");
    await expect(page.getByTestId("search-input")).toHaveValue("q f");
    await expect(page.getByTestId("queue-pane")).not.toHaveClass(/collapsed/);
    await expect(page.getByTestId("fullscreen-player")).toHaveCount(0);
    await page.keyboard.press("Escape");
    // Shift+Right goes to the next track.
    await page.getByTestId("content").click();
    await page.keyboard.press("Shift+ArrowRight");
    await expect(page.getByTestId("queue-row-history")).toHaveCount(1);
  });

  test("list keyboard navigation and select-all as a predicate", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await page.getByTestId("nav-songs").click();
    // The table is an ARIA grid with a roving tab stop on its rows.
    const table = page.getByTestId("songs-table");
    await expect(table).toHaveAttribute("role", "grid");
    await expect(page.getByTestId("track-row").first()).toBeVisible();
    await page.getByTestId("track-row").first().focus();
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("ArrowDown");
    await expect(page.getByTestId("track-row").nth(2)).toHaveClass(/selected/);
    await page.keyboard.press("Shift+ArrowDown");
    await expect(page.locator('[data-testid="track-row"].selected')).toHaveCount(2);
    await page.keyboard.press("Control+a");
    const label = await table.getAttribute("aria-label");
    expect(label).toMatch(/\d{3,} selected/);
  });
});
