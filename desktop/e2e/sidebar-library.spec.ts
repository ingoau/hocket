// The sidebar: its places, the pins, then Your library (every playlist, the albums and artists
// played lately, newest first) with its chips, filter and sort; and the top bar's toggle that
// folds it to a rail of icons and covers.
import type { Page } from "@playwright/test";
import type { AccountInfo } from "../src/core/api";
import { completeSetup, expect, resizeWindow, test } from "./fixtures";

async function playAlbum(page: Page, id: string): Promise<void> {
  const servers = (await page.evaluate(() => window.hocket.query({ type: "accounts" }))) as { data: AccountInfo[] };
  const serverId = servers.data[0]!.id;
  await page.evaluate(({ serverId, id }) => window.hocket.dispatch({ type: "playContext", data: { args: { context: { source: serverId, kind: { type: "album", data: { id } }, label: "Album", sort: "default" }, shuffle: false, saveOutgoing: true } } } as never), { serverId, id });
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
    // The rows are virtualised: what is near the top is drawn.
    expect(await entries(page)).toContain("album:al-3");

    // A pinned album moves up to the pins and out of the list.
    await page.evaluate(() => window.hocket.dispatch({ type: "setSetting", data: { key: "library.pins", value: JSON.stringify([{ kind: "album", id: "al-1", tap: "open" }]) } }));
    await expect(page.locator('[data-testid="sidebar-pin"][data-pin="album:al-1"]')).toBeVisible();
    await expect(page.locator('[data-testid="sidebar-library-item"][data-entry="album:al-1"]')).toHaveCount(0);

    // Albums only, then back to everything.
    await page.getByTestId("side-lib-chip-album").click();
    await expect(page.getByTestId("side-lib-chip-album")).toHaveAttribute("aria-pressed", "true");
    expect((await entries(page)).every((e) => e?.startsWith("album:"))).toBe(true);
    await page.getByTestId("side-lib-chip-artist").click();
    await expect(list.first()).toBeVisible();
    expect((await entries(page)).every((e) => e?.startsWith("artist:"))).toBe(true);
    await page.getByTestId("side-lib-chip-playlist").click();
    await expect(list.first()).toBeVisible();
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

test.describe("Your library from the keyboard", () => {
  test("one tab stop; arrows, Home and End move along the whole list, drawn or not", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playAlbum(page, "al-1");
    const rows = page.getByTestId("sidebar-library-item");
    await expect(rows.first()).toBeVisible();
    // A single tab stop in the list.
    expect(await rows.evaluateAll((els) => els.filter((el) => (el as HTMLElement).tabIndex === 0).length)).toBe(1);
    await rows.first().focus();
    const focused = () => page.evaluate(() => {
      const li = document.activeElement?.closest("li");
      return { pos: Number(li?.getAttribute("aria-posinset")), size: Number(li?.getAttribute("aria-setsize")), entry: document.activeElement?.getAttribute("data-entry") };
    });
    expect((await focused()).pos).toBe(1);
    await page.keyboard.press("ArrowDown");
    await expect.poll(async () => (await focused()).pos).toBe(2);
    await page.keyboard.press("End");
    await expect.poll(async () => { const f = await focused(); return f.pos === f.size && f.size > 1; }).toBe(true);
    await page.keyboard.press("Home");
    await expect.poll(async () => (await focused()).pos).toBe(1);
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

  test("a narrow window is a rail with the covers showing, and Mod+B leaves the remembered state alone", async ({ hocket }) => {
    const { app, page } = hocket;
    await completeSetup(page);
    await playAlbum(page, "al-1");
    await resizeWindow(app, page, 820, 700);
    await expect(page.getByTestId("sidebar")).toHaveAttribute("data-rail", "true");
    await expect(page.getByTestId("sidebar-toggle")).toHaveCount(0);
    const thumb = page.locator('[data-testid="sidebar-library-item"] .lib-thumb').first();
    await expect(thumb).toBeVisible();
    expect((await thumb.boundingBox())!.width).toBeGreaterThan(30);
    await page.locator("body").click({ position: { x: 500, y: 300 } });
    await page.keyboard.press("ControlOrMeta+b");
    await resizeWindow(app, page, 1456, 912);
    // Still expanded: the shortcut did nothing while the window was narrow.
    await expect(page.getByTestId("sidebar")).not.toHaveAttribute("data-rail", "true");
  });
});
