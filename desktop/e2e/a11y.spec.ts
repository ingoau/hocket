// Automated accessibility audit (axe-core) of every view, panel and overlay in
// both themes, against the FakeCore (which serves syllable lyrics). Zero
// violations of any impact; see a11y-helpers.ts for the (empty) exemptions.
import type { Page } from "@playwright/test";
import { completeSetup, expect, test } from "./fixtures";
import { expectNoViolations, playShowcase, serverId, setTheme } from "./a11y-helpers";

const THEMES = ["light", "dark"] as const;

/** Something in every panel: an album queue with the syllable-lyrics track current, and a saved queue in Recent. */
async function populate(page: Page): Promise<void> {
  await page.getByTestId("nav-albums").click();
  await page.getByTestId("grid-tile").nth(1).dblclick();
  await page.getByTestId("album-play").click();
  await expect(page.getByTestId("queue-row-current")).toBeVisible();
  await playShowcase(page);
  const sid = await serverId(page);
  await page.evaluate((s) => window.hocket.dispatch({ type: "playNext", data: { server_id: s, track_ids: ["tr-2", "tr-3"] } }), sid);
  await expect(page.getByTestId("queue-row-next").first()).toBeVisible();
  // The first syllable line is active from 1.5 s.
  await expect(page.locator('[data-testid="lyrics-sr-list"] li[aria-current="true"]')).toHaveCount(1, { timeout: 10_000 });
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
    for (const theme of THEMES) {
      await setTheme(page, theme);
      for (const view of ["home", "albums", "artists", "songs", "playlists", "genres", "downloads", "filters", "stats"]) {
        await page.getByTestId(`nav-${view}`).click();
        await expect(page.getByTestId(`view-${view}`)).toBeVisible();
        await page.waitForTimeout(250);
        await expectNoViolations(page, `${theme} ${view}`);
      }
      await page.getByTestId("nav-albums").click();
      await page.getByTestId("grid-tile").first().dblclick();
      await expect(page.getByTestId("view-album")).toBeVisible();
      await expect(page.getByTestId("track-row").first()).toBeVisible();
      await expectNoViolations(page, `${theme} album detail`);
      await page.getByTestId("track-row").first().locator("a").first().click();
      await expect(page.getByTestId("view-artist")).toBeVisible();
      await page.waitForTimeout(250);
      await expectNoViolations(page, `${theme} artist detail`);
      await page.getByTestId("nav-playlists").click();
      await page.getByTestId("grid-tile").first().dblclick();
      await expect(page.getByTestId("view-playlist")).toBeVisible();
      await page.waitForTimeout(250);
      await expectNoViolations(page, `${theme} playlist detail`);
      await page.getByTestId("nav-genres").click();
      await page.getByTestId("genre-tile").first().click();
      await expect(page.getByTestId("view-albums")).toBeVisible();
      await expectNoViolations(page, `${theme} genre detail`);
      await page.getByTestId("nav-filters").click();
      await page.getByTestId("new-filter").click();
      await expect(page.getByTestId("view-filter-builder")).toBeVisible();
      await expect(page.getByTestId("filter-count")).toContainText(/\d/);
      await expectNoViolations(page, `${theme} filter builder`);
      await page.getByTestId("search-input").fill("the");
      await page.keyboard.press("Enter");
      await expect(page.getByTestId("view-search")).toBeVisible();
      await page.waitForTimeout(300);
      await expectNoViolations(page, `${theme} search results`);
      // The Recent tab of the side panel (saved queues).
      await page.getByTestId("tab-recent").click();
      await expect(page.getByTestId("saved-queues").or(page.getByTestId("saved-queues-empty"))).toBeVisible();
      await expectNoViolations(page, `${theme} recent tab`);
      await page.getByTestId("tab-queue").click();
    }
  });

  test("settings, every section, light and dark", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await page.getByTestId("nav-settings").click();
    for (const theme of THEMES) {
      await setTheme(page, theme);
      for (const section of ["general", "audio", "transcoding", "connect", "storage", "lyrics", "appearance", "customisation", "shortcuts", "backup", "diagnostics", "about"]) {
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
      // Fullscreen player: the artwork, then each pane.
      await page.getByTestId("toggle-fullscreen").click();
      await expect(page.getByTestId("fullscreen-player")).toBeVisible();
      await page.waitForTimeout(700);
      await expectNoViolations(page, `${theme} fullscreen/artwork`);
      for (const mode of ["lyrics", "queue", "about"]) {
        await page.getByTestId(`fs-mode-${mode}`).click();
        await page.waitForTimeout(700);
        await expectNoViolations(page, `${theme} fullscreen/${mode}`);
      }
      // Back to the artwork for the next theme.
      await page.getByTestId("fs-mode-about").click();
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
      await page.getByTestId("nav-songs").click();
      const row = page.getByTestId("track-row").first();
      await row.click({ button: "right" });
      await expect(page.getByTestId("context-menu")).toBeVisible();
      await expectNoViolations(page, `${theme} context menu`);
      // Add to playlist (a list dialog), from the menu.
      await page.locator('[data-testid="context-menu"] [data-action="addToPlaylist"]').click();
      await expect(page.getByTestId("dialog-addToPlaylist")).toBeVisible();
      await expectNoViolations(page, `${theme} add-to-playlist dialog`);
      await page.keyboard.press("Escape");
      // Gone, exit animation included: its "New playlist…" row would match the button below.
      await expect(page.getByTestId("dialog-addToPlaylist")).toHaveCount(0);
      // Prompt dialog (new playlist), confirmation dialog (remove server), sleep timer, Connect, track info.
      await page.getByTestId("nav-playlists").click();
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
      await page.getByTestId("nav-songs").click();
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
      await expect(page.getByTestId("queue-row-current")).toHaveCount(1);
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
