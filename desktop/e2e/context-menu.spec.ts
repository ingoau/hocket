// The context menu and its Add to playlist submenu (docs: the final menu boards): a song's menu
// from a playlist, ticking and unticking playlists, a playlist holding the song twice (×2, Remove
// one), adding another copy, searching and creating a playlist from the search; and the menu for
// several songs (mixed Love, counts, rating them all), each change with a toast and Undo.
import type { Page } from "@playwright/test";
import { completeSetup, expect, openPlace, test } from "./fixtures";

/** Opens a playlist from the Playlists page by name. */
async function openPlaylist(page: Page, name: string): Promise<void> {
  await openPlace(page, "playlists");
  await page.getByTestId("playlists-grid").getByTestId("grid-tile").filter({ hasText: name }).first().dblclick();
  await expect(page.getByTestId("view-playlist")).toBeVisible();
  await expect(page.getByTestId("track-row").first()).toBeVisible();
}

/** Clicks a row on its title, clear of the play button and the artist and album links. */
const onTitle = { x: 130, y: 12 };

test.describe("context menu", () => {
  test("a song's menu and its add to playlist submenu", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    // "Slow Sundays" holds its first song twice (fake core).
    await openPlaylist(page, "Slow Sundays");
    const menu = page.getByTestId("context-menu");
    await page.getByTestId("track-row").first().click({ position: onTitle, button: "right" });
    await expect(menu).toBeVisible();
    // The play strip: icon-only, named by tooltip and label.
    for (const name of ["Play", "Shuffle", "Play next", "Play later"]) {
      await expect(menu.getByRole("menuitem", { name, exact: true })).toHaveAttribute("title", name);
    }
    await expect(menu.getByRole("group", { name: "Rating" }).getByRole("menuitemradio")).toHaveCount(5);
    await expect(menu.getByRole("menuitemcheckbox", { name: /Love/ })).toHaveAttribute("aria-checked", "false");
    await expect(menu.locator('[data-action="removeFromPlaylist"]')).toHaveText(/^Remove from Slow SundaysDel$/);

    // Hovering Add to playlist opens the submenu, its search focused.
    await menu.locator('[data-action="addToPlaylist"]').hover();
    const sub = page.getByTestId("playlist-submenu");
    await expect(sub).toBeVisible();
    await expect(page.getByTestId("playlist-search")).toBeFocused();
    const row = (name: string) => sub.getByTestId("playlist-row").filter({ hasText: name });
    const toast = (text: string) => page.getByTestId("toast").filter({ hasText: text });
    await expect(row("Slow Sundays")).toHaveAttribute("aria-checked", "true");
    await expect(row("Slow Sundays")).toContainText("×2");

    // Held twice: a click asks; Remove one leaves one copy, and Undo brings the second back.
    await row("Slow Sundays").click();
    await expect(page.getByTestId("playlist-ask")).toBeVisible();
    await expect(page.getByTestId("playlist-remove-all")).toHaveText("Remove both");
    await page.getByTestId("playlist-remove-one").click();
    await expect(toast("Removed one from Slow Sundays")).toBeVisible();
    await expect(row("Slow Sundays")).toHaveAttribute("aria-checked", "true");
    await expect(row("Slow Sundays")).not.toContainText("×2");
    await toast("Removed one from Slow Sundays").getByTestId("toast-action").click();
    await expect(row("Slow Sundays")).toContainText("×2");

    // A plain click ticks and unticks, each with a toast.
    await row("Party starters").click();
    await expect(row("Party starters")).toHaveAttribute("aria-checked", "true");
    await expect(toast("Added to Party starters")).toBeVisible();
    await row("Party starters").click();
    await expect(row("Party starters")).toHaveAttribute("aria-checked", "false");
    await expect(toast("Removed from Party starters")).toBeVisible();

    // The hover + (and Alt+click) adds another copy; the row doesn't move.
    const before = await row("Running").boundingBox();
    await row("Running").hover();
    const plus = sub.getByTestId("playlist-add-copy").and(sub.getByRole("menuitem", { name: "Add another copy to Running" }));
    await expect(plus).toHaveCSS("opacity", "1");
    await plus.click();
    await expect(toast("Added to Running")).toBeVisible();
    await row("Running").click({ modifiers: ["Alt"] });
    await expect(toast("Added again to Running")).toBeVisible();
    await expect(row("Running")).toContainText("×2");
    expect(await row("Running").boundingBox()).toEqual(before);

    // The keyboard: ↓ through the rows, Shift+Enter adds a copy, Enter on a
    // row holding several asks, ↓ and Enter pick Remove one.
    const search = page.getByTestId("playlist-search");
    const moveTo = async (name: string) => {
      for (let i = 0; i < 20; i++) {
        const id = await search.getAttribute("aria-activedescendant");
        if (id && (await page.locator(`#${id}`).textContent())?.startsWith(name)) return;
        await search.press("ArrowDown");
      }
      throw new Error(`no row ${name}`);
    };
    await moveTo("Party starters");
    await search.press("Shift+Enter");
    await expect(toast("Added to Party starters").last()).toBeVisible();
    await expect(row("Party starters")).toHaveAttribute("aria-checked", "true");
    await moveTo("Running");
    await search.press("Enter");
    await expect(page.getByTestId("playlist-ask")).toBeVisible();
    await expect(search).toHaveAttribute("aria-activedescendant", "pp-ask-0");
    await search.press("ArrowDown");
    await search.press("Enter");
    await expect(toast("Removed one from Running")).toBeVisible();
    await expect(row("Running")).not.toContainText("×2");

    // Searching: one Matches group, New playlist names the search, and no matches says so.
    await search.fill("part");
    await expect(sub.getByText("Matches", { exact: true })).toBeVisible();
    await expect(sub.getByTestId("playlist-row")).toHaveCount(1);
    await expect(page.getByTestId("playlist-new")).toHaveText('New playlist "part"');
    await search.fill("zzz");
    await expect(sub.getByRole("status")).toHaveText('No playlists match "zzz"');
    // New playlist from the search creates it with the song in it.
    await search.fill("Rainy days");
    await page.getByTestId("playlist-new").click();
    await expect(toast("Added to Rainy days")).toBeVisible();
    await expect(search).toHaveValue("");
    await expect(row("Rainy days")).toHaveAttribute("aria-checked", "true");

    // Escape goes back to the menu, a second one closes it.
    await search.press("Escape");
    await expect(sub).toHaveCount(0);
    await expect(menu).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(menu).toHaveCount(0);
  });

  test("the menu for several songs", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await openPlaylist(page, "Slow Sundays");
    // Songs 2–4 of the playlist; two of the three loved.
    const ids = await page.evaluate(async () => {
      const r = (await window.hocket.query({ type: "playlistTracks", data: { id: "pl-5", page: { offset: 1, limit: 3 } } })) as { data: { items: { id: string }[] } };
      return r.data.items.map((x) => x.id);
    });
    await page.evaluate((ids) => {
      for (const [i, id] of ids.entries()) window.hocket.dispatch({ type: "setLoved", data: { targets: [{ type: "track", data: { id } }], loved: i < 2 } });
      for (const id of ids) window.hocket.dispatch({ type: "setRating", data: { targets: [{ type: "track", data: { id } }], rating: 0 } });
    }, ids);
    const rows = page.getByTestId("track-row");
    await rows.nth(1).click({ position: onTitle });
    await rows.nth(3).click({ position: onTitle, modifiers: ["Shift"] });
    await rows.nth(2).click({ position: onTitle, button: "right" });
    const menu = page.getByTestId("context-menu");
    await expect(menu).toBeVisible();
    await expect(menu).toHaveAttribute("aria-label", /^3 songs · /);
    await expect(menu.getByRole("group", { name: "Play 3 songs" })).toBeVisible();
    const love = menu.getByRole("menuitemcheckbox", { name: /Love/ });
    await expect(love).toHaveAttribute("aria-checked", "mixed");
    await expect(love).toContainText("2 of 3");
    await expect(menu.locator('[data-action="download"]')).toContainText("Download 3 songs");
    await expect(menu.locator('[data-action="removeFromPlaylist"]')).toContainText("Remove 3 from Slow Sundays");
    await expect(menu.getByRole("menuitemradio", { name: "Rate all 3 songs 4 stars" })).toBeVisible();
    // Love on the mixed state loves the rest; the toast's Undo takes it back.
    await love.click();
    const toast = (text: string) => page.getByTestId("toast").filter({ hasText: text });
    await expect(toast("Loved 3 songs")).toBeVisible();
    await rows.nth(2).click({ position: onTitle, button: "right" });
    await expect(love).toHaveAttribute("aria-checked", "true");
    await expect(love).toContainText("Loved");
    // A digit rates them all, from the menu.
    await page.keyboard.press("3");
    await expect(menu).toHaveCount(0);
    await expect(toast("Rated 3 songs 3 stars")).toBeVisible();
    await rows.nth(2).click({ position: onTitle, button: "right" });
    await expect(menu.getByRole("menuitemradio", { name: "Rate all 3 songs 3 stars" })).toHaveAttribute("aria-checked", "true");
    await page.keyboard.press("Escape");
    await toast("Rated 3 songs 3 stars").getByTestId("toast-action").click();
    await rows.nth(2).click({ position: onTitle, button: "right" });
    await expect(menu.getByRole("menuitemradio", { name: "Rate all 3 songs 3 stars" })).toHaveAttribute("aria-checked", "false");
    // Different ratings read as Mixed, with the stars dashed and none checked.
    await page.keyboard.press("Escape");
    await page.evaluate((id) => window.hocket.dispatch({ type: "setRating", data: { targets: [{ type: "track", data: { id } }], rating: 5 } }), ids[0]!);
    await rows.nth(2).click({ position: onTitle, button: "right" });
    await expect(menu.getByRole("group", { name: /Rating/ })).toContainText("Mixed");
    await expect(menu.locator('[role="menuitemradio"][aria-checked="true"]')).toHaveCount(0);
    await page.keyboard.press("Escape");
  });
});
