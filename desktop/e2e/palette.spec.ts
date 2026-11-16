// The command palette (Ctrl/Cmd+K): grouped library results and commands, Enter running the row's
// primary action, recent picks as Suggestions, and the actions menu (Ctrl/Cmd+K again) with its
// keycaps, filter and Escape order; the menu's chords also work straight from the search field.
import { expectNoViolations } from "./a11y-helpers";
import { completeSetup, expect, playFirstAlbum, test } from "./fixtures";

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
    await page.keyboard.press("Control+p");
    await expect(palette).toHaveCount(0);
    const picker = page.getByTestId("playlist-submenu");
    await expect(picker).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(picker).toHaveCount(0);

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
});
