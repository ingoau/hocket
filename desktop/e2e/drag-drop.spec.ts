// The shared drag and drop (lib/drag.ts): library items, queue rows (history too)
// and the song playing drag onto playlists (a sidebar pin, a playlist tile, the
// open playlist between its rows) and onto the queue. Fake library: pl-1
// "Morning coffee" is yours, pl-3 "Focus" is smart (takes nothing).
import type { Page } from "@playwright/test";
import { completeSetup, expect, html5DragEnd, html5DragOver, html5DragStart, html5Drop, openPlace, playFirstAlbum, test } from "./fixtures";

async function playlistTrackIds(page: Page, id: string): Promise<string[]> {
  const r = (await page.evaluate((pid) => window.hocket.query({ type: "playlistTracks", data: { id: pid, page: { offset: 0, limit: 10_000 } } }), id)) as { data: { items: { id: string }[] } };
  return r.data.items.map((x) => x.id);
}

async function pinPlaylist(page: Page): Promise<void> {
  await page.evaluate(() => window.hocket.dispatch({ type: "setSetting", data: { key: "library.pins", value: JSON.stringify([{ kind: "playlist", id: "pl-1", tap: "open" }, { kind: "album", id: "al-2", tap: "open" }]) } }));
  await expect(page.getByTestId("sidebar-pin")).toHaveCount(2);
}

