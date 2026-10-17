import { completeSetup, expect, playFirstAlbum, test, openPlace } from "./fixtures";

test.describe("keyboard", () => {
  test("space toggles play, Q toggles the queue, shortcuts stay out of text fields", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playFirstAlbum(page);
    const play = page.getByTestId("play-pause");
    await expect(play).toHaveAttribute("aria-label", "Pause");
    // Focus the content pane rather than clicking its centre (that lands on whatever the layout
    // puts there, e.g. the album's own rating).
    await page.getByTestId("content").focus();
    await page.keyboard.press("Space");
    await expect(play).toHaveAttribute("aria-label", "Play");
    await page.keyboard.press("Space");
    await expect(play).toHaveAttribute("aria-label", "Pause");
    // Q closes the side panel while it shows the queue; again re-opens it on the queue.
    await page.keyboard.press("q");
    await expect(page.getByTestId("right-panel")).toHaveCount(0);
    await page.keyboard.press("q");
    await expect(page.getByTestId("right-panel")).toHaveAttribute("data-view", "queue");
    // Typing in search must not trigger single-key shortcuts.
    await page.getByTestId("search-input").click();
    await page.keyboard.type("q f");
    await expect(page.getByTestId("search-input")).toHaveValue("q f");
    await expect(page.getByTestId("right-panel")).toHaveAttribute("data-view", "queue");
    await expect(page.getByTestId("fullscreen-player")).toHaveCount(0);
    await page.keyboard.press("Escape");
    // Shift+Right goes to the next track.
    await page.getByTestId("content").focus();
    await page.keyboard.press("Shift+ArrowRight");
    await expect(page.getByTestId("queue-row-history")).toHaveCount(1);
  });

  test("list keyboard navigation and select-all as a predicate", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await openPlace(page, "songs");
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

  test("a shortcut pressed while a context menu is open acts on the menu's item", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playFirstAlbum(page);
    await openPlace(page, "songs");
    // Right-click a song that isn't the one playing; Ctrl+L loves it, and only it.
    const row = page.getByTestId("track-row").nth(4);
    await row.click({ button: "right" });
    const menu = page.getByTestId("context-menu");
    await expect(menu.locator('[data-action="love"]')).toBeVisible();
    await page.keyboard.press("Control+l");
    await expect(menu).toHaveCount(0);
    await row.click({ button: "right" });
    await expect(menu.locator('[data-action="unlove"]')).toBeVisible();
    // The same chord unloves it again from the menu.
    await page.keyboard.press("Control+l");
    await expect(menu).toHaveCount(0);
    await row.click({ button: "right" });
    await expect(menu.locator('[data-action="love"]')).toBeVisible();
    // A digit rates the menu's song.
    await page.keyboard.press("4");
    await expect(menu).toHaveCount(0);
    await expect(row.locator('[data-star="4"]')).toHaveAttribute("aria-checked", "true");
  });
});
