// Automated accessibility audit (axe-core) of every view, panel and overlay in
// both themes, against the FakeCore (which serves syllable lyrics). Zero
// violations of any impact; see a11y-helpers.ts for the (empty) exemptions.
import type { Page } from "@playwright/test";
import { completeSetup, expect, showSideView, test, openPlace } from "./fixtures";
import { expectNoViolations, playShowcase, serverId, setTheme } from "./a11y-helpers";

const THEMES = ["light", "dark"] as const;

/** Something in every panel: an album queue with the syllable-lyrics track current, and a saved queue in Recent. Leaves the side panel on the lyrics. */
async function populate(page: Page): Promise<void> {
  await openPlace(page, "albums");
  await page.getByTestId("grid-tile").nth(1).dblclick();
  await page.getByTestId("album-play").click();
  await showSideView(page, "queue");
  await expect(page.getByTestId("queue-row-current")).toBeVisible();
  await playShowcase(page);
  const sid = await serverId(page);
  await page.evaluate((s) => window.hocket.dispatch({ type: "playNext", data: { server_id: s, track_ids: ["tr-2", "tr-3"] } }), sid);
  await expect(page.getByTestId("queue-row-next").first()).toBeVisible();
  await showSideView(page, "lyrics");
  // The first syllable line is active from 1.5 s.
  await expect(page.locator('[data-testid="lyrics-sr-list"] li[aria-current="true"]')).toHaveCount(1, { timeout: 10_000 });
  // Repeat the showcase so the scans (which outlast it at the fake's timescale) keep seeing it.
  await page.evaluate(() => window.hocket.dispatch({ type: "setRepeat", data: { mode: "one" } }));
}

