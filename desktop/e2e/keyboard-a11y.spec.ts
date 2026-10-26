// Keyboard-only walkthrough (no mouse anywhere in this file): setup, the skip
// link, a full Tab cycle in a logical order with a visible focus ring on every
// stop, roving grids and tables, activation keys, the context-menu key, focus
// traps and focus return for menus, dialogs, the palette and the fullscreen
// player, the queue listbox and selector, and the seek/volume/splitter sliders.
import type { Page } from "@playwright/test";
import { expect, openPlace, test } from "./fixtures";
import { focused, hasRing, keyboardSetup, type FocusInfo } from "./a11y-helpers";

/** Press Tab (or Shift+Tab) until the focused element matches; fails after `max` presses. */
async function tabTo(page: Page, match: (f: FocusInfo) => boolean, what: string, { max = 120, back = false } = {}): Promise<FocusInfo> {
  for (let i = 0; i < max; i++) {
    await page.keyboard.press(back ? "Shift+Tab" : "Tab");
    const f = await focused(page);
    if (f && match(f)) return f;
  }
  throw new Error(`Tab never reached ${what}`);
}
const byTestId = (id: string) => (f: FocusInfo) => f.testId === id;

async function activeAttr(page: Page, attr: string): Promise<string | null> {
  return page.evaluate((a) => document.activeElement?.getAttribute(a) ?? null, attr);
}

/** Keyboard only: the sidebar's Library, then the Library's `place` tile. */
async function openPlaceByKeyboard(page: Page, place: "albums" | "songs"): Promise<void> {
  await tabTo(page, byTestId("nav-library"), "the Library nav item");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("view-library")).toBeVisible();
  await tabTo(page, byTestId(`library-${place}`), `the Library's ${place} tile`);
  await page.keyboard.press("Enter");
  await expect(page.getByTestId(`view-${place}`)).toBeVisible();
}

/** Keyboard only: albums → first album → Play. */
async function playFirstAlbumByKeyboard(page: Page): Promise<void> {
  await openPlaceByKeyboard(page, "albums");
  await tabTo(page, (f) => f.role === "gridcell", "the album grid");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("view-album")).toBeVisible();
  await tabTo(page, byTestId("album-play"), "the album Play button");
  await page.keyboard.press("Enter");
  // Q shows the queue in the side panel (it opens on its overview).
  await page.keyboard.press("q");
  await expect(page.getByTestId("right-panel")).toHaveAttribute("data-view", "queue");
  await expect(page.getByTestId("queue-row-current")).toBeVisible();
}

