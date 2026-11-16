// The shared drag and drop (lib/drag.ts): library items, queue rows (history by
// HTML5 drag, upcoming rows by their dnd-kit drag) and the song playing drop onto
// playlists (a pin, a Your library entry, a playlist tile, the open playlist
// between its rows) and onto the queue. Fake library: pl-1 "Morning coffee" is
// yours, pl-3 "Focus" is smart (takes nothing).
import type { Page } from "@playwright/test";
import { completeSetup, expect, html5DragEnd, html5DragOver, html5DragStart, html5Drop, openPlace, playFirstAlbum, showSideView, test } from "./fixtures";

async function playlistTrackIds(page: Page, id: string): Promise<string[]> {
  const r = (await page.evaluate((pid) => window.hocket.query({ type: "playlistTracks", data: { id: pid, page: { offset: 0, limit: 10_000 } } }), id)) as { data: { items: { id: string }[] } };
  return r.data.items.map((x) => x.id);
}

async function pinPlaylist(page: Page): Promise<void> {
  await page.evaluate(() => window.hocket.dispatch({ type: "setSetting", data: { key: "library.pins", value: JSON.stringify([{ kind: "playlist", id: "pl-1", tap: "open" }, { kind: "album", id: "al-2", tap: "open" }]) } }));
  await expect(page.getByTestId("sidebar-pin")).toHaveCount(2);
}

async function currentTrackId(page: Page): Promise<string> {
  const s = (await page.evaluate(() => window.hocket.query({ type: "snapshot" }))) as { data: { queue: { current?: { track: { id: string } } } } };
  const id = s.data.queue.current?.track.id;
  expect(id).toBeTruthy();
  return id!;
}

const pin = (page: Page) => page.locator('[data-testid="sidebar-pin"][data-pin="playlist:pl-1"]');