test.describe("axe: no violations anywhere", () => {
  test.setTimeout(240_000);

  test("setup screen, light and dark", async ({ hocket }) => {
    const { page } = hocket;
    for (const scheme of THEMES) {
      await page.emulateMedia({ colorScheme: scheme });
      await expect(page.getByTestId("setup")).toBeVisible();
      await expectNoViolations(page, `setup (${scheme})`);
    }
    // With an inline error showing.
    await page.getByTestId("setup-connect").click();
    await expect(page.getByTestId("setup-error")).toBeVisible();
    await expectNoViolations(page, "setup with error");
  });

  test("library views, detail views, queue, recent, lyrics and the player bar, light and dark", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await populate(page);
    // Pins of every list's kind, so the lists show their Pinned groups (Recent comes from the history).
    await page.evaluate(() => window.hocket.dispatch({ type: "setSetting", data: { key: "library.pins", value: JSON.stringify([
      { kind: "playlist", id: "pl-2", tap: "open" }, { kind: "album", id: "al-3", tap: "open" }, { kind: "artist", id: "ar-2", tap: "open" }, { kind: "track", id: "tr-4", tap: "play" },
    ]) } }));
    for (const theme of THEMES) {
      await setTheme(page, theme);
      for (const view of ["home", "library", "albums", "artists", "songs", "playlists", "genres", "downloads", "filters", "stats"] as const) {
        if (view === "home" || view === "library") await page.getByTestId(`nav-${view}`).click();
        else await openPlace(page, view);
        await expect(page.getByTestId(`view-${view}`)).toBeVisible();
        if (view === "playlists" || view === "albums" || view === "artists" || view === "songs") await expect(page.getByTestId(`${view}-pinned`)).toBeVisible();
        if (view === "playlists" || view === "albums" || view === "genres") await expect(page.getByTestId(`${view}-recent`)).toBeVisible();
        await page.waitForTimeout(250);
        await expectNoViolations(page, `${theme} ${view}`);
      }
      // The sort popover, open over its list.
      await openPlace(page, "artists");
      await page.getByTestId("artists-sort").click();
      await expect(page.getByTestId("artists-sort-menu")).toBeVisible();
      await page.waitForTimeout(250);
      await expectNoViolations(page, `${theme} sort popover`);
      await page.keyboard.press("Escape");
      await expect(page.getByTestId("artists-sort-menu")).toHaveCount(0);
      await openPlace(page, "albums");
      await page.getByTestId("grid-tile").first().dblclick();
      await expect(page.getByTestId("view-album")).toBeVisible();
      await expect(page.getByTestId("track-row").first()).toBeVisible();
      await expectNoViolations(page, `${theme} album detail`);
      await page.getByTestId("track-row").first().locator("a").first().click();
      await expect(page.getByTestId("view-artist")).toBeVisible();
      await page.waitForTimeout(250);
      await expectNoViolations(page, `${theme} artist detail`);
      await openPlace(page, "playlists");
      await page.getByTestId("grid-tile").first().dblclick();
      await expect(page.getByTestId("view-playlist")).toBeVisible();
      await page.waitForTimeout(250);
      await expectNoViolations(page, `${theme} playlist detail`);
      await openPlace(page, "genres");
      await page.getByTestId("genre-tile").first().click();
      await expect(page.getByTestId("view-albums")).toBeVisible();
      await page.waitForTimeout(250);
      await expectNoViolations(page, `${theme} genre detail`);
      await openPlace(page, "filters");
      await page.getByTestId("new-filter").click();
      await expect(page.getByTestId("view-filter-builder")).toBeVisible();
      await expect(page.getByTestId("filter-count")).toContainText(/\d/);
      await expectNoViolations(page, `${theme} filter builder`);
      await page.getByTestId("search-input").fill("the");
      await page.keyboard.press("Enter");
      await expect(page.getByTestId("view-search")).toBeVisible();
      await page.waitForTimeout(300);
      await expectNoViolations(page, `${theme} search results`);
      // Every view of the side panel: the overview, the queue and its selector, Recent queues, About.
      const panel = page.getByTestId("right-panel");
      await page.getByTestId("sidebar-back").click();
      await expect(panel).toHaveAttribute("data-view", "overview");
      await expect(page.getByTestId("sidebar-lyrics-card")).toBeVisible();
      await expectNoViolations(page, `${theme} side panel overview`);
      await page.getByTestId("sidebar-upnext-card-link").click();
      await expect(panel.getByTestId("queue-row-current")).toBeVisible();
      await expectNoViolations(page, `${theme} side panel queue`);
      await panel.getByTestId("queue-selector").click();
      await expect(page.getByTestId("queue-selector-menu")).toBeVisible();
      await expectNoViolations(page, `${theme} queue selector`);
      await page.getByTestId("queue-selector-all").click();
      await expect(page.getByTestId("saved-queues").or(page.getByTestId("saved-queues-empty"))).toBeVisible();
      await expectNoViolations(page, `${theme} recent queues`);
      await page.getByTestId("sidebar-back").click();
      await page.getByTestId("sidebar-back").click();
      await page.getByTestId("sidebar-about-card-link").click();
      await expect(panel.getByTestId("np-about")).toContainText("Duration");
      await expectNoViolations(page, `${theme} side panel about`);
      await page.getByTestId("sidebar-back").click();
      await showSideView(page, "lyrics");
      await page.getByTestId("lyrics-options").click();
      await expect(page.getByTestId("lyrics-tools")).toBeVisible();
      await expectNoViolations(page, `${theme} lyrics options`);
      await page.keyboard.press("Escape");
      await expect(page.getByTestId("lyrics-tools")).toHaveCount(0);
    }
  });

  test("settings, every section, light and dark", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await page.getByTestId("nav-settings").click();
    for (const theme of THEMES) {
      await setTheme(page, theme);
      for (const section of ["general", "audio", "transcoding", "connect", "storage", "lyrics", "appearance", "customisation", "shortcuts", "backup", "diagnostics", "advanced", "about"]) {
        await page.getByTestId(`settings-nav-${section}`).click();
        await expect(page.getByTestId(`settings-nav-${section}`)).toHaveAttribute("aria-current", "page");
        await page.waitForTimeout(200);
        await expectNoViolations(page, `${theme} settings/${section}`);
      }
    }
  });

  test("fullscreen player, palette, context menu, dialogs, toasts and popovers, light and dark", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await populate(page);
    for (const theme of THEMES) {
      await setTheme(page, theme);
      // Fullscreen player: the artwork, then each view (it opens on the one it was left on, the queue the first time).
      await page.getByTestId("toggle-fullscreen").click();
      const fs = page.getByTestId("fullscreen-player");
      await expect(fs).toBeVisible();
      const opened = await fs.getAttribute("data-mode");
      if (opened !== "art") await page.getByTestId(`fs-mode-${opened}`).click();
      await expect(fs).toHaveAttribute("data-mode", "art");
      await page.waitForTimeout(700);
      await expectNoViolations(page, `${theme} fullscreen/artwork`);
      await page.getByTestId("fs-pill-volume").click();
      await expect(page.getByTestId("fs-volume-popover")).toBeVisible();
      await expectNoViolations(page, `${theme} fullscreen/volume`);
      await page.keyboard.press("Escape");
      for (const mode of ["lyrics", "queue", "about"]) {
        await page.getByTestId(`fs-mode-${mode}`).click();
        await page.waitForTimeout(700);
        await expectNoViolations(page, `${theme} fullscreen/${mode}`);
        if (mode === "lyrics") {
          await fs.getByTestId("fs-mode-lyrics").click({ button: "right" });
          await expect(fs.getByTestId("lyrics-tools")).toBeVisible();
          await expectNoViolations(page, `${theme} fullscreen/lyrics options`);
          await page.keyboard.press("Escape");
          await expect(fs.getByTestId("lyrics-tools")).toHaveCount(0);
        }
      }
      await page.getByTestId("fs-mode-queue").click();
      await fs.getByTestId("queue-selector").click();
      await expect(page.getByTestId("queue-selector-menu")).toBeVisible();
      await expectNoViolations(page, `${theme} fullscreen/queue selector`);
      await page.keyboard.press("Escape");
      // Back to the artwork for the next theme.
      await page.getByTestId("fs-mode-queue").click();
      await page.keyboard.press("Escape");
      await expect(page.getByTestId("fullscreen-player")).toHaveCount(0);
      // Command palette with results and actions.
      await page.getByTestId("content").click();
      await page.keyboard.press("Control+k");
      await page.getByTestId("palette-input").fill("s");
      await expect(page.getByTestId("palette-item").first()).toBeVisible();
      await expectNoViolations(page, `${theme} palette`);
      await page.keyboard.press("Escape");
      // Search popover.
      await page.getByTestId("search-input").fill("a");
      await expect(page.getByTestId("search-result").first()).toBeVisible();
      await expectNoViolations(page, `${theme} search popover`);
      await page.keyboard.press("Escape");
      await page.getByTestId("search-input").fill("");
      // Jobs popover.
      await page.getByTestId("jobs-button").click();
      await expect(page.getByTestId("jobs-popover")).toBeVisible();
      await expectNoViolations(page, `${theme} jobs popover`);
      await page.getByTestId("jobs-button").click();
      // Context menu on a track row.
      await openPlace(page, "songs");
      const row = page.getByTestId("track-row").first();
      await row.click({ button: "right" });
      await expect(page.getByTestId("context-menu")).toBeVisible();
      await expectNoViolations(page, `${theme} context menu`);
      // Its Add to playlist submenu, then the same while searching and with Remove both / Remove one open.
      await page.locator('[data-testid="context-menu"] [data-action="addToPlaylist"]').click();
      await expect(page.getByTestId("playlist-submenu")).toBeVisible();
      await expect(page.getByTestId("playlist-search")).toBeFocused();
      await expectNoViolations(page, `${theme} add-to-playlist submenu`);
      await page.getByTestId("playlist-search").fill("zzz");
      await expect(page.getByTestId("playlist-submenu").getByRole("status")).toHaveText('No playlists match "zzz"');
      await expectNoViolations(page, `${theme} add-to-playlist submenu, no matches`);
      await page.keyboard.press("Escape");
      await expect(page.getByTestId("playlist-submenu")).toHaveCount(0);
      await page.keyboard.press("Escape");
      await expect(page.getByTestId("context-menu")).toHaveCount(0);
      // Add to playlist for what's playing (the player bar's +): the same picker, beside the button.
      await page.getByTestId("bar-add").click();
      await expect(page.getByTestId("playlist-submenu")).toBeVisible();
      await expectNoViolations(page, `${theme} add-to-playlist picker`);
      await page.keyboard.press("Escape");
      await expect(page.getByTestId("playlist-submenu")).toHaveCount(0);
      // Prompt dialog (new playlist), confirmation dialog (remove server), sleep timer, Connect, track info.
      await openPlace(page, "playlists");
      await page.getByRole("button", { name: "New playlist" }).click();
      await expect(page.getByTestId("dialog-prompt")).toBeVisible();
      await expectNoViolations(page, `${theme} prompt dialog`);
      await page.keyboard.press("Escape");
      await page.getByTestId("nav-settings").click();
      await page.getByTestId("settings-nav-general").click();
      await page.getByTestId("server-row").getByRole("button", { name: "Remove server" }).click();
      await expect(page.getByTestId("dialog-confirm")).toBeVisible();
      await expectNoViolations(page, `${theme} confirmation dialog`);
      await page.keyboard.press("Escape");
      await page.getByTestId("connect-button").click();
      await expect(page.getByTestId("dialog-connect")).toBeVisible();
      await expectNoViolations(page, `${theme} connect dialog`);
      await page.keyboard.press("Escape");
      await page.getByTestId("content").click();
      await page.keyboard.press("Control+k");
      await page.getByTestId("palette-input").fill("sleep timer");
      await page.getByTestId("palette-item").filter({ hasText: "Sleep timer" }).first().click();
      await expect(page.getByTestId("dialog-sleepTimer")).toBeVisible();
      await expectNoViolations(page, `${theme} sleep timer dialog`);
      await page.keyboard.press("Escape");
      await openPlace(page, "songs");
      await page.getByTestId("track-row").first().click({ button: "right" });
      await page.locator('[data-testid="context-menu"] [data-action="ui.info"]').click();
      await expect(page.getByTestId("dialog-trackInfo")).toBeVisible();
      await expect(page.getByTestId("dialog-trackInfo").locator("dl")).toBeVisible();
      await expectNoViolations(page, `${theme} track info dialog`);
      await page.keyboard.press("Escape");
      // A toast with an Undo action (only queue-replacing actions get one).
      await page.evaluate(() => window.hocket.dispatch({ type: "clearQueue" }));
      await expect(page.getByTestId("toast").first()).toBeVisible();
      await expectNoViolations(page, `${theme} toast`);
      await page.evaluate(() => window.hocket.dispatch({ type: "undo" }));
      await showSideView(page, "queue");
      await expect(page.getByTestId("queue-row-current")).toHaveCount(1);
    }
  });

  test("edit pins panel, light and dark", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    // One of each kind, the album playing and the playlist shuffling (every chip colour).
    const sid = await serverId(page);
    const genres = (await page.evaluate((s) => window.hocket.query({ type: "genres", data: { server_id: s } }), sid)) as { data?: { name: string }[] };
    const pins = [
      { kind: "album", id: "al-2", tap: "play" }, { kind: "playlist", id: "pl-1", tap: "shuffle" }, { kind: "artist", id: "ar-1", tap: "open" },
      { kind: "track", id: "tr-1", tap: "play" }, { kind: "genre", id: genres.data?.[0]?.name ?? "Ambient", tap: "open" },
    ];
    await page.evaluate((value) => window.hocket.dispatch({ type: "setSetting", data: { key: "library.pins", value: JSON.stringify(value) } }), pins);
    await expect(page.getByTestId("sidebar-pin")).toHaveCount(pins.length);
    await page.getByTestId("sidebar-pin").first().click({ button: "right" });
    await page.getByTestId("context-menu").getByRole("menuitem", { name: "Edit pins…" }).click();
    const panel = page.getByTestId("edit-pins-panel");
    await expect(panel.getByTestId("edit-pin")).toHaveCount(pins.length);
    for (const theme of THEMES) {
      await setTheme(page, theme);
      await page.waitForTimeout(400);
      await expectNoViolations(page, `${theme} edit pins`);
      // The Unpinned toast with its Undo, and a tile picked up.
      await panel.getByTestId("edit-pin-unpin").last().click();
      await expect(page.getByTestId("toast").first()).toBeVisible();
      const handle = panel.getByTestId("edit-pin-handle").first();
      await handle.focus();
      await page.keyboard.press("Space");
      await expect(handle).toHaveAttribute("aria-pressed", "true");
      await page.waitForTimeout(400);
      await expectNoViolations(page, `${theme} edit pins, unpinned and picked up`);
      await page.keyboard.press("Escape");
      await expect(handle).toHaveAttribute("aria-pressed", "false");
      await page.getByTestId("toast").first().getByTestId("toast-action").click();
      await expect(panel.getByTestId("edit-pin")).toHaveCount(pins.length);
    }
  });

  test("the always-on-top mini player, light and dark", async ({ hocket }) => {
    const { app, page } = hocket;
    await completeSetup(page);
    await playShowcase(page);
    const opened = app.waitForEvent("window", { predicate: (w) => w.url().includes("window=mini") });
    await page.getByRole("button", { name: "Mini player" }).click();
    const mini = await opened;
    await mini.waitForLoadState("domcontentloaded");
    await expect(mini.getByTestId("mini-player")).toBeVisible();
    for (const theme of THEMES) {
      await setTheme(page, theme);
      await expect(mini.locator("html")).toHaveAttribute("data-theme", theme);
      await expectNoViolations(mini, `mini player (${theme})`);
    }
    // Its seek bar is the same keyboard slider.
    await expect(mini.getByTestId("seek-slider")).toHaveAttribute("aria-valuetext", / of /);
  });
});