test.describe("drag and drop onto playlists and the queue", () => {
  test("a song row dropped on a playlist's sidebar pin is added at its end", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await pinPlaylist(page);
    const before = await playlistTrackIds(page, "pl-1");
    await openPlace(page, "songs");
    const row = page.getByTestId("track-row").nth(4);
    const id = await row.getAttribute("data-track-id");
    expect(id).toBeTruthy();

    const pin = page.locator('[data-testid="sidebar-pin"][data-pin="playlist:pl-1"]');
    await html5DragStart(page, row);
    await html5DragOver(page, pin);
    // Drop feedback on the pin while the song is over it.
    await expect(pin).toHaveAttribute("data-drop-over", "");
    await html5Drop(page, pin);
    await expect(pin).not.toHaveAttribute("data-drop-over", "");
    await expect(page.getByTestId("toast").filter({ hasText: "Added to Morning coffee" })).toBeVisible();
    await expect.poll(() => playlistTrackIds(page, "pl-1")).toEqual([...before, id]);
  });

  test("an album tile dropped on a playlist tile adds the album's songs; a smart playlist takes nothing", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    const before = await playlistTrackIds(page, "pl-1");
    const album = (await page.evaluate(() => window.hocket.query({ type: "albumTracks", data: { id: "al-2" } }))) as { data: { id: string }[] };
    const albumIds = album.data.map((x) => x.id);
    expect(albumIds.length).toBeGreaterThan(0);

    // An album from the album page's head, onto the playlists grid.
    await page.evaluate(() => window.hocket.dispatch({ type: "setSetting", data: { key: "library.pins", value: JSON.stringify([{ kind: "album", id: "al-2", tap: "open" }]) } }));
    await expect(page.getByTestId("sidebar-pin")).toHaveCount(1);
    await openPlace(page, "playlists");
    const smart = page.locator('[data-testid="grid-tile"][data-id="pl-3"]');
    const mine = page.locator('[data-testid="grid-tile"][data-id="pl-1"]');
    await expect(mine).toBeVisible();

    await html5DragStart(page, page.getByTestId("sidebar-pin").first());
    // Which playlists are read-only is read when the drag starts.
    await page.waitForTimeout(300);
    await html5DragOver(page, smart);
    await expect(smart).not.toHaveAttribute("data-drop-over", "");
    await html5DragOver(page, mine);
    await expect(mine).toHaveAttribute("data-drop-over", "");
    await html5Drop(page, mine);
    await expect.poll(() => playlistTrackIds(page, "pl-1")).toEqual([...before, ...albumIds]);
  });

  test("a queue row dropped between a playlist's rows lands there", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playFirstAlbum(page);
    const qrow = page.getByTestId("queue-row-upcoming").nth(1);
    const title = (await qrow.locator(".t1").textContent())?.trim();
    expect(title).toBeTruthy();

    await openPlace(page, "playlists");
    await page.locator('[data-testid="grid-tile"][data-id="pl-1"]').dblclick();
    await expect(page.getByTestId("view-playlist")).toBeVisible();
    const before = await playlistTrackIds(page, "pl-1");
    const target = page.getByTestId("track-row").nth(2);
    await expect(target).toBeVisible();

    await html5DragStart(page, qrow);
    // The top half of the third row: the gap before it, shown as a line.
    await html5DragOver(page, target);
    await expect(target).toHaveClass(/drop-before/);
    await html5Drop(page, target);
    await expect.poll(async () => (await playlistTrackIds(page, "pl-1")).length).toBe(before.length + 1);
    await expect(page.getByTestId("track-row").nth(2).locator(".title")).toContainText(title!);
    await expect(target).not.toHaveClass(/drop-before/);
  });

  test("the song playing drags from the player bar to a playlist", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await pinPlaylist(page);
    await playFirstAlbum(page);
    const playing = (await page.evaluate(() => window.hocket.query({ type: "queue" }))) as { data: { current?: { track: { id: string } } } };
    const playingId = playing.data.current?.track.id;
    expect(playingId).toBeTruthy();
    const before = await playlistTrackIds(page, "pl-1");
    const pin = page.locator('[data-testid="sidebar-pin"][data-pin="playlist:pl-1"]');

    await html5DragStart(page, page.getByTestId("bar-now").locator(".title a"));
    await html5DragOver(page, pin);
    await expect(pin).toHaveAttribute("data-drop-over", "");
    await html5Drop(page, pin);
    await expect.poll(() => playlistTrackIds(page, "pl-1")).toEqual([...before, playingId]);
  });

  test("queue rows reorder by drag, and a history row dropped on the queue plays again next", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playFirstAlbum(page);
    const upcoming = page.getByTestId("queue-row-upcoming");
    const moving = (await upcoming.first().locator(".t1").textContent())?.trim();
    await html5DragStart(page, upcoming.first());
    await expect(upcoming.first()).toHaveClass(/dragging/);
    // The top half of the third row: the gap before it, so it ends up second.
    await html5DragOver(page, upcoming.nth(2));
    await expect(upcoming.nth(2)).toHaveAttribute("data-drop", "before");
    await html5Drop(page, upcoming.nth(2));
    await expect(upcoming.nth(1).locator(".t1")).toHaveText(moving!);
    await expect(page.locator(".qrow.dragging")).toHaveCount(0);

    // Skip one so the first song is in History, then drag it back in.
    const first = (await page.getByTestId("queue-row-current").locator(".t1").textContent())?.trim();
    await page.getByTestId("next").click();
    const history = page.getByTestId("queue-row-history").filter({ hasText: first! });
    await expect(history).toHaveCount(1);
    await html5DragStart(page, history);
    await html5DragOver(page, page.getByTestId("queue-row-current"));
    await html5Drop(page, page.getByTestId("queue-row-current"));
    await expect(page.getByTestId("queue-row-next").first().locator(".t1")).toHaveText(first!);
    // Still in History too: it was copied, not moved.
    await expect(history).toHaveCount(1);
  });

  test("a drag that ends without a drop leaves no highlight behind", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await pinPlaylist(page);
    await openPlace(page, "songs");
    const pin = page.locator('[data-testid="sidebar-pin"][data-pin="playlist:pl-1"]');
    await html5DragStart(page, page.getByTestId("track-row").first());
    await html5DragOver(page, pin);
    await expect(pin).toHaveAttribute("data-drop-over", "");
    await html5DragEnd(page);
    await expect(page.locator("[data-drop-over]")).toHaveCount(0);
  });
});
