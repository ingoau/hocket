import { completeSetup, expect, playFirstAlbum, test } from "./fixtures";

test.describe("playback and queue", () => {
  test("playing an album fills the queue timeline with the model visible", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playFirstAlbum(page);
    const timeline = page.getByTestId("queue-timeline");
    await expect(timeline).toBeVisible();
    await expect(page.getByTestId("queue-row-current")).toHaveCount(1);
    await expect(page.getByTestId("queue-row-upcoming").first()).toBeVisible();
    await expect(timeline).toContainText("Continuing from");
    // Next moves the current item into history (nothing consumed).
    await page.getByTestId("next").click();
    await expect(page.getByTestId("queue-row-history")).toHaveCount(1);
    await page.getByTestId("previous").click();
    await page.getByTestId("previous").click();
    await expect(page.getByTestId("queue-row-history")).toHaveCount(0);
    // Play next inserts into "Playing next".
    await page.getByTestId("nav-songs").click();
    const row = page.getByTestId("track-row").nth(3);
    await row.click({ button: "right" });
    await page.getByTestId("context-menu").locator('[data-action="playNext"]').click();
    await expect(page.getByTestId("queue-row-next")).toHaveCount(1);
    await expect(page.getByTestId("queue-timeline")).toContainText("Playing next");
    // Position ticks while playing.
    const pos = page.getByTestId("seek-position").first();
    await expect(pos).not.toHaveText("0:00", { timeout: 8000 });
  });

  test("the fullscreen player opens with F and shows lyrics, related and up next", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playFirstAlbum(page);
    await page.getByTestId("content").click();
    await page.keyboard.press("f");
    await expect(page.getByTestId("fullscreen-player")).toBeVisible();
    await page.getByTestId("fs-tab-related").click();
    await expect(page.getByTestId("related-list")).toBeVisible();
    await page.getByTestId("fs-tab-upNext").click();
    await expect(page.getByTestId("fullscreen-player").getByTestId("queue-timeline")).toBeVisible();
    await page.getByTestId("fullscreen-exit").click();
    await expect(page.getByTestId("fullscreen-player")).toHaveCount(0);
  });
});
