// Several accounts (docs/providers.md, Phase 3): Home's shelves (Continue listening and each
// account's) order and hide like any section; the source chip shows with a second account and
// filters the library, search and playlists; a new playlist asks where it lives (or Mixed); and a
// song's details list its sources with "Link to…".
import type { Page } from "@playwright/test";
import { completeSetup, expect, openPlace, test } from "./fixtures";

async function addSecondAccount(page: Page): Promise<void> {
  await page.evaluate(() => window.hocket.dispatch({ type: "addAccount", data: { setup: { type: "navidrome", data: { url: "https://second.example/", username: "bob", password: "pw", name: "Second" } } } }));
  await expect.poll(async () => ((await page.evaluate(() => window.hocket.query({ type: "accounts" }))) as { data: unknown[] }).data.length).toBe(2);
}

test.describe("providers", () => {
  test("Home lists the core's shelves, and they hide and move like any section", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    const added = page.locator('[data-testid^="home-section-shelf:"][data-testid$="/recentlyAdded"]');
    await expect(added).toBeVisible();
    await expect(added.locator("h2")).toHaveText("Recently added");
    await expect(added.getByTestId("home-item").first()).toBeVisible();
    // Most played starts off; Customise lists every shelf.
    await expect(page.locator('[data-testid^="home-section-shelf:"][data-testid$="/mostPlayed"]')).toHaveCount(0);
    await page.getByTestId("home-customise").click();
    const panel = page.getByTestId("home-customise-panel");
    await expect(panel.locator('[data-testid^="customise-shelf:"]')).toHaveCount(4);
    await panel.locator('[data-testid^="customise-switch-shelf:"][data-testid$="/mostPlayed"]').click();
    await expect(page.locator('[data-testid^="home-section-shelf:"][data-testid$="/mostPlayed"]')).toBeVisible();
    await panel.locator('[data-testid^="customise-switch-shelf:"][data-testid$="/recentlyAdded"]').click();
    await expect(added).toHaveCount(0);
    // The layout is the synced setting.
    const stored = (await page.evaluate(() => window.hocket.query({ type: "setting", data: { key: "home.sections" } }))) as { data?: { value: string } };
    expect(stored.data?.value).toContain("/mostPlayed");
  });

  test("a second account brings the source chip: it filters the songs, and playlists offer Mixed", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await openPlace(page, "songs");
    await expect(page.getByTestId("source-chips")).toHaveCount(0);
    await addSecondAccount(page);
    const chips = page.getByTestId("source-chips");
    await expect(chips).toBeVisible();
    await expect(chips.getByRole("button")).toHaveCount(3);
    await expect(chips.getByRole("button").first()).toHaveText("All");
    const rows = page.getByTestId("track-row");
    await expect(rows.first()).toBeVisible();
    // The second account has no songs of its own.
    await page.getByTestId(/^source-chip-/).filter({ hasText: "Second" }).click();
    await expect(rows).toHaveCount(0);
    await page.getByTestId("source-chip-all").click();
    await expect(rows.first()).toBeVisible();
    // Playlists offer Mixed too.
    await openPlace(page, "playlists");
    await expect(page.getByTestId("source-chip-mixed")).toBeVisible();
    // The New playlist dialog asks where the playlist lives.
    await page.getByRole("button", { name: "New playlist" }).click();
    const dialog = page.getByTestId("dialog-newPlaylist");
    await expect(dialog).toBeVisible();
    await expect(dialog.getByTestId("new-playlist-account").locator("option")).toHaveText([/music\.example/, "Second", /Mixed/]);
    await page.keyboard.press("Escape");
  });

  test("a song's details list its sources, with Link to… for another account", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await addSecondAccount(page);
    await openPlace(page, "songs");
    const row = page.getByTestId("track-row").first();
    await row.click({ button: "right" });
    await page.getByTestId("context-menu").getByRole("menuitem", { name: "Song info" }).click();
    const sources = page.getByTestId("song-sources");
    await expect(sources).toBeVisible();
    await expect(sources.getByTestId("song-source")).toHaveCount(1);
    await expect(sources.getByTestId("song-source")).toContainText("Plays from here");
    await sources.getByTestId("song-source-link").click();
    await expect(page.getByTestId("song-link-picker")).toBeVisible();
  });
});
