import type { Page } from "@playwright/test";
import { completeSetup, expect, html5DragEnd, html5DragOver, html5DragStart, html5Drop, openPlace, playFirstAlbum, test } from "./fixtures";

/** Titles of the queue rows of one kind, in order. */
async function titles(page: Page, kind: "next" | "upcoming"): Promise<string[]> {
  return page.getByTestId(`queue-row-${kind}`).locator(".t1").allTextContents();
}

/** Queue one song with Play next from the Songs list (so both sections exist). */
async function playNextSong(page: Page, index: number): Promise<string> {
  await openPlace(page, "songs");
  const row = page.getByTestId("track-row").nth(index);
  const title = (await row.locator(".title").textContent())!.trim();
  await row.click({ button: "right" });
  await page.getByRole("menuitem", { name: /play next/i }).click();
  await expect(page.getByTestId("queue-row-next").first()).toContainText(title);
  return title;
}

test.describe("dragging onto the queue", () => {
  test("dragging a song over the player shows the queue, drop adds it, then the player comes back", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await openPlace(page, "albums");
    await page.getByTestId("grid-tile").first().dblclick();
    await expect(page.getByTestId("view-album")).toBeVisible();
    await page.getByTestId("album-play").click();
    await expect(page.getByTestId("play-pause")).toHaveAttribute("aria-label", "Pause");
    const panel = page.getByTestId("right-panel");
    await expect(panel).toHaveAttribute("data-view", "overview");

    await openPlace(page, "songs");
    const row = page.getByTestId("track-row").nth(3);
    await expect(row).toBeVisible();
    const title = (await row.locator(".title").textContent())?.trim();
    expect(title).toBeTruthy();

    await html5DragStart(page, row);
    await html5DragOver(page, page.getByTestId("player-bar"));
    await expect(panel).toHaveAttribute("data-view", "queue");
    await expect(panel).toHaveAttribute("data-drop-reveal", "true");
    await expect(page.getByTestId("queue-row-current")).toBeVisible();

    await html5DragOver(page, page.getByTestId("queue-row-current"));
    await html5Drop(page, page.getByTestId("queue-wrap"));
    // The player comes back; the dropped song is first in Up next (Play next).
    await expect(panel).toHaveAttribute("data-view", "overview");
    await expect(panel).not.toHaveAttribute("data-drop-reveal", "true");
    await expect(page.getByTestId("sidebar-upnext-row").first()).toContainText(title!);
  });

  test("leaving the player without dropping puts the player back", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await openPlace(page, "albums");
    await page.getByTestId("grid-tile").first().dblclick();
    await page.getByTestId("album-play").click();
    await expect(page.getByTestId("play-pause")).toHaveAttribute("aria-label", "Pause");
    const panel = page.getByTestId("right-panel");
    await expect(panel).toHaveAttribute("data-view", "overview");

    await openPlace(page, "songs");
    await html5DragStart(page, page.getByTestId("track-row").nth(2));
    await html5DragOver(page, page.getByTestId("player-bar"));
    await expect(panel).toHaveAttribute("data-drop-reveal", "true");
    await html5DragOver(page, page.getByTestId("content"));
    await expect(panel).not.toHaveAttribute("data-drop-reveal", "true", { timeout: 2000 });
    await expect(panel).toHaveAttribute("data-view", "overview");
    await html5DragEnd(page);
    await expect(page.getByTestId("queue-row-next")).toHaveCount(0);
  });

  test("a queue item reorders when dragged from the row, not only the grip", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playFirstAlbum(page);
    const first = page.getByTestId("queue-row-upcoming").first();
    const second = page.getByTestId("queue-row-upcoming").nth(1);
    await expect(second).toBeVisible();
    const moving = await first.locator(".t1").textContent();
    // From the title, not the grip on the trailing edge.
    const a = (await first.locator(".text").boundingBox())!;
    const b = (await second.locator(".text").boundingBox())!;
    await page.mouse.move(a.x + a.width / 2, a.y + a.height / 2);
    await page.mouse.down();
    await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2, { steps: 16 });
    await page.mouse.up();
    await expect(page.getByTestId("queue-row-upcoming").nth(1).locator(".t1")).toHaveText(moving!);
  });

  test("a song dropped on the Continue playing header goes after Playing next, under one line", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playFirstAlbum(page);
    const queued = await playNextSong(page, 4);
    const upcomingBefore = await titles(page, "upcoming");
    const source = page.getByTestId("track-row").nth(7);
    const title = (await source.locator(".title").textContent())!.trim();

    await html5DragStart(page, source);
    await html5DragOver(page, page.locator("[data-group='upcoming'] .queue-section"));
    // The line sits above the first Continue playing row, and nowhere else.
    await expect(page.locator(".qrow[data-drop]")).toHaveCount(1);
    await expect(page.getByTestId("queue-row-upcoming").first()).toHaveAttribute("data-drop", "before");
    await html5Drop(page, page.locator("[data-group='upcoming'] .queue-section"));

    await expect(page.getByTestId("queue-row-upcoming").first()).toContainText(title);
    expect(await titles(page, "next")).toEqual([queued]);
    expect((await titles(page, "upcoming")).slice(1)).toEqual(upcomingBefore);
  });

  test("the insertion line goes away when a drag ends without a drop on the queue", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playFirstAlbum(page);
    await openPlace(page, "songs");
    await html5DragStart(page, page.getByTestId("track-row").nth(3));
    await html5DragOver(page, page.getByTestId("queue-row-upcoming").nth(2));
    await expect(page.locator(".qrow[data-drop]")).toHaveCount(1);
    await html5DragEnd(page);
    await expect(page.locator(".qrow[data-drop]")).toHaveCount(0);
  });

  test("dragging one row of a selection moves the whole selection", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playFirstAlbum(page);
    const before = await titles(page, "upcoming");
    const rows = page.getByTestId("queue-row-upcoming");
    await rows.nth(0).click();
    await rows.nth(1).click({ modifiers: ["Shift"] });
    await expect(page.locator(".qrow.upcoming.selected")).toHaveCount(2);
    // Drag the first selected row down onto the fourth row.
    const a = (await rows.nth(0).locator(".text").boundingBox())!;
    const b = (await rows.nth(3).locator(".text").boundingBox())!;
    await page.mouse.move(a.x + a.width / 2, a.y + a.height / 2);
    await page.mouse.down();
    await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2, { steps: 16 });
    await expect(page.locator(".qrow.upcoming.dragging")).toHaveCount(2);
    await page.mouse.up();
    // [s0, s1, x2, x3, ...] → [x2, x3, s0, s1, ...]: together, in their order, after the row dropped on.
    await expect.poll(() => titles(page, "upcoming")).toEqual([before[2], before[3], before[0], before[1], ...before.slice(4)]);
  });
});