test.describe("drag and drop onto playlists and the queue", () => {
  test("a song row dropped on a playlist's pin is added at its end, with the drop highlighted", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await pinPlaylist(page);
    const before = await playlistTrackIds(page, "pl-1");
    await openPlace(page, "songs");
    const row = page.getByTestId("track-row").nth(4);
    const id = await row.getAttribute("data-track-id");
    expect(id).toBeTruthy();

    await html5DragStart(page, row);
    await html5DragOver(page, pin(page));
    await expect(pin(page)).toHaveAttribute("data-drop-over", "");
    await html5Drop(page, pin(page));
    await expect(pin(page)).not.toHaveAttribute("data-drop-over", "");
    await expect(page.getByTestId("toast").filter({ hasText: "Added to Morning coffee" })).toBeVisible();
    await expect.poll(() => playlistTrackIds(page, "pl-1")).toEqual([...before, id]);
  });

  test("an album pin dropped on a playlist tile adds the album's songs; a smart playlist takes nothing", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await pinPlaylist(page);
    const before = await playlistTrackIds(page, "pl-1");
    const album = (await page.evaluate(() => window.hocket.query({ type: "albumTracks", data: { id: "al-2" } }))) as { data: { id: string }[] };
    const albumIds = album.data.map((x) => x.id);
    expect(albumIds.length).toBeGreaterThan(0);

    await openPlace(page, "playlists");
    const smart = page.locator('[data-testid="grid-tile"][data-id="pl-3"]');
    const mine = page.locator('[data-testid="grid-tile"][data-id="pl-1"]');
    // Clear of the player bar, which takes a drop itself.
    await mine.scrollIntoViewIfNeeded();
    await smart.scrollIntoViewIfNeeded();
    await expect(mine).toBeVisible();

    await html5DragStart(page, page.locator('[data-testid="sidebar-pin"][data-pin="album:al-2"]'));
    await html5DragOver(page, smart);
    await expect(smart).not.toHaveAttribute("data-drop-over", "");
    await html5DragOver(page, mine);
    await expect(mine).toHaveAttribute("data-drop-over", "");
    await html5Drop(page, mine);
    await expect.poll(() => playlistTrackIds(page, "pl-1")).toEqual([...before, ...albumIds]);
  });

  test("the song playing drops between an open playlist's rows", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playFirstAlbum(page);
    const playing = await currentTrackId(page);
    await openPlace(page, "playlists");
    await page.locator('[data-testid="grid-tile"][data-id="pl-1"]').dblclick();
    await expect(page.getByTestId("view-playlist")).toBeVisible();
    const before = await playlistTrackIds(page, "pl-1");
    const target = page.getByTestId("track-row").nth(2);
    await target.scrollIntoViewIfNeeded();
    await expect(target).toBeVisible();

    await html5DragStart(page, page.getByTestId("bar-now"));
    // The top half of the third row: the gap before it, shown as a line.
    await html5DragOver(page, target);
    await expect(target).toHaveClass(/drop-before/);
    await html5Drop(page, target);
    await expect.poll(() => playlistTrackIds(page, "pl-1")).toEqual([...before.slice(0, 2), playing, ...before.slice(2)]);
    await expect(page.locator(".tr.drop-before, .tr.drop-after")).toHaveCount(0);
  });

  test("a queue row dragged out of the queue onto a playlist entry in the sidebar is added there", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playFirstAlbum(page);
    // Your library, playlists only: the entry is near the top.
    await page.getByTestId("side-lib-chip-playlist").click();
    const entry = page.locator('[data-testid="sidebar-library-item"][data-entry="playlist:pl-1"]');
    await entry.scrollIntoViewIfNeeded();
    await expect(entry).toBeVisible();
    const before = await playlistTrackIds(page, "pl-1");
    const row = page.getByTestId("queue-row-rest").nth(1);
    const key = await row.getAttribute("data-key");
    const s = (await page.evaluate(() => window.hocket.query({ type: "snapshot" }))) as { data: { queue: { entries: { item: { key: string; track: { id: string } } }[] } } };
    const id = s.data.queue.entries.find((e) => e.item.key === key)?.item.track.id;
    expect(id).toBeTruthy();
    const queueOrder = await page.getByTestId("queue-row-rest").evaluateAll((els) => els.map((el) => el.getAttribute("data-key")));

    // The upcoming rows reorder with a pointer drag; out over a playlist it adds instead.
    const a = (await row.locator(".text").boundingBox())!;
    const b = (await entry.boundingBox())!;
    await page.mouse.move(a.x + a.width / 2, a.y + a.height / 2);
    await page.mouse.down();
    await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2, { steps: 20 });
    await expect(entry).toHaveAttribute("data-drop-over", "");
    await expect(page.locator(".drag-chip")).toBeVisible();
    await page.mouse.up();
    await expect.poll(() => playlistTrackIds(page, "pl-1")).toEqual([...before, id]);
    await expect(page.locator("[data-drop-over]")).toHaveCount(0);
    // The queue itself is as it was.
    expect(await page.getByTestId("queue-row-rest").evaluateAll((els) => els.map((el) => el.getAttribute("data-key")))).toEqual(queueOrder);
  });

  test("a history row dropped on the queue plays again, and stays in History", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playFirstAlbum(page);
    const first = (await page.getByTestId("queue-row-current").locator(".t1").textContent())?.trim();
    await page.getByTestId("next").click();
    await showSideView(page, "queue");
    const history = page.getByTestId("queue-row-history").filter({ hasText: first! });
    await expect(history).toHaveCount(1);
    await html5DragStart(page, history);
    await html5DragOver(page, page.getByTestId("queue-row-current"));
    await html5Drop(page, page.getByTestId("queue-row-current"));
    await expect(page.locator(".qrow.group, .qrow.rest").first().locator(".t1")).toHaveText(first!);
    await expect(history).toHaveCount(1);
  });

  test("a drag that ends without a drop leaves no highlight behind", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await pinPlaylist(page);
    await openPlace(page, "songs");
    await html5DragStart(page, page.getByTestId("track-row").first());
    await html5DragOver(page, pin(page));
    await expect(pin(page)).toHaveAttribute("data-drop-over", "");
    await html5DragEnd(page);
    await expect(page.locator("[data-drop-over]")).toHaveCount(0);
  });
});
