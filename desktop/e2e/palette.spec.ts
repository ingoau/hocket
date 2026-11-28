// The command palette (Ctrl/Cmd+K): grouped library results and commands, Enter running the row's
// primary action, recent picks as Suggestions, and the actions menu (Ctrl/Cmd+K again) with its
// keycaps, filter and Escape order; the menu's chords also work straight from the search field.
// Add to playlist opens the playlist submenu off the actions menu, and a library row drags out of
// the palette onto the window under it.
import type { Page } from "@playwright/test";
import { expectNoViolations } from "./a11y-helpers";
import { completeSetup, expect, html5DragEnd, html5DragOver, html5DragStart, html5Drop, openPlace, playFirstAlbum, test } from "./fixtures";

/** Opens a playlist from the Playlists page by name. */
async function openPlaylist(page: Page, name: string): Promise<void> {
  await openPlace(page, "playlists");
  await page.getByTestId("playlists-grid").getByTestId("grid-tile").filter({ hasText: name }).first().dblclick();
  await expect(page.getByTestId("view-playlist")).toBeVisible();
}

async function playlistTrackIds(page: Page, id: string): Promise<string[]> {
  const r = (await page.evaluate((pid) => window.hocket.query({ type: "playlistTracks", data: { id: pid, page: { offset: 0, limit: 10_000 } } }), id)) as { data: { items: { id: string }[] } };
  return r.data.items.map((x) => x.id);
}

