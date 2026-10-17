// Edit pins: the side panel opened from a pin's menu (or the sidebar's pins). Its chips, unpinning with
// Undo and reordering (keyboard and drag) change a draft only; Done saves it, Cancel and Escape drop it.
import type { Page } from "@playwright/test";
import { completeSetup, expect, test } from "./fixtures";

async function seedPins(page: Page, hidden: object[] = []): Promise<string> {
  const genres = (await page.evaluate(async () => {
    const s = (await window.hocket.query({ type: "servers" })) as { data: { id: string }[] };
    return window.hocket.query({ type: "genres", data: { server_id: s.data[0]!.id } });
  })) as { data?: { name: string }[] };
  const genre = genres.data?.[0]?.name ?? "Ambient";
  const pins = [
    { kind: "playlist", id: "pl-1", tap: "open" },
    { kind: "album", id: "al-2", tap: "shuffle" },
    { kind: "artist", id: "ar-1", tap: "open" },
    { kind: "genre", id: genre, tap: "open" },
    { kind: "track", id: "tr-1", tap: "play" },
  ];
  await page.evaluate((value) => window.hocket.dispatch({ type: "setSetting", data: { key: "library.pins", value: JSON.stringify(value) } }), [...pins, ...hidden]);
  await expect(page.getByTestId("sidebar-pin")).toHaveCount(pins.length);
  return genre;
}

async function storedPins(page: Page): Promise<{ kind: string; id: string; tap: string }[]> {
  const r = (await page.evaluate(() => window.hocket.query({ type: "setting", data: { key: "library.pins" } }))) as { data?: { value: string } };
  return r.data?.value ? JSON.parse(r.data.value) : [];
}

const sidebarTitles = (page: Page) => page.getByTestId("sidebar-pin").locator(".pin-title").allTextContents();

