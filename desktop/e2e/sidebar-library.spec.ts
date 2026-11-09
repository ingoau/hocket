// The sidebar: its places, the pins, then Your library (every playlist, the albums and artists
// played lately, newest first) with its chips, filter and sort; and the top bar's toggle that
// folds it to a rail of icons and covers.
import type { Page } from "@playwright/test";
import type { ServerInfo } from "../src/core/api";
import { completeSetup, expect, test } from "./fixtures";

async function playAlbum(page: Page, id: string): Promise<void> {
  const servers = (await page.evaluate(() => window.hocket.query({ type: "servers" }))) as { data: ServerInfo[] };
  const serverId = servers.data[0]!.id;
  await page.evaluate(({ serverId, id }) => window.hocket.dispatch({ type: "playContext", data: { args: { context: { serverId, kind: { type: "album", data: { id } }, label: "Album", sort: "default" }, shuffle: false, saveOutgoing: true } } } as never), { serverId, id });
  await expect.poll(async () => ((await page.evaluate(() => window.hocket.query({ type: "snapshot" }))) as { data: { queue: { current?: { track: { albumId?: string } } } } }).data.queue.current?.track.albumId).toBe(id);
}

const entries = (page: Page) => page.getByTestId("sidebar-library-item").evaluateAll((els) => els.map((el) => el.getAttribute("data-entry")));

test.describe("Your library in the sidebar", () => {
  test("lists what was just played first, leaves out pins, and narrows by kind, by name and by sort", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playAlbum(page, "al-3");
    await playAlbum(page, "al-1");
    const list = page.getByTestId("sidebar-library-item");
    await expect(list.first()).toHaveAttribute("data-entry", "album:al-1");
    // The playing album is marked.
    await expect(list.first()).toHaveClass(/playing/);
    const all = await entries(page);
    expect(all).toContain("album:al-3");
    expect(all.some((e) => e?.startsWith("playlist:"))).toBe(true);
    expect(all.some((e) => e?.startsWith("artist:"))).toBe(true);

    // A pinned album moves up to the pins and out of the list.
    await page.evaluate(() => window.hocket.dispatch({ type: "setSetting", data: { key: "library.pins", value: JSON.stringify([{ kind: "album", id: "al-1", tap: "open" }]) } }));
    await expect(page.locator('[data-testid="sidebar-pin"][data-pin="album:al-1"]')).toBeVisible();
    await expect(page.locator('[data-testid="sidebar-library-item"][data-entry="album:al-1"]')).toHaveCount(0);

    // Albums only, then back to everything.
    await page.getByTestId("side-lib-chip-album").click();
    await expect(page.getByTestId("side-lib-chip-album")).toHaveAttribute("aria-pressed", "true");
    expect((await entries(page)).every((e) => e?.startsWith("album:"))).toBe(true);
    await page.getByTestId("side-lib-chip-playlist").click();
    expect((await entries(page)).every((e) => e?.startsWith("playlist:"))).toBe(true);
    // Remembered on the device.
    await page.reload();
    await expect(page.getByTestId("side-lib-chip-playlist")).toHaveAttribute("aria-pressed", "true");
    await page.getByTestId("side-lib-chip-playlist").click();

    // A–Z from the sort menu.
    await page.getByTestId("side-lib-sort").click();
    await page.getByRole("menuitemradio", { name: "Alphabetical" }).click();
    const titles = await page.locator('[data-testid="sidebar-library-item"] .lib-title').allTextContents();
    expect(titles).toEqual([...titles].sort((a, b) => a.localeCompare(b, undefined, { sensitivity: "base" })));

    // The filter field: matching names only, and a line when nothing matches.
    await page.getByTestId("side-lib-search").click();
    const field = page.getByTestId("side-lib-filter");
    await expect(field).toBeFocused();
    const first = titles[0]!;
    await field.fill(first);
    await expect(page.locator('[data-testid="sidebar-library-item"] .lib-title').first()).toHaveText(first);
    await field.fill("zzz nothing like this");
    await expect(page.getByTestId("side-lib-empty")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(field).toHaveCount(0);
    await expect(page.getByTestId("sidebar-library-item").first()).toBeVisible();
  });

  test("an item opens its page, and right-click gives its menu", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playAlbum(page, "al-2");
    const item = page.locator('[data-testid="sidebar-library-item"][data-entry="album:al-2"]');
    await item.click();
    await expect(page.getByTestId("view-album")).toBeVisible();
    await expect(item).toHaveAttribute("aria-current", "page");
    await item.click({ button: "right" });
    await expect(page.getByRole("menu")).toBeVisible();
    await page.keyboard.press("Escape");
  });
});

test.describe("the collapsed sidebar", () => {
  test("the top bar's toggle and Mod+B fold it to a rail and back, remembered", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playAlbum(page, "al-1");
    const sidebar = page.getByTestId("sidebar");
    const wide = (await sidebar.boundingBox())!.width;
    await page.getByTestId("sidebar-toggle").click();
    await expect(sidebar).toHaveAttribute("data-rail", "true");
    const rail = (await sidebar.boundingBox())!.width;
    expect(rail).toBeLessThan(80);
    // Icons and covers only: no resize handle, no chips, the names left for tooltips and screen readers.
    await expect(page.getByTestId("sidebar-resize")).toHaveCount(0);
    await expect(page.getByTestId("side-lib-chip-album")).toHaveCount(0);
    await expect(page.getByTestId("sidebar-library-item").first()).toBeVisible();
    await expect(page.getByTestId("nav-home")).toHaveAccessibleName(/Home/);
    await expect(page.getByTestId("sidebar-toggle")).toHaveAccessibleName("Expand sidebar");
    await page.reload();
    await expect(page.getByTestId("sidebar")).toHaveAttribute("data-rail", "true");
    await page.locator("body").click({ position: { x: 700, y: 400 } });
    await page.keyboard.press("ControlOrMeta+b");
    await expect(page.getByTestId("sidebar")).not.toHaveAttribute("data-rail", "true");
    expect(Math.round((await sidebar.boundingBox())!.width)).toBe(Math.round(wide));
  });
});
