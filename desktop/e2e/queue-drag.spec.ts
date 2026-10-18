import { completeSetup, expect, html5DragEnd, html5DragOver, html5DragStart, html5Drop, openPlace, playFirstAlbum, test } from "./fixtures";

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
});