test.describe("edit pins", () => {
  test("opens from a pin's menu; chips, unpin + Undo and keyboard reorder edit a draft that Done saves", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    const genre = await seedPins(page);
    const before = await sidebarTitles(page);
    const pin = page.getByTestId("sidebar-pin").first();
    await pin.click({ button: "right" });
    const menu = page.getByTestId("context-menu");
    const item = menu.getByRole("menuitem", { name: "Edit pins…" });
    await expect(item).toBeVisible();
    await expect(item.locator(".cm-icon svg")).toHaveCount(1);
    await item.click();

    const panel = page.getByTestId("edit-pins-panel");
    await expect(panel).toBeVisible();
    await expect(panel.getByRole("heading", { name: "Edit pins" })).toBeFocused();
    await expect(panel).toContainText("Drag to reorder. Click the chip to choose what a click does.");
    const tiles = panel.getByTestId("edit-pin");
    await expect(tiles).toHaveCount(5);
    await expect(panel.getByTestId("edit-pins-count")).toHaveText("5 pins · Pin more from any item's menu");
    // Every kind but the song has a chip; the genre's too.
    await expect(panel.getByTestId("edit-pin-chip")).toHaveCount(4);
    await expect(tiles.filter({ hasText: genre }).getByTestId("edit-pin-chip")).toHaveText(/Opens/);

    // The genre's chip: Opens → Plays → Shuffles.
    const genreChip = tiles.filter({ hasText: genre }).getByTestId("edit-pin-chip");
    await genreChip.click();
    await expect(genreChip).toHaveAttribute("data-tap", "play");
    await expect(genreChip).toHaveAccessibleName(`Click plays ${genre}. Change`);
    await genreChip.click();
    await expect(genreChip).toHaveAttribute("data-tap", "shuffle");
    await expect(genreChip).toHaveText(/Shuffles/);
    // Nothing is saved yet.
    expect((await storedPins(page)).find((p) => p.kind === "genre")?.tap).toBe("open");

    // Unpin the artist, then Undo puts it back where it was.
    const artist = before[2]!;
    await panel.getByRole("button", { name: `Unpin ${artist}` }).click();
    await expect(tiles).toHaveCount(4);
    await expect(panel.getByTestId("edit-pins-count")).toHaveText("4 pins · Pin more from any item's menu");
    const toast = page.getByTestId("toast").filter({ hasText: `Unpinned ${artist}` });
    await expect(toast).toBeVisible();
    await expect(page.getByTestId("sidebar-pin")).toHaveCount(5);
    await toast.getByTestId("toast-action").click();
    await expect(tiles).toHaveCount(5);
    await expect(tiles.nth(2)).toContainText(artist);

    // Keyboard reorder: pick the first tile up, one along and a row down, drop.
    const handle = panel.getByRole("button", { name: `Reorder ${before[0]}` });
    await handle.focus();
    await page.keyboard.press("Space");
    await expect(handle).toHaveAttribute("aria-pressed", "true");
    await expect(tiles.first()).toHaveClass(/lifted/);
    await page.keyboard.press("ArrowRight");
    await expect(tiles.nth(1)).toContainText(before[0]!);
    await page.keyboard.press("ArrowDown");
    await expect(tiles.nth(3)).toContainText(before[0]!);
    await expect(handle).toBeFocused();
    await expect(panel.getByRole("status")).toHaveText(`${before[0]}, position 4 of 5`);
    await page.keyboard.press("Enter");
    await expect(handle).toHaveAttribute("aria-pressed", "false");
    // Picked up again, Escape puts it back where it was picked up, and the panel stays open.
    await page.keyboard.press("Space");
    await page.keyboard.press("ArrowLeft");
    await expect(tiles.nth(2)).toContainText(before[0]!);
    await page.keyboard.press("Escape");
    await expect(tiles.nth(3)).toContainText(before[0]!);
    await expect(panel).toBeVisible();

    // The sidebar is untouched until Done.
    expect(await sidebarTitles(page)).toEqual(before);
    await panel.getByRole("button", { name: "Done" }).click();
    await expect(panel).toHaveCount(0);
    // Focus goes back to the pin whose menu opened it, now fourth.
    await expect(page.getByTestId("sidebar-pin").filter({ hasText: before[0] })).toBeFocused();
    await expect.poll(() => sidebarTitles(page)).toEqual([before[1], before[2], before[3], before[0], before[4]]);
    const saved = await storedPins(page);
    expect(saved.map((p) => p.kind)).toEqual(["album", "artist", "genre", "playlist", "track"]);
    expect(saved.find((p) => p.kind === "genre")?.tap).toBe("shuffle");
    // The genre's row now shows it shuffles.
    await expect(page.getByTestId("sidebar-pin").filter({ hasText: genre }).locator('svg[data-icon="shuffle"]')).toHaveCount(1);
  });

  test("Cancel and Escape drop the draft; dragging reorders; the sidebar's pins area opens it too", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await seedPins(page);
    const before = await sidebarTitles(page);
    const panel = page.getByTestId("edit-pins-panel");
    const tiles = panel.getByTestId("edit-pin");

    // From the sidebar's pins area (the gap above them).
    const pins = page.getByTestId("sidebar-pins");
    const box = (await pins.boundingBox())!;
    await page.mouse.click(box.x + box.width / 2, box.y + 5, { button: "right" });
    await page.getByTestId("context-menu").getByRole("menuitem", { name: "Edit pins…" }).click();
    await expect(panel).toBeVisible();

    // Drag the first tile onto the third.
    await tiles.first().dragTo(tiles.nth(2));
    await expect(tiles.nth(2)).toContainText(before[0]!);
    await panel.getByRole("button", { name: `Unpin ${before[1]}` }).click();
    await panel.getByTestId("edit-pin-chip").first().click();
    const toast = page.getByTestId("toast").filter({ hasText: `Unpinned ${before[1]}` });
    await expect(toast).toBeVisible();
    await panel.getByRole("button", { name: "Cancel editing" }).click();
    await expect(panel).toHaveCount(0);
    expect(await sidebarTitles(page)).toEqual(before);
    // Nothing was unpinned, so its toast (and Undo) goes with the panel.
    await expect(toast).toHaveCount(0);

    // Escape drops it too; unchanged, Done just closes.
    await page.getByTestId("sidebar-pin").nth(1).click({ button: "right" });
    await page.getByTestId("context-menu").getByRole("menuitem", { name: "Edit pins…" }).click();
    await expect(panel).toBeVisible();
    await panel.getByTestId("edit-pin-chip").first().click();
    await page.keyboard.press("Escape");
    await expect(panel).toHaveCount(0);
    expect((await storedPins(page))[0]!.tap).toBe("open");
    await page.getByTestId("sidebar-pin").nth(1).click({ button: "right" });
    await page.getByTestId("context-menu").getByRole("menuitem", { name: "Edit pins…" }).click();
    await panel.getByRole("button", { name: "Done" }).click();
    await expect(panel).toHaveCount(0);
    expect(await sidebarTitles(page)).toEqual(before);
  });

  test("Done merges what changed elsewhere meanwhile, keeps unknown pins, and the mouse doesn't pick up", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    const station = { kind: "station", id: "x", tap: "open", future: 1 };
    const genre = await seedPins(page, [station]);
    const before = await sidebarTitles(page);
    await page.getByTestId("sidebar-pin").first().click({ button: "right" });
    await page.getByTestId("context-menu").getByRole("menuitem", { name: "Edit pins…" }).click();
    const panel = page.getByTestId("edit-pins-panel");
    const tiles = panel.getByTestId("edit-pin");
    await expect(tiles).toHaveCount(5);
    // A mouse click on the handle doesn't pick the tile up (that's the keyboard's); dragging is the mouse's.
    const handle = panel.getByTestId("edit-pin-handle").first();
    await handle.click();
    await expect(handle).toHaveAttribute("aria-pressed", "false");
    // Reorder in the panel, and meanwhile unpin the genre from its sidebar menu.
    await handle.focus();
    await page.keyboard.press("Space");
    await page.keyboard.press("ArrowRight");
    await page.keyboard.press("Enter");
    await expect(tiles.nth(1)).toContainText(before[0]!);
    await page.getByTestId("sidebar-pin").filter({ hasText: genre }).click({ button: "right" });
    await page.getByTestId("context-menu").getByRole("menuitem", { name: "Unpin from Library" }).click();
    await expect(page.getByTestId("sidebar-pin")).toHaveCount(4);
    await panel.getByRole("button", { name: "Done" }).click();
    await expect(panel).toHaveCount(0);
    // The genre stays unpinned, the reorder is saved, and the station (unknown here) is kept as it was.
    await expect.poll(() => sidebarTitles(page)).toEqual([before[1], before[0], before[2], before[4]]);
    const saved = await storedPins(page);
    expect(saved.some((p) => p.kind === "genre")).toBe(false);
    expect(saved).toContainEqual(station);
  });

  test("a genre's pin menu offers what a click does", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    const genre = await seedPins(page);
    await page.getByTestId("sidebar-pin").filter({ hasText: genre }).click({ button: "right" });
    const menu = page.getByTestId("context-menu");
    await menu.getByRole("menuitemradio", { name: "Click plays" }).click();
    await expect(page.getByTestId("sidebar-pin").filter({ hasText: genre }).locator('svg[data-icon="play"]')).toHaveCount(1);
  });
});