test.describe("command palette", () => {
  test("groups results by kind, runs a command on Enter and suggests it next time", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await page.getByTestId("content").click();
    await page.keyboard.press("Control+k");
    const palette = page.getByTestId("palette");
    await expect(palette).toBeVisible();
    // No query: the commands, no library groups.
    await expect(palette.getByTestId("palette-group-action")).toBeVisible();
    await expect(palette.getByTestId("palette-group-track")).toHaveCount(0);
    await page.getByTestId("palette-input").fill("s");
    await expect(palette.getByTestId("palette-group-track").getByTestId("palette-item").first()).toBeVisible();
    // Groups in their fixed order, each row with its type on the right.
    const order = await palette.locator("[data-testid^=palette-group-]").evaluateAll((els) => els.map((el) => el.getAttribute("data-testid")));
    const at = (id: string) => order.indexOf(`palette-group-${id}`);
    expect(at("track")).toBe(0);
    for (const [a, b] of [["track", "album"], ["album", "artist"], ["artist", "action"]] as const) if (at(b) >= 0) expect(at(a)).toBeLessThan(at(b));
    await expect(palette.getByTestId("palette-group-track").locator(".pi-type").first()).toHaveText("Song");
    await expect(palette.getByTestId("palette-group-action").locator(".pi-type").first()).toHaveText("Command");
    await page.getByTestId("palette-input").fill("settings");
    await page.keyboard.press("Enter");
    await expect(page.getByTestId("view-settings")).toBeVisible();
    await expect(palette).toHaveCount(0);
    // Next time, with no query, Settings leads the Suggestions.
    await page.getByTestId("content").click();
    await page.keyboard.press("Control+k");
    const suggestions = palette.getByTestId("palette-group-suggestions");
    await expect(suggestions.getByTestId("palette-item").first()).toContainText("Settings");
    await page.keyboard.press("Escape");
    await expect(palette).toHaveCount(0);
  });

  test("the actions menu: Ctrl+K on a row, keycaps, its filter, Escape closes it then the palette", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await page.getByTestId("content").click();
    await page.keyboard.press("Control+k");
    const palette = page.getByTestId("palette");
    const input = page.getByTestId("palette-input");
    await input.fill("s");
    const firstSong = palette.getByTestId("palette-group-track").getByTestId("palette-item").first();
    await expect(firstSong).toBeVisible();
    // The first row is a song and active: the footer names its primary action.
    await expect(input).toHaveAttribute("aria-activedescendant", "pal-opt-0");
    await expect(page.getByTestId("palette-primary")).toContainText("Play");
    await page.keyboard.press("Control+k");
    const menu = page.getByTestId("palette-actions");
    await expect(menu).toBeVisible();
    await expect(page.getByTestId("palette-actions-filter")).toBeFocused();
    const actions = menu.getByTestId("palette-action");
    await expect(actions.first()).toHaveText(/Play.*↵/);
    // The context menu's actions, each with its chord as keycaps.
    const playNext = actions.filter({ hasText: "Play next" });
    await expect(playNext).toHaveAttribute("aria-keyshortcuts", "Control+Enter");
    await expect(playNext.locator(".keycap")).toHaveText(["Ctrl", "↵"]);
    await expect(actions.filter({ hasText: "Add to queue" }).locator(".keycap")).toHaveText(["Ctrl", "Shift", "↵"]);
    await expect(actions.filter({ hasText: "Add to playlist…" })).toHaveCount(1);
    await expect(actions.filter({ hasText: "Go to album" })).toHaveCount(1);
    await expect(actions.filter({ hasText: /^(Love|Unlove)/ })).toHaveCount(1);
    await expectNoViolations(page, "palette actions menu");
    // Grouped: the play group is set apart from the rest.
    expect(await menu.locator(".pa-group").count()).toBeGreaterThan(1);
    // The menu sits bottom-right of the palette.
    const p = (await palette.boundingBox())!;
    const m = (await menu.boundingBox())!;
    expect(p.x + p.width - (m.x + m.width)).toBeLessThan(24);
    expect(m.y + m.height).toBeGreaterThan(p.y + p.height - 80);
    // Its own filter.
    await page.keyboard.type("queue");
    await expect(actions).toHaveCount(1);
    await expect(actions.first()).toContainText("Add to queue");
    // Escape closes the menu, back to the search with the row still active; then the palette.
    await page.keyboard.press("Escape");
    await expect(menu).toHaveCount(0);
    await expect(palette).toBeVisible();
    await expect(input).toBeFocused();
    await expect(input).toHaveValue("s");
    // Ctrl+K toggles it, and the footer's Actions button does too.
    await page.keyboard.press("Control+k");
    await expect(menu).toBeVisible();
    await page.keyboard.press("Control+k");
    await expect(menu).toHaveCount(0);
    await page.getByTestId("palette-actions-button").click();
    await expect(menu).toBeVisible();
    // A press outside the menu only dismisses it: the row clicked doesn't run.
    await firstSong.click({ position: { x: 40, y: 10 } });
    await expect(menu).toHaveCount(0);
    await expect(palette).toBeVisible();
    // Enter on the focused Actions button presses it (it doesn't play the row).
    await page.getByTestId("palette-actions-button").focus();
    await page.keyboard.press("Enter");
    await expect(menu).toBeVisible();
    await page.keyboard.press("Escape");
    await page.keyboard.press("Escape");
    await expect(palette).toHaveCount(0);
  });

  test("the palette opens at once, and the arrows keep the actions menu's active row in view", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await page.getByTestId("content").click();
    await page.keyboard.press("Control+k");
    const palette = page.getByTestId("palette");
    await expect(palette).toBeVisible();
    // No entrance animation on the sheet or its scrim.
    expect(await palette.evaluate((el) => getComputedStyle(el).animationName)).toBe("none");
    expect(await palette.evaluate((el) => getComputedStyle(el.parentElement!).animationName)).toBe("none");
    await page.getByTestId("palette-input").fill("s");
    await expect(palette.getByTestId("palette-group-track").getByTestId("palette-item").first()).toBeVisible();
    await page.keyboard.press("Control+k");
    const actions = page.getByTestId("palette-action");
    await expect(actions.first()).toBeVisible();
    const count = await actions.count();
    for (let i = 1; i < count; i++) await page.keyboard.press("ArrowDown");
    const last = actions.nth(count - 1);
    await expect(last).toHaveAttribute("aria-selected", "true");
    // Wholly inside the scrolled list (the list scrolled, so it overflows).
    const list = page.locator("#palette-actions-list");
    expect(await list.evaluate((el) => el.scrollHeight > el.clientHeight)).toBe(true);
    const inView = (row: typeof last) => async () => {
      const l = (await list.boundingBox())!;
      const r = (await row.boundingBox())!;
      return r.y >= l.y - 2 && r.y + r.height <= l.y + l.height + 2;
    };
    await expect.poll(inView(last)).toBe(true);
    // And back up to the first.
    for (let i = 1; i < count; i++) await page.keyboard.press("ArrowUp");
    await expect(actions.first()).toHaveAttribute("aria-selected", "true");
    await expect.poll(inView(actions.first())).toBe(true);
  });

  test("opt-in telemetry logs a pick that wasn't the first row and an undo right after it", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await page.getByTestId("content").click();
    await page.keyboard.press("Control+k");
    await page.getByTestId("palette-input").fill("settings");
    await page.keyboard.press("Enter");
    await page.getByTestId("settings-nav-diagnostics").click();
    const toggle = page.getByTestId("setting-palette-telemetry");
    // Off by default, with nothing logged.
    await expect(toggle).not.toBeChecked();
    await expect(page.getByTestId("palette-telemetry-summary")).toHaveCount(0);
    await toggle.click();
    await expect(toggle).toBeChecked();
    // Pick the second song, then undo straight away.
    await page.getByTestId("content").click();
    await page.keyboard.press("Control+k");
    const palette = page.getByTestId("palette");
    await page.getByTestId("palette-input").fill("s");
    await expect(palette.getByTestId("palette-group-track").getByTestId("palette-item").nth(1)).toBeVisible();
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("Enter");
    await expect(palette).toHaveCount(0);
    // Focus off any field (a click at the page's centre could land on one of Settings' controls).
    await page.getByTestId("content").focus();
    await page.keyboard.press("Control+z");
    const row = page.locator("#setting-paletteTelemetry + .setting-row");
    await expect(row).toContainText("1 picks, 1 not the first row (100%), 1 undone");
    // A palette closed without a pick counts too.
    await page.keyboard.press("Control+k");
    await page.keyboard.press("Escape");
    await expect(row).toContainText("1 closed without a pick");
    await page.getByTestId("palette-telemetry-clear").click();
    await expect(row).toContainText("0 picks");
  });

  test("the menu's chords work from the search field: Play next on a song, Add to playlist on an album, Enter opens an album", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playFirstAlbum(page);
    const palette = page.getByTestId("palette");
    const input = page.getByTestId("palette-input");

    // A song: Ctrl+Enter plays it next, without opening the menu.
    await page.getByTestId("content").click();
    await page.keyboard.press("Control+k");
    await input.fill("s");
    await expect(palette.getByTestId("palette-group-track").getByTestId("palette-item").first()).toBeVisible();
    await page.keyboard.press("Control+k");
    const title = (await page.getByTestId("palette-actions").locator(".pa-title").textContent()) ?? "";
    expect(title).not.toBe("");
    await page.keyboard.press("Escape");
    await page.keyboard.press("Control+Enter");
    await expect(palette).toHaveCount(0);
    const timeline = page.getByTestId("queue-timeline");
    await expect(timeline).toContainText("Next in queue");
    await expect(timeline).toContainText(title);

    // An album: Ctrl+P opens the playlist picker for it.
    await page.getByTestId("content").click();
    await page.keyboard.press("Control+k");
    await input.fill("a");
    const album = palette.getByTestId("palette-group-album").getByTestId("palette-item").first();
    await expect(album).toBeVisible();
    // Pointing at a row makes it the active one, as ↑↓ would.
    await album.hover({ position: { x: 40, y: 10 } });
    await expect(input).toHaveAttribute("aria-activedescendant", (await album.getAttribute("id")) ?? "");
    // It opens the actions menu on its playlist page; Escape steps back out, one at a time.
    await page.keyboard.press("Control+p");
    const picker = page.getByTestId("playlist-submenu");
    await expect(picker).toBeVisible();
    await expect(page.getByTestId("palette-actions")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(picker).toHaveCount(0);
    await expect(page.getByTestId("palette-actions")).toBeVisible();
    await page.keyboard.press("Escape");
    await page.keyboard.press("Escape");
    await expect(palette).toHaveCount(0);

    // Enter on an album opens it.
    await page.getByTestId("content").click();
    await page.keyboard.press("Control+k");
    await input.fill("a");
    await expect(album).toBeVisible();
    await album.click();
    await expect(page.getByTestId("view-album")).toBeVisible();
    await expect(palette).toHaveCount(0);
  });

  test("a library result opens its context menu from the pointer and the keyboard", async ({ hocket }) => {
    // R-20261104-9rrx: right-clicking a result in the palette did nothing.
    const { page } = hocket;
    await completeSetup(page);
    await page.getByTestId("content").click();
    await page.keyboard.press("Control+k");
    const palette = page.getByTestId("palette");
    const input = page.getByTestId("palette-input");
    await input.fill("s");
    const results = palette.locator('[data-testid="palette-item"]', { has: page.locator(".art") });
    await expect(results.first()).toBeVisible();
    const menu = page.getByTestId("context-menu");
    await results.first().click({ button: "right" });
    await expect(menu).toBeVisible();
    await expect(menu.getByRole("menuitem").first()).toBeVisible();
    // Escape closes the menu only; focus goes back to the palette.
    await page.keyboard.press("Escape");
    await expect(menu).toHaveCount(0);
    await expect(palette).toBeVisible();
    await expect(input).toBeFocused();
    // Shift+F10 in the input opens the active result's menu.
    await page.keyboard.press("Shift+F10");
    await expect(menu).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(input).toBeFocused();
    // A menu action that navigates takes the palette away with it.
    const track = palette.locator('[data-testid="palette-item"]', { has: page.locator(".art") }).first();
    await track.click({ button: "right" });
    const goTo = menu.getByRole("menuitem", { name: /^Go to (album|artist)$/ }).first();
    const where = await goTo.textContent();
    await goTo.click();
    await expect(palette).toHaveCount(0);
    // Going where the view already is closes the palette too.
    await page.keyboard.press("Control+k");
    await input.fill("s");
    await palette.locator('[data-testid="palette-item"]', { has: page.locator(".art") }).first().click({ button: "right" });
    await menu.getByRole("menuitem", { name: where ?? "", exact: true }).click();
    await expect(palette).toHaveCount(0);
  });

  test("Add to playlist is a page of the actions menu, starting on the open playlist; Escape goes back to the actions as they were", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await openPlaylist(page, "Late drive");
    const palette = page.getByTestId("palette");
    const input = page.getByTestId("palette-input");
    await page.keyboard.press("Control+k");
    await input.fill("s");
    const song = palette.getByTestId("palette-group-track").getByTestId("palette-item").first();
    await expect(song).toBeVisible();
    await page.keyboard.press("Control+k");
    const actions = page.getByTestId("palette-actions");
    const filter = page.getByTestId("palette-actions-filter");
    await filter.fill("pl");
    await actions.getByTestId("palette-action").filter({ hasText: "Add to playlist" }).click();

    // In the menu's place (its actions gone), on the open playlist's row.
    const sub = actions.getByTestId("playlist-submenu");
    await expect(sub).toBeVisible();
    await expect(sub.locator(".pa-title")).toHaveText("Add to playlist");
    await expect(actions.getByTestId("palette-action")).toHaveCount(0);
    await expect(palette).toBeVisible();
    const late = sub.getByTestId("playlist-row").filter({ hasText: "Late drive" }).first();
    const search = sub.getByTestId("playlist-search");
    await expect(search).toBeFocused();
    await expect(search).toHaveAttribute("aria-activedescendant", (await late.getAttribute("id")) ?? "");
    const id = (await late.getAttribute("data-playlist")) ?? "";
    const before = await playlistTrackIds(page, id);

    // Enter adds the song there; the submenu stays for more.
    await page.keyboard.press("Enter");
    await expect(page.getByTestId("toast").filter({ hasText: "Late drive" })).toBeVisible();
    await expect.poll(async () => (await playlistTrackIds(page, id)).length).toBe(before.length + 1);
    await expect(late).toHaveAttribute("aria-checked", "true");

    // Escape: back to the actions, their filter (focused) and Add to playlist as they were; then the
    // menu closes, then the palette.
    await page.keyboard.press("Escape");
    await expect(sub).toHaveCount(0);
    await expect(actions).toBeVisible();
    await expect(filter).toBeFocused();
    await expect(filter).toHaveValue("pl");
    await expect(actions.locator(".pa-item.active")).toHaveText(/Add to playlist/);
    await page.keyboard.press("Escape");
    await expect(actions).toHaveCount(0);
    await expect(input).toBeFocused();

    // Ctrl+K on the page closes the menu, as it does from the actions.
    await page.keyboard.press("Control+k");
    await page.keyboard.press("Control+p");
    await expect(sub).toBeVisible();
    await expect(search).toBeFocused();
    await page.keyboard.press("Control+k");
    await expect(actions).toHaveCount(0);
    await expect(input).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(palette).toHaveCount(0);
  });

  test("a song dragged out of the palette steps it aside for the drop; a drag with no drop brings it back", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await page.evaluate(() => window.hocket.dispatch({ type: "setSetting", data: { key: "library.pins", value: JSON.stringify([{ kind: "playlist", id: "pl-1", tap: "open" }]) } }));
    const pin = page.locator('[data-testid="sidebar-pin"][data-pin="playlist:pl-1"]');
    await expect(pin).toBeVisible();
    const palette = page.getByTestId("palette");
    const overlay = page.locator(".palette-overlay");
    const input = page.getByTestId("palette-input");
    const before = await playlistTrackIds(page, "pl-1");

    // No drop: the palette comes back as it was.
    await page.keyboard.press("Control+k");
    await input.fill("s");
    const song = palette.getByTestId("palette-group-track").getByTestId("palette-item").first();
    await expect(song).toBeVisible();
    await expect(song).toHaveAttribute("draggable", "true");
    await html5DragStart(page, song);
    await html5DragOver(page, pin);
    await expect(overlay).toHaveClass(/drag-aside/);
    await expect(page.getByTestId("app-body")).not.toHaveAttribute("inert", "");
    await html5DragEnd(page);
    await expect(overlay).not.toHaveClass(/drag-aside/);
    await expect(input).toHaveValue("s");
    await expect(input).toBeFocused();

    // Dropped on a playlist's pin: added there, and the palette closes.
    await html5DragStart(page, song);
    await html5DragOver(page, pin);
    await expect(overlay).toHaveClass(/drag-aside/);
    await html5Drop(page, pin);
    await expect(palette).toHaveCount(0);
    await expect(page.getByTestId("toast").filter({ hasText: "Added to Morning coffee" })).toBeVisible();
    await expect.poll(async () => (await playlistTrackIds(page, "pl-1")).length).toBe(before.length + 1);
  });
});
