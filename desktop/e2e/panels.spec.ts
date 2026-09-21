import { completeSetup, expect, test } from "./fixtures";

test.describe("panels", () => {
  test("the queue/lyrics divider position is remembered across restarts", async ({ hocket }) => {
    const { page, app } = hocket;
    await completeSetup(page);
    const divider = page.getByTestId("panel-divider");
    await expect(divider).toBeVisible();
    const before = Number(await divider.getAttribute("aria-valuenow"));
    const box = (await divider.boundingBox())!;
    const panel = (await page.getByTestId("right-panel").boundingBox())!;
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.down();
    await page.mouse.move(box.x + box.width / 2, panel.y + panel.height * 0.25, { steps: 8 });
    await page.mouse.up();
    const after = Number(await divider.getAttribute("aria-valuenow"));
    expect(after).toBeLessThan(before);
    expect(after).toBeGreaterThanOrEqual(15);
    // Persisted in localStorage: reload the window and read it back.
    await page.reload();
    await expect(page.getByTestId("app")).toBeVisible();
    await expect(page.getByTestId("panel-divider")).toHaveAttribute("aria-valuenow", String(after));
    // Collapsing lyrics gives the queue the full height.
    await page.getByTestId("collapse-lyrics").click();
    await expect(page.getByTestId("lyrics-pane")).toHaveClass(/collapsed/);
    await expect(page.getByTestId("panel-divider")).toHaveCount(0);
    void app;
  });
});
