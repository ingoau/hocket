import type { Locator, Page } from "@playwright/test";
import { completeSetup, expect, html5DragEnd, html5DragOver, html5DragStart, html5Drop, openPlace, playFirstAlbum, test } from "./fixtures";

/** Titles of the queue rows of one kind, in order. */
async function titles(page: Page, kind: "group" | "rest"): Promise<string[]> {
  return page.getByTestId(`queue-row-${kind}`).locator(".t1").allTextContents();
}

/** Queue one song with Play next from the Songs list (so the group and the rest both exist). */
async function playNextSong(page: Page, index: number): Promise<string> {
  await openPlace(page, "songs");
  const row = page.getByTestId("track-row").nth(index);
  const title = (await row.locator(".title").textContent())!.trim();
  await row.click({ button: "right" });
  await page.getByRole("menuitem", { name: /play next/i }).click();
  await expect(page.getByTestId("queue-row-group").first()).toContainText(title);
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
    // The player comes back; the dropped song is right after the current one (Play next).
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
    await expect(page.getByTestId("queue-row-group")).toHaveCount(0);
  });

  test("a row dragged above the Next from heading joins the group, and back below leaves it", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playFirstAlbum(page);
    const queued = await playNextSong(page, 4);
    const rest = await titles(page, "rest");
    const drag = async (from: Locator, to: Locator, dy: number) => {
      const a = (await from.locator(".text").boundingBox())!;
      const b = (await to.boundingBox())!;
      await page.mouse.move(a.x + a.width / 2, a.y + a.height / 2);
      await page.mouse.down();
      await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2 + dy, { steps: 16 });
      await page.mouse.up();
    };
    // The second row after the group, up onto the group row: into the group.
    await drag(page.getByTestId("queue-row-rest").nth(1), page.getByTestId("queue-row-group").first(), -4);
    await expect.poll(() => titles(page, "group")).toEqual([rest[1], queued]);
    await expect(page.getByTestId("queue-timeline")).toContainText("Next in queue · 2");
    // The group's last row, down onto the first row after it: out of the group.
    await drag(page.getByTestId("queue-row-group").last(), page.getByTestId("queue-row-rest").first(), 4);
    await expect.poll(() => titles(page, "group")).toEqual([rest[1]]);
    expect((await titles(page, "rest")).slice(0, 2)).toEqual([rest[0], queued]);
  });

  test("Clear empties Next in queue and Undo brings it back; Stash moves it out too", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playFirstAlbum(page);
    const queued = await playNextSong(page, 4);
    await page.getByTestId("queue-clear-group").click();
    await expect(page.getByTestId("queue-row-group")).toHaveCount(0);
    await expect(page.getByTestId("queue-clear-group")).toHaveCount(0);
    await page.getByTestId("content").focus();
    await page.keyboard.press("Control+z");
    await expect.poll(() => titles(page, "group")).toEqual([queued]);
    await page.getByTestId("queue-stash-group").click();
    await expect(page.getByTestId("queue-row-group")).toHaveCount(0);
  });

  test("a queue item reorders when dragged from the row, not only the grip", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playFirstAlbum(page);
    const first = page.getByTestId("queue-row-rest").first();
    const second = page.getByTestId("queue-row-rest").nth(1);
    await expect(second).toBeVisible();
    const moving = await first.locator(".t1").textContent();
    // From the title, not the grip on the trailing edge.
    const a = (await first.locator(".text").boundingBox())!;
    const b = (await second.locator(".text").boundingBox())!;
    await page.mouse.move(a.x + a.width / 2, a.y + a.height / 2);
    await page.mouse.down();
    await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2, { steps: 16 });
    await page.mouse.up();
    await expect(page.getByTestId("queue-row-rest").nth(1).locator(".t1")).toHaveText(moving!);
  });

  test("a song dropped on the first row after the group goes there, out of the group, under one line", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playFirstAlbum(page);
    const queued = await playNextSong(page, 4);
    const restBefore = await titles(page, "rest");
    const source = page.getByTestId("track-row").nth(7);
    const title = (await source.locator(".title").textContent())!.trim();

    await html5DragStart(page, source);
    await html5DragOver(page, page.getByTestId("queue-row-rest").first());
    // The line sits above the first row under "Next from", and nowhere else.
    await expect(page.locator(".qrow[data-drop]")).toHaveCount(1);
    await expect(page.getByTestId("queue-row-rest").first()).toHaveAttribute("data-drop", "before");
    await html5Drop(page, page.getByTestId("queue-row-rest").first());

    await expect(page.getByTestId("queue-row-rest").first()).toContainText(title);
    expect(await titles(page, "group")).toEqual([queued]);
    expect((await titles(page, "rest")).slice(1)).toEqual(restBefore);
    // Added outside the group: the row carries the queue icon; the group's rows don't.
    await expect(page.getByTestId("queue-row-rest").first().getByTestId("queue-mark-added")).toHaveCount(1);
    await expect(page.getByTestId("queue-row-group").first().getByTestId("queue-mark-added")).toHaveCount(0);
  });

  test("a song dropped on the top of the Next from heading joins the end of Next in queue", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playFirstAlbum(page);
    const queued = await playNextSong(page, 4);
    const restBefore = await titles(page, "rest");
    const source = page.getByTestId("track-row").nth(7);
    const title = (await source.locator(".title").textContent())!.trim();

    await html5DragStart(page, source);
    await html5DragOver(page, page.getByTestId("queue-heading-rest"));
    await expect(page.locator(".qrow[data-drop]")).toHaveCount(1);
    await expect(page.getByTestId("queue-row-group").last()).toHaveAttribute("data-drop", "after");
    await html5Drop(page, page.getByTestId("queue-heading-rest"));

    await expect.poll(() => titles(page, "group")).toEqual([queued, title]);
    expect(await titles(page, "rest")).toEqual(restBefore);
  });

  test("the insertion line goes away when a drag ends without a drop on the queue", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playFirstAlbum(page);
    await openPlace(page, "songs");
    await html5DragStart(page, page.getByTestId("track-row").nth(3));
    await html5DragOver(page, page.getByTestId("queue-row-rest").nth(2));
    await expect(page.locator(".qrow[data-drop]")).toHaveCount(1);
    await html5DragEnd(page);
    await expect(page.locator(".qrow[data-drop]")).toHaveCount(0);
  });

  test("dragging one row of a selection moves the whole selection", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playFirstAlbum(page);
    const before = await titles(page, "rest");
    const rows = page.getByTestId("queue-row-rest");
    await rows.nth(0).click();
    await rows.nth(1).click({ modifiers: ["Shift"] });
    await expect(page.locator(".qrow.rest.selected")).toHaveCount(2);
    // Drag the first selected row down onto the fourth row.
    const a = (await rows.nth(0).locator(".text").boundingBox())!;
    const b = (await rows.nth(3).locator(".text").boundingBox())!;
    await page.mouse.move(a.x + a.width / 2, a.y + a.height / 2);
    await page.mouse.down();
    await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2, { steps: 16 });
    await expect(page.locator(".qrow.rest.dragging")).toHaveCount(2);
    await page.mouse.up();
    // [s0, s1, x2, x3, ...] → [x2, x3, s0, s1, ...]: together, in their order, after the row dropped on.
    await expect.poll(() => titles(page, "rest")).toEqual([before[2], before[3], before[0], before[1], ...before.slice(4)]);
  });

  test("a selection dragged up into Next in queue lands there together, in the group", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playFirstAlbum(page);
    const queued = await playNextSong(page, 4);
    const rest = await titles(page, "rest");
    // Select two rows after the group; drag one up onto the group row.
    const next = page.getByTestId("queue-row-group").first();
    const rows = page.getByTestId("queue-row-rest");
    await rows.nth(1).click();
    await rows.nth(2).click({ modifiers: ["Shift"] });
    const a = (await rows.nth(2).locator(".text").boundingBox())!;
    const b = (await next.locator(".text").boundingBox())!;
    await page.mouse.move(a.x + a.width / 2, a.y + a.height / 2);
    await page.mouse.down();
    await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2 - 4, { steps: 16 });
    await page.mouse.up();
    await expect.poll(() => titles(page, "group")).toEqual([rest[1], rest[2], queued]);
    expect(await titles(page, "rest")).toEqual([rest[0], ...rest.slice(3)]);
  });
});