test.describe("keyboard only", () => {
  test.setTimeout(120_000);

  test("skip link, then a full Tab cycle in landmark order with a visible ring on every stop", async ({ hocket }) => {
    const { page } = hocket;
    await keyboardSetup(page);
    // The first Tab in the library lands on the skip link, which shows itself and jumps to the content.
    await page.keyboard.press("Tab");
    const skip = await focused(page);
    expect(skip?.testId).toBe("skip-link");
    expect(skip?.visible).toBe(true);
    expect(hasRing(skip)).toBe(true);
    await page.keyboard.press("Enter");
    expect(await page.evaluate(() => document.activeElement?.id)).toBe("main");
    // The next Tab goes into the content, not back to the top bar.
    await page.keyboard.press("Tab");
    expect((await focused(page))?.region).toBe("main");
    await playFirstAlbumByKeyboard(page);
    // Tab from the skip link all the way round (focusing it is not a pointer action).
    await page.evaluate(() => document.querySelector<HTMLElement>('[data-testid="skip-link"]')?.focus());
    const order = ["banner", "navigation", "main", "complementary", "player", "toasts"];
    const stops: FocusInfo[] = [];
    for (let i = 0; i < 250; i++) {
      await page.keyboard.press("Tab");
      const f = await focused(page);
      if (!f || f.testId === "skip-link") break;
      stops.push(f);
    }
    expect(stops.length).toBeGreaterThan(25);
    expect(stops.length).toBeLessThan(250);
    const where = (f: FocusInfo) => `${f.region} ${f.tag}[role=${f.role}] ${f.testId ?? ""} "${f.name}"`;
    for (const f of stops) {
      expect(hasRing(f), `focus ring on ${where(f)}`).toBe(true);
      expect(f.visible, `visible: ${where(f)}`).toBe(true);
    }
    // Logical order: banner → sidebar → content → side panel → player bar (→ a toast's Undo), never back.
    const regions = stops.map((f) => order.indexOf(f.region));
    expect(regions.every((r) => r >= 0), stops.map(where).join("\n")).toBe(true);
    for (let i = 1; i < regions.length; i++) expect(regions[i]!, `${where(stops[i - 1]!)} → ${where(stops[i]!)}`).toBeGreaterThanOrEqual(regions[i - 1]!);
    // Composite widgets are one stop each: the track grid, the queue listbox, each rating group.
    expect(stops.filter((f) => f.role === "row").length).toBe(1);
    expect(stops.filter((f) => f.role === "listbox").length).toBe(1);
    expect(stops.filter((f) => f.role === "radio").length).toBe(2); // album rating, player-bar rating
    for (const id of ["search-input", "nav-library", "seek-slider", "volume", "play-pause", "queue-timeline", "queue-selector", "queue-autoplay", "sidebar-back", "side-panel-resize", "toggle-side-panel", "toggle-lyrics", "toggle-queue", "connect-button", "sidebar-resize"]) {
      expect(stops.some((f) => f.testId === id), `${id} is reachable`).toBe(true);
    }
  });

  test("album grid: one Tab stop, arrows rove, Home/End, Enter opens", async ({ hocket }) => {
    const { page } = hocket;
    await keyboardSetup(page);
    await openPlaceByKeyboard(page, "albums");
    const grid = page.getByTestId("albums-grid");
    await expect(grid.getByTestId("grid-tile").first()).toBeVisible();
    await tabTo(page, (f) => f.role === "gridcell", "the grid");
    expect(await activeAttr(page, "data-index")).toBe("0");
    const cols = Number(await grid.getAttribute("aria-colcount"));
    expect(cols).toBeGreaterThan(1);
    await page.keyboard.press("ArrowRight");
    expect(await activeAttr(page, "data-index")).toBe("1");
    await page.keyboard.press("ArrowDown");
    expect(await activeAttr(page, "data-index")).toBe(String(1 + cols));
    await page.keyboard.press("ArrowLeft");
    expect(await activeAttr(page, "data-index")).toBe(String(cols));
    // Exactly one tile is in the Tab order, and it is the focused one; it is selected.
    expect(await grid.locator('[role="gridcell"][tabindex="0"]').count()).toBe(1);
    await expect(grid.locator('[role="gridcell"][tabindex="0"]')).toBeFocused();
    await expect(grid.locator('[role="gridcell"][tabindex="0"]')).toHaveAttribute("aria-selected", "true");
    const total = Number((await page.getByTestId("view-albums").locator(".view-header .muted").first().textContent()) ?? "0");
    await page.keyboard.press("End");
    await expect.poll(() => activeAttr(page, "data-index")).toBe(String(total - 1));
    await page.keyboard.press("Home");
    await expect.poll(() => activeAttr(page, "data-index")).toBe("0");
    // Arrows inside the grid never reach the global keymap (ArrowLeft would seek).
    const title = await grid.locator('[role="gridcell"][data-index="0"]').getAttribute("aria-label");
    await page.keyboard.press("Enter");
    await expect(page.getByTestId("view-album")).toBeVisible();
    await expect(page.getByTestId("view-album").locator("h1")).toHaveText(title!.split(",")[0]!);
    // Back (Alt+Left) and focus lands somewhere sensible, not on <body>.
    await page.keyboard.press("Alt+ArrowLeft");
    await expect(page.getByTestId("view-albums")).toBeVisible();
  });

  test("activation keys: Enter plays, Space presses the focused button (not play/pause), ratings are a radio group", async ({ hocket }) => {
    const { page } = hocket;
    await keyboardSetup(page);
    await playFirstAlbumByKeyboard(page);
    const play = page.getByTestId("play-pause");
    await expect(play).toHaveAttribute("aria-label", "Pause");
    // Space on a toggle button toggles it, and playback keeps going.
    await tabTo(page, byTestId("shuffle"), "Shuffle");
    await page.keyboard.press("Space");
    await expect(page.getByTestId("shuffle")).toHaveAttribute("aria-pressed", "true");
    await expect(play).toHaveAttribute("aria-label", "Pause");
    await page.keyboard.press("Enter");
    await expect(page.getByTestId("shuffle")).toHaveAttribute("aria-pressed", "false");
    // Space on the play button itself pauses once (not twice).
    await tabTo(page, byTestId("play-pause"), "Play/Pause");
    await page.keyboard.press("Space");
    await expect(play).toHaveAttribute("aria-label", "Play");
    // The album's rating: one Tab stop, arrows rate, Delete clears.
    const group = page.getByTestId("view-album").locator(".detail-rating").getByRole("radiogroup");
    await expect(group).toHaveAttribute("aria-label", /^Rating for /);
    await tabTo(page, (f) => f.role === "radio" && f.region === "main", "the album rating");
    await page.keyboard.press("ArrowRight");
    await expect(group.getByRole("radio", { name: "2 stars" })).toHaveAttribute("aria-checked", "true");
    await expect(group.getByRole("radio", { name: "2 stars" })).toBeFocused();
    await page.keyboard.press("End");
    await expect(group.getByRole("radio", { name: "5 stars" })).toHaveAttribute("aria-checked", "true");
    await page.keyboard.press("Delete");
    await expect(group.getByRole("radio", { checked: true })).toHaveCount(0);
  });

  test("songs table: roving rows, Enter plays, digits rate, Shift+F10 and the menu key open the menu, Escape returns focus, dialogs trap Tab", async ({ hocket }) => {
    const { page } = hocket;
    await keyboardSetup(page);
    await openPlaceByKeyboard(page, "songs");
    await expect(page.getByTestId("track-row").first()).toBeVisible();
    await tabTo(page, (f) => f.role === "row", "the songs grid");
    expect(await activeAttr(page, "data-index")).toBe("0");
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("ArrowDown");
    expect(await activeAttr(page, "data-index")).toBe("2");
    const row = page.locator('[data-testid="track-row"][data-index="2"]');
    await expect(row).toBeFocused();
    await expect(row).toHaveAttribute("aria-selected", "true");
    // Enter plays that row; the table marks it current.
    await page.keyboard.press("Enter");
    await expect(row).toHaveAttribute("aria-current", "true");
    await expect(page.getByTestId("play-pause")).toHaveAttribute("aria-label", "Pause");
    // A digit rates the row.
    await page.keyboard.press("4");
    await expect(row.getByRole("radio", { name: "4 stars" })).toHaveAttribute("aria-checked", "true");
    // Shift+F10: the row's menu, first item active (the play strip's Play), focus inside.
    await page.keyboard.press("Shift+F10");
    const menu = page.getByTestId("context-menu");
    const activeCell = async () => page.locator(`#${await menu.getAttribute("aria-activedescendant")}`);
    await expect(menu).toBeVisible();
    await expect(menu).toBeFocused();
    await expect(menu).toHaveAttribute("aria-activedescendant", "cm-item-0");
    expect(await (await activeCell()).getAttribute("aria-label")).toBe("Play");
    // → moves along the strip; ↓ goes to the next line, the rating, landing on its current star.
    await page.keyboard.press("ArrowRight");
    expect(await (await activeCell()).getAttribute("aria-label")).toBe("Shuffle");
    await page.keyboard.press("ArrowDown");
    await expect(await activeCell()).toHaveAttribute("data-star", "4");
    await expect(await activeCell()).toHaveAttribute("aria-checked", "true");
    await page.keyboard.press("ArrowDown");
    await expect(await activeCell()).toHaveAttribute("role", "menuitemcheckbox");
    await page.keyboard.press("Escape");
    await expect(menu).toHaveCount(0);
    await expect(row).toBeFocused();
    // The context-menu key does the same; Add to playlist opens its submenu with the search focused.
    await page.keyboard.press("ContextMenu");
    await expect(menu).toBeVisible();
    for (let i = 0; i < 20; i++) {
      if ((await (await activeCell()).getAttribute("data-action")) === "addToPlaylist") break;
      await page.keyboard.press("ArrowDown");
    }
    await page.keyboard.press("ArrowRight");
    const search = page.getByTestId("playlist-search");
    await expect(search).toBeFocused();
    // ↓ walks the rows (New playlist first), Escape goes back to the menu, on Add to playlist.
    await page.keyboard.press("ArrowDown");
    await expect(search).toHaveAttribute("aria-activedescendant", "pp-row-0");
    await page.keyboard.press("ArrowDown");
    await expect(search).toHaveAttribute("aria-activedescendant", "pp-row-1");
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("playlist-submenu")).toHaveCount(0);
    await expect(menu).toBeFocused();
    expect(await (await activeCell()).getAttribute("data-action")).toBe("addToPlaylist");
    await page.keyboard.press("Escape");
    await expect(menu).toHaveCount(0);
    await expect(row).toBeFocused();
    // Add to playlist for what's playing (the player bar's +) is the same picker as the menu's
    // submenu, beside the button: focus goes to its search field, Tab or Escape closes it, and focus
    // goes back to the +.
    await tabTo(page, byTestId("bar-add"), "Add to playlist");
    await page.keyboard.press("Enter");
    const picker = page.getByTestId("playlist-submenu");
    await expect(picker).toBeVisible();
    await expect(page.getByTestId("playlist-search")).toBeFocused();
    await page.keyboard.press("ArrowDown");
    await expect(page.getByTestId("playlist-search")).toHaveAttribute("aria-activedescendant", "pp-row-0");
    await page.keyboard.press("Tab");
    await expect(picker).toHaveCount(0);
    await expect(page.getByTestId("bar-add")).toBeFocused();
    await page.keyboard.press("Enter");
    await expect(page.getByTestId("playlist-search")).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(picker).toHaveCount(0);
    await expect(page.getByTestId("bar-add")).toBeFocused();
  });

  test("queue listbox and selector, and the seek, volume and splitter sliders", async ({ hocket }) => {
    const { page } = hocket;
    await keyboardSetup(page);
    await playFirstAlbumByKeyboard(page);
    // Pause so positions hold still.
    await tabTo(page, byTestId("play-pause"), "Play/Pause");
    await page.keyboard.press("Enter");
    await expect(page.getByTestId("play-pause")).toHaveAttribute("aria-label", "Play");
    // The queue: one stop, the active option follows the arrows.
    await tabTo(page, byTestId("queue-timeline"), "the queue", { back: true });
    const list = page.getByTestId("queue-timeline");
    const currentId = await page.getByTestId("queue-row-current").getAttribute("id");
    await expect(list).toHaveAttribute("aria-activedescendant", currentId!);
    await expect(page.getByTestId("queue-row-current")).toHaveAttribute("aria-current", "true");
    await page.keyboard.press("ArrowDown");
    const firstUp = page.getByTestId("queue-row-upcoming").first();
    const moving = await firstUp.locator(".t1").textContent();
    await expect(list).toHaveAttribute("aria-activedescendant", (await firstUp.getAttribute("id"))!);
    // Alt+Down moves it one place later (the keyboard's drag and drop), and the cursor follows it.
    await page.keyboard.press("Alt+ArrowDown");
    const second = page.getByTestId("queue-row-upcoming").nth(1);
    await expect(second.locator(".t1")).toHaveText(moving!);
    await expect.poll(async () => (await list.getAttribute("aria-activedescendant")) === (await second.getAttribute("id"))).toBe(true);
    await page.keyboard.press("Alt+ArrowUp");
    await expect(page.getByTestId("queue-row-upcoming").first().locator(".t1")).toHaveText(moving!);
    // The queue's name opens the Queues page with focus on its back arrow; back returns focus to the name.
    await tabTo(page, byTestId("queue-selector"), "the queue's name", { back: true });
    await page.keyboard.press("Enter");
    await expect(page.getByTestId("right-panel")).toHaveAttribute("data-view", "recent");
    await expect(page.getByTestId("sidebar-back")).toBeFocused();
    await page.keyboard.press("Enter");
    await expect(page.getByTestId("right-panel")).toHaveAttribute("data-view", "queue");
    await expect(page.getByTestId("queue-selector")).toBeFocused();
    // The side panel's width: a separator the arrows move.
    await tabTo(page, byTestId("side-panel-resize"), "the side panel splitter", { back: true });
    const width = Number(await page.getByTestId("side-panel-resize").getAttribute("aria-valuenow"));
    await page.keyboard.press("ArrowLeft");
    await expect(page.getByTestId("side-panel-resize")).toHaveAttribute("aria-valuenow", String(width + 16));
    // Back from the queue to the overview, and its cards' links drill in again.
    await tabTo(page, byTestId("sidebar-back"), "the side panel's back arrow");
    await page.keyboard.press("Enter");
    await expect(page.getByTestId("right-panel")).toHaveAttribute("data-view", "overview");
    // Focus lands on the link that went in.
    await expect(page.getByTestId("sidebar-upnext-card-link")).toBeFocused();
    await page.keyboard.press("Enter");
    await expect(page.getByTestId("right-panel")).toHaveAttribute("data-view", "queue");
    await expect(page.getByTestId("sidebar-back")).toBeFocused();
    // Seek: arrows move 5 s, the value is spoken in words.
    await tabTo(page, byTestId("seek-slider"), "the seek slider");
    const seek = page.getByTestId("seek-slider");
    const before = Number(await seek.getAttribute("aria-valuenow"));
    await page.keyboard.press("ArrowRight");
    await expect(seek).toHaveAttribute("aria-valuenow", String(before + 5));
    await expect(seek).toHaveAttribute("aria-valuetext", /^\d+ seconds? of \d+ minutes?( \d+ seconds?)?$|^\d+ minutes?.* of /);
    await page.keyboard.press("End");
    await expect(seek).toHaveAttribute("aria-valuenow", (await seek.getAttribute("aria-valuemax"))!);
    await page.keyboard.press("Home");
    await expect(seek).toHaveAttribute("aria-valuenow", "0");
    // Volume: arrows change the volume by 5 %, never seek.
    await tabTo(page, byTestId("volume"), "the volume slider");
    const volume = page.getByTestId("volume");
    const v0 = Number(await volume.inputValue());
    await page.keyboard.press("ArrowLeft");
    await expect.poll(async () => Math.round(Number(await volume.inputValue()) * 100)).toBe(Math.round(Math.max(0, v0 - 0.05) * 100));
    await expect(volume).toHaveAttribute("aria-valuetext", `${Math.round(Math.max(0, v0 - 0.05) * 100)}%`);
    await expect(seek).toHaveAttribute("aria-valuenow", "0");
    await page.keyboard.press("ArrowUp");
    await expect.poll(async () => Math.round(Number(await volume.inputValue()) * 100)).toBe(Math.round(v0 * 100));
  });

  test("fullscreen and the palette: focus moves in, Tab is trapped, Escape closes and focus returns", async ({ hocket }) => {
    const { page } = hocket;
    await keyboardSetup(page);
    await playFirstAlbumByKeyboard(page);
    await tabTo(page, byTestId("nav-library"), "Library", { back: true });
    await page.keyboard.press("f");
    const fs = page.getByTestId("fullscreen-player");
    await expect(fs).toBeVisible();
    await expect(fs).toHaveAttribute("role", "dialog");
    await expect(page.getByTestId("fullscreen-exit")).toBeFocused();
    for (let i = 0; i < 30; i++) {
      await page.keyboard.press("Tab");
      const inside = await page.evaluate(() => !!document.activeElement?.closest('[data-testid="fullscreen-player"]'));
      expect(inside, `Tab #${i}`).toBe(true);
      expect(hasRing(await focused(page)), `ring at Tab #${i}`).toBe(true);
    }
    // The Lyrics / Queue / About toggles are buttons. It opens on the queue (the first time):
    // Enter on its toggle brings the artwork back alone, again shows the queue.
    await tabTo(page, byTestId("fs-mode-queue"), "the Queue toggle");
    await expect(page.getByTestId("fs-mode-queue")).toHaveAttribute("aria-pressed", "true");
    await page.keyboard.press("Enter");
    await expect(page.getByTestId("fs-mode-queue")).toHaveAttribute("aria-pressed", "false");
    await expect(fs.getByTestId("np-artwork")).toBeVisible();
    await expect(page.getByTestId("fs-mode-queue")).toBeFocused();
    await page.keyboard.press("Enter");
    await expect(page.getByTestId("fs-mode-queue")).toHaveAttribute("aria-pressed", "true");
    await expect(fs.getByTestId("queue-timeline")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(fs).toHaveCount(0);
    await expect(page.getByTestId("nav-library")).toBeFocused();
    // The palette.
    await page.keyboard.press("Control+k");
    await expect(page.getByTestId("palette-input")).toBeFocused();
    await page.keyboard.type("s");
    await expect(page.getByTestId("palette-item").nth(2)).toBeVisible();
    // Library results arrive after the actions; let the list settle before moving.
    await page.waitForTimeout(400);
    await page.keyboard.press("ArrowDown");
    await expect(page.getByTestId("palette-input")).toHaveAttribute("aria-activedescendant", "pal-opt-1");
    await page.keyboard.press("Tab");
    expect(await page.evaluate(() => !!document.activeElement?.closest('[data-testid="palette"]'))).toBe(true);
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("palette")).toHaveCount(0);
    await expect(page.getByTestId("nav-library")).toBeFocused();
  });
  test("filter rule editor: each chip group is one Tab stop with arrows inside, Enter and Remove rule keep focus on the rules", async ({ hocket }) => {
    const { page } = hocket;
    await keyboardSetup(page);
    await openPlace(page, "filters");
    await page.getByTestId("new-filter").click();
    const editor = page.getByTestId("rule-editor");
    await expect(editor).toBeVisible();
    // It opens on the chosen field; the arrows move the choice along the chips.
    const fields = editor.getByRole("radiogroup", { name: "Field" });
    await expect(fields.getByRole("radio", { checked: true })).toBeFocused();
    const before = (await fields.getByRole("radio", { checked: true }).textContent()) ?? "";
    await page.keyboard.press("ArrowRight");
    const after = fields.getByRole("radio", { checked: true });
    await expect(after).not.toHaveText(before);
    await expect(after).toBeFocused();
    // Tab leaves the field chips for the condition chips (one stop), then the value.
    await page.keyboard.press("Tab");
    await expect(editor.getByRole("radiogroup", { name: "Condition" }).getByRole("radio", { checked: true })).toBeFocused();
    await page.keyboard.press("Tab");
    await expect(editor.getByTestId("rule-value")).toBeFocused();
    await page.keyboard.type("Ambient");
    // Enter closes the editor onto its rule.
    await page.keyboard.press("Enter");
    await expect(editor).toHaveCount(0);
    await expect(page.getByTestId("rule-edit").first()).toBeFocused();
    // A new rule's Remove rule leaves focus on the rule before it, not the page.
    await page.getByTestId("add-rule").click();
    await expect(editor).toBeVisible();
    await editor.getByRole("button", { name: "Remove rule" }).click();
    await expect(editor).toHaveCount(0);
    await expect(page.getByTestId("rule")).toHaveCount(1);
    await expect(page.getByTestId("rule-edit").first()).toBeFocused();
  });
});
