// The library lists: the sort popover (choose an order, flip the direction, by pointer and by keys),
// each list remembering its sort (leaving and coming back, and across a restart), the Artists grid in
// the chosen order, the Pinned and Recent groups over Playlists, and the Library without "Newest first".
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { Page } from "@playwright/test";
import { completeSetup, expect, launchFake, openPlace, resizeWindow, test } from "./fixtures";

async function serverId(page: Page): Promise<string> {
  const s = (await page.evaluate(() => window.hocket.query({ type: "servers" }))) as { data: { id: string }[] };
  return s.data[0]!.id;
}

/** The titles of the first [n] tiles of a grid, in order. */
async function gridTitles(page: Page, grid: string, n: number): Promise<string[]> {
  const tiles = page.getByTestId(grid).getByTestId("grid-tile");
  await expect(tiles.nth(n - 1)).toBeVisible();
  return (await tiles.evaluateAll((els) => els.map((e) => e.querySelector(".t1")?.textContent ?? ""))).slice(0, n);
}

test.describe("library lists", () => {
  test("the sort popover chooses an order and a direction, by pointer and by keys", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await openPlace(page, "albums");
    const button = page.getByTestId("albums-sort");
    // One tonal button names the order and its direction.
    await expect(button).toHaveText("Recently added");
    await expect(button).toHaveAttribute("aria-label", "Sort: Recently added, Newest");
    await button.click();
    const menu = page.getByTestId("albums-sort-menu");
    await expect(menu).toBeVisible();
    await expect(menu.getByText("Sort by")).toBeVisible();
    const options = menu.getByRole("menuitemradio");
    await expect(menu.getByRole("menuitemradio", { name: "Recently added" })).toHaveAttribute("aria-checked", "true");
    await expect(menu.getByRole("menuitemradio", { name: "Recently added" })).toBeFocused();
    await expect(options).toHaveCount(7 + 2);
    // Choosing applies at once; the popover stays for the direction.
    await menu.getByRole("menuitemradio", { name: "Title" }).click();
    await expect(button).toHaveAttribute("data-sort", "title");
    await expect(menu.getByRole("menuitemradio", { name: "Title" })).toHaveAttribute("aria-checked", "true");
    await expect(menu.getByRole("menuitemradio", { name: "Recently added" })).toHaveAttribute("aria-checked", "false");
    // The direction is worded for the order (A–Z for a title) and flips at once.
    await menu.getByRole("menuitemradio", { name: "A–Z" }).click();
    await expect(button).toHaveAttribute("data-descending", "false");
    await expect(button).toHaveAttribute("aria-label", "Sort: Title, A–Z");
    const sid = await serverId(page);
    const expected = (await page.evaluate((s) => window.hocket.query({ type: "albums", data: { server_id: s, sort: "title", descending: false, page: { offset: 0, limit: 4 } } }), sid)) as { data: { items: { name: string }[] } };
    await expect.poll(() => gridTitles(page, "albums-grid", 4)).toEqual(expected.data.items.map((a) => a.name));
    // Escape closes it and focus goes back to the button.
    await page.keyboard.press("Escape");
    await expect(menu).toHaveCount(0);
    await expect(button).toBeFocused();
    // By keys: Enter opens on the chosen order, the arrows move, Space chooses; down past the last order is the direction.
    await page.keyboard.press("Enter");
    await expect(menu.getByRole("menuitemradio", { name: "Title" })).toBeFocused();
    await page.keyboard.press("ArrowDown");
    await expect(menu.getByRole("menuitemradio", { name: "Artist" })).toBeFocused();
    await page.keyboard.press("Space");
    await expect(button).toHaveAttribute("data-sort", "artist");
    await page.keyboard.press("End");
    await expect(menu.getByTestId("sort-descending")).toBeFocused();
    await page.keyboard.press("ArrowLeft");
    await expect(menu.getByTestId("sort-ascending")).toBeFocused();
    await page.keyboard.press("ArrowRight");
    await page.keyboard.press("Enter");
    await expect(button).toHaveAttribute("data-descending", "true");
    await expect(button).toHaveAttribute("aria-label", "Sort: Artist, Z–A");
    await page.keyboard.press("ArrowUp");
    await expect(menu.getByRole("menuitemradio", { name: "Random" })).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(button).toBeFocused();
    // An outside click closes it too.
    await button.click();
    await expect(menu).toBeVisible();
    await page.getByTestId("view-albums").locator("h1").click();
    await expect(menu).toHaveCount(0);
  });

  test("the sort popover stays inside a short window, scrolling where it doesn't fit", async ({ hocket }) => {
    const { app, page } = hocket;
    await completeSetup(page);
    // The smallest window: Songs' nine orders fit neither below nor above the button.
    await resizeWindow(app, page, 900, 600);
    await openPlace(page, "songs");
    await page.getByTestId("songs-sort").click();
    const menu = page.getByTestId("songs-sort-menu");
    await expect(menu).toBeVisible();
    await expect.poll(async () => {
      const box = (await menu.boundingBox())!;
      const height = await page.evaluate(() => window.innerHeight);
      return box.y >= 0 && box.y + box.height <= height;
    }).toBe(true);
    // The direction, last in the menu, is still reachable.
    await page.keyboard.press("End");
    await expect(menu.getByTestId("sort-descending")).toBeFocused();
    await expect(menu.getByTestId("sort-descending")).toBeInViewport();
    // A click on the heading keeps the keys with the popover: Escape still closes it.
    await menu.getByText("Sort by").click();
    await page.keyboard.press("Escape");
    await expect(menu).toHaveCount(0);
    await expect(page.getByTestId("songs-sort")).toBeFocused();
  });

  test("each list remembers its sort, when reopened and after a restart", async () => {
    const userData = mkdtempSync(join(tmpdir(), "hocket-e2e-sort-"));
    try {
      let { app, page } = await launchFake(userData);
      await completeSetup(page);
      await openPlace(page, "playlists");
      await page.getByTestId("playlists-sort").click();
      await page.getByTestId("playlists-sort-menu").getByRole("menuitemradio", { name: "Songs" }).click();
      await page.getByTestId("sort-descending").click();
      await page.keyboard.press("Escape");
      await openPlace(page, "genres");
      await page.getByTestId("genres-sort").click();
      await page.getByTestId("genres-sort-menu").getByRole("menuitemradio", { name: "Albums" }).click();
      await page.keyboard.press("Escape");
      // Away and back: each list opens with its own sort.
      await page.getByTestId("nav-home").click();
      await openPlace(page, "playlists");
      await expect(page.getByTestId("playlists-sort")).toHaveAttribute("aria-label", "Sort: Songs, Most");
      await openPlace(page, "genres");
      await expect(page.getByTestId("genres-sort")).toHaveAttribute("aria-label", "Sort: Albums, Fewest");
      await openPlace(page, "artists");
      await expect(page.getByTestId("artists-sort")).toHaveAttribute("aria-label", "Sort: Name, A–Z");
      // The playlists show in that order.
      const sid = await serverId(page);
      const lists = (await page.evaluate((s) => window.hocket.query({ type: "playlists", data: { server_id: s } }), sid)) as { data: { name: string; songCount: number }[] };
      const bySongs = [...lists.data].sort((a, b) => b.songCount - a.songCount || a.name.localeCompare(b.name)).map((p) => p.name);
      await openPlace(page, "playlists");
      await expect.poll(() => gridTitles(page, "playlists-grid", 3)).toEqual(bySongs.slice(0, 3));
      await app.close();
      // A restart with the same profile.
      ({ app, page } = await launchFake(userData));
      await expect(page.getByTestId("app").or(page.getByTestId("setup"))).toBeVisible({ timeout: 15_000 });
      if (await page.getByTestId("setup").isVisible()) await completeSetup(page);
      await openPlace(page, "playlists");
      await expect(page.getByTestId("playlists-sort")).toHaveAttribute("aria-label", "Sort: Songs, Most");
      await openPlace(page, "genres");
      await expect(page.getByTestId("genres-sort")).toHaveAttribute("aria-label", "Sort: Albums, Fewest");
      await app.close();
    } finally {
      rmSync(userData, { recursive: true, force: true });
    }
  });

  test("artists are a grid of round portraits in the chosen order", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await openPlace(page, "artists");
    const grid = page.getByTestId("artists-grid");
    await expect(grid).toHaveAttribute("role", "grid");
    expect(Number(await grid.getAttribute("aria-colcount"))).toBeGreaterThan(1);
    await expect(grid.getByTestId("grid-tile").first().locator(".art")).toHaveClass(/round/);
    // Name A–Z by default.
    const sid = await serverId(page);
    const query = async (sort: string, descending: boolean) => ((await page.evaluate(({ s, sort, descending }) => window.hocket.query({ type: "artists", data: { server_id: s, page: { offset: 0, limit: 5 }, sort: sort as never, descending } }), { s: sid, sort, descending })) as { data: { items: { name: string; albumCount: number }[] } }).data.items;
    await expect.poll(() => gridTitles(page, "artists-grid", 5)).toEqual((await query("title", false)).map((a) => a.name));
    // Most albums first: the counts under the names fall.
    await page.getByTestId("artists-sort").click();
    await page.getByTestId("artists-sort-menu").getByRole("menuitemradio", { name: "Albums" }).click();
    await page.getByTestId("sort-descending").click();
    await page.keyboard.press("Escape");
    const top = await query("albumCount", true);
    await expect.poll(() => gridTitles(page, "artists-grid", 5)).toEqual(top.map((a) => a.name));
    for (let i = 1; i < top.length; i++) expect(top[i - 1]!.albumCount).toBeGreaterThanOrEqual(top[i]!.albumCount);
    await expect(grid.getByTestId("grid-tile").first().locator(".t2")).toHaveText(`${top[0]!.albumCount} albums`);
    // Random is a stable shuffle, unlike name order.
    await page.getByTestId("artists-sort").click();
    await page.getByTestId("artists-sort-menu").getByRole("menuitemradio", { name: "Random" }).click();
    await page.keyboard.press("Escape");
    await expect.poll(async () => (await gridTitles(page, "artists-grid", 5)).join()).toBe((await query("random", true)).map((a) => a.name).join());
  });

  test("playlists start with Pinned (in pin order) and Recent (the latest changed, not pinned)", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    const sid = await serverId(page);
    const lists = (await page.evaluate((s) => window.hocket.query({ type: "playlists", data: { server_id: s } }), sid)) as { data: { id: string; name: string; changed?: number }[] };
    const [a, b] = [lists.data[5]!, lists.data[2]!];
    await page.evaluate((pins) => window.hocket.dispatch({ type: "setSetting", data: { key: "library.pins", value: JSON.stringify(pins) } }), [
      { kind: "playlist", id: a.id, tap: "open" },
      { kind: "album", id: "al-1", tap: "open" },
      { kind: "playlist", id: b.id, tap: "open" },
    ]);
    await openPlace(page, "playlists");
    const pinned = page.getByTestId("playlists-pinned");
    await expect(pinned.getByRole("heading", { name: "Pinned" })).toBeVisible();
    await expect(pinned.getByTestId("group-pinned-item")).toHaveCount(2);
    await expect(pinned.getByTestId("group-pinned-item").nth(0)).toContainText(a.name);
    await expect(pinned.getByTestId("group-pinned-item").nth(1)).toContainText(b.name);
    const recent = page.getByTestId("playlists-recent");
    await expect(recent.getByRole("heading", { name: "Recent" })).toBeVisible();
    const expected = lists.data.filter((p) => p.id !== a.id && p.id !== b.id && p.changed !== undefined).sort((x, y) => (y.changed ?? 0) - (x.changed ?? 0) || x.id.localeCompare(y.id)).slice(0, 4).map((p) => p.name);
    await expect(recent.getByTestId("group-recent-item")).toHaveCount(expected.length);
    expect(await recent.getByTestId("group-recent-item").locator(".t1").allTextContents()).toEqual(expected);
    // The whole list is still complete, under its own header with the sort.
    await expect(page.getByRole("heading", { name: "All playlists" })).toBeVisible();
    await expect(page.getByTestId("playlists-grid").getByTestId("grid-tile")).toHaveCount(lists.data.length);
    // A group tile opens its playlist.
    await pinned.getByTestId("group-pinned-item").first().locator(".tile-main").click();
    await expect(page.getByTestId("view-playlist")).toBeVisible();
    await expect(page.getByTestId("view-playlist").locator("h1")).toHaveText(a.name);
    // Albums: Recent holds the albums of the recently played songs; nothing pinned there but al-1.
    await openPlace(page, "albums");
    await expect(page.getByTestId("albums-pinned").getByTestId("group-pinned-item")).toHaveCount(1);
    // Recent loads after the history and each album's details: wait for it rather than count at once.
    const recentAlbums = page.getByTestId("albums-recent").getByTestId("group-recent-item");
    await expect(recentAlbums.first()).toBeVisible();
    const pinnedAlbum = (await page.evaluate(() => window.hocket.query({ type: "album", data: { id: "al-1" } }))) as { data: { name: string } };
    expect(await recentAlbums.locator(".t1").allTextContents()).not.toContain(pinnedAlbum.data.name);
    expect(await recentAlbums.count()).toBeLessThanOrEqual(6);
  });

  test("the Library's Recently added has no Newest first note, and the list titles use the wordmark", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await page.getByTestId("nav-library").click();
    await expect(page.getByTestId("view-library").getByRole("heading", { name: "Recently added" })).toBeVisible();
    await expect(page.getByTestId("view-library").getByText("Newest first")).toHaveCount(0);
    const font = (view: string) => page.getByTestId(view).locator(".view-header h1").evaluate((el) => getComputedStyle(el).fontFamily);
    expect(await font("view-library")).toContain("Unbounded");
    for (const place of ["albums", "artists", "playlists", "songs", "genres", "filters", "downloads", "stats"] as const) {
      await openPlace(page, place);
      expect(await font(`view-${place}`), place).toContain("Unbounded");
    }
  });
});
