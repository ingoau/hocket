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
    // From the title, not the grip on the trailing edge (the shared HTML5 drag, lib/drag.ts),
    // dropped in the lower half of the second row: the gap after it.
    await html5DragStart(page, first.locator(".text"));
    await page.evaluate((el) => {
      const dt = (window as unknown as { __hocketLibraryDrag?: DataTransfer }).__hocketLibraryDrag;
      const r = el!.getBoundingClientRect();
      const init: DragEventInit = { bubbles: true, cancelable: true, composed: true, dataTransfer: dt, clientX: r.left + r.width / 2, clientY: r.bottom - 4 };
      el!.dispatchEvent(new DragEvent("dragover", init));
    }, await second.elementHandle());
    // The line is drawn at the top of the row below the gap.
    await expect(page.getByTestId("queue-row-upcoming").nth(2)).toHaveAttribute("data-drop", "before");
    await html5Drop(page, second);
    await expect(page.getByTestId("queue-row-upcoming").nth(1).locator(".t1")).toHaveText(moving!);
  });
});
