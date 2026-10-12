import { completeSetup, expect, test } from "./fixtures";

test.describe("panels", () => {
  test("the side panel's width and view are remembered across restarts", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    const panel = page.getByTestId("right-panel");
    await expect(panel).toHaveAttribute("data-view", "overview");
    // Drag its left edge to widen it.
    const handle = page.getByTestId("side-panel-resize");
    const before = Number(await handle.getAttribute("aria-valuenow"));
    const box = (await handle.boundingBox())!;
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.down();
    await page.mouse.move(box.x + box.width / 2 - 60, box.y + box.height / 2, { steps: 8 });
    await page.mouse.up();
    const after = Number(await handle.getAttribute("aria-valuenow"));
    expect(after).toBeGreaterThan(before);
    // The pane follows (its column is that wide, less the gutter before it).
    await expect.poll(async () => Math.round((await panel.boundingBox())!.width)).toBe(after - 8);
    // The keyboard moves it too.
    await handle.focus();
    await page.keyboard.press("ArrowRight");
    await expect(handle).toHaveAttribute("aria-valuenow", String(after - 16));
    await page.getByTestId("toggle-lyrics").click();
    await expect(panel).toHaveAttribute("data-view", "lyrics");
    // Persisted in localStorage: reload the window and read it back. No divider any more: one view has the height.
    await page.reload();
    await expect(page.getByTestId("app")).toBeVisible();
    await expect(page.getByTestId("right-panel")).toHaveAttribute("data-view", "lyrics");
    await expect(page.getByTestId("side-panel-resize")).toHaveAttribute("aria-valuenow", String(after - 16));
    await expect(page.getByTestId("panel-divider")).toHaveCount(0);
    // Closed is remembered too.
    await page.getByTestId("toggle-side-panel").click();
    await expect(page.getByTestId("right-panel")).toHaveCount(0);
    await page.reload();
    await expect(page.getByTestId("app")).toBeVisible();
    await expect(page.getByTestId("right-panel")).toHaveCount(0);
  });
});
