// Detail pages scroll as one page, as a native player's do: one scroller (the track table never
// scrolls on its own, however long), nothing above the songs moving as the page loads, a compact
// bar (Play and the title) fading in once the hero's actions have scrolled away with the column
// labels stuck under it, a new page starting at the top, and Back and Forward returning to where
// the page was left.
import type { Page } from "@playwright/test";
import { completeSetup, expect, openPlace, resizeWindow, test } from "./fixtures";

/** The elements in the content pane that scroll vertically and have something to scroll. */
function scrollers(page: Page): Promise<string[]> {
  return page.evaluate(() => Array.from(document.querySelectorAll<HTMLElement>("[data-testid=content] *"))
    .filter((e) => /(auto|scroll)/.test(getComputedStyle(e).overflowY) && e.scrollHeight > e.clientHeight + 1)
    .map((e) => e.dataset.testid ?? e.className));
}

test.describe("detail pages scroll as one page", () => {
  test("an album: one scroller, no shift as it loads, the compact bar, and Back restoring the scroll", async ({ hocket }) => {
    const { app, page } = hocket;
    await completeSetup(page);
    await resizeWindow(app, page, 1280, 800);
    await openPlace(page, "albums");
    const tile = page.getByTestId("grid-tile").nth(3);
    await expect(tile).toBeVisible();
    // Where the actions sit in the page, every frame from the page's first paint until it has loaded.
    await page.evaluate(() => {
      const seen: number[] = [];
      (window as unknown as { actionTops: number[] }).actionTops = seen;
      const t0 = performance.now();
      const tick = () => {
        const actions = document.querySelector<HTMLElement>("[data-testid=view-album] .detail-actions");
        if (actions) seen.push(actions.offsetTop);
        if (performance.now() - t0 < 2000) requestAnimationFrame(tick);
      };
      requestAnimationFrame(tick);
    });
    await tile.dblclick();
    const view = page.getByTestId("view-album");
    await expect(view.getByTestId("track-row").first()).toBeVisible();
    await page.waitForTimeout(2100);
    const tops = await page.evaluate(() => (window as unknown as { actionTops: number[] }).actionTops);
    expect(tops.length).toBeGreaterThan(0);
    expect(new Set(tops).size, `the actions moved while the album loaded: ${tops.join(",")}`).toBe(1);

    expect(await scrollers(page)).toEqual(["view-album"]);
    const bar = view.getByTestId("detail-bar");
    await expect(bar).not.toHaveClass(/stuck/);
    // Hidden until then: see-through, and out of the pointer's and the keyboard's way.
    await expect(bar).toHaveCSS("opacity", "0");
    await expect(bar).toHaveAttribute("inert", "");

    // Scrolled past the actions: the bar fades in over the page and the column labels stick under it.
    const box = (await page.getByTestId("content").boundingBox())!;
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    for (let i = 0; i < 8; i++) await page.mouse.wheel(0, 120);
    await expect(bar).toHaveClass(/stuck/);
    await expect(bar).toContainText(await view.locator(".detail-head h1").innerText());
    await expect(async () => {
      const [barBottom, headTop] = await view.evaluate((el) => [el.querySelector("[data-testid=detail-bar]")!.getBoundingClientRect().bottom, el.querySelector(".table-head")!.getBoundingClientRect().top]);
      expect(Math.abs(headTop - barBottom)).toBeLessThan(1);
    }).toPass();
    const left = await view.evaluate((el) => el.scrollTop);
    expect(left).toBeGreaterThan(0);

    // Another album starts at its top; Back returns to where this one was left.
    await page.getByTestId("album-artist").evaluate((el: HTMLElement) => el.click());
    await expect(page.getByTestId("view-artist")).toBeVisible();
    await expect.poll(() => page.getByTestId("view-artist").evaluate((el) => el.scrollTop)).toBe(0);
    await page.getByTestId("artist-album").first().click();
    await expect(page.getByTestId("view-album")).toBeVisible();
    await expect.poll(() => page.getByTestId("view-album").evaluate((el) => el.scrollTop)).toBe(0);
    const back = page.getByTestId("topbar").getByRole("button", { name: "Back", exact: true });
    await back.click();
    await expect(page.getByTestId("view-artist")).toBeVisible();
    await back.click();
    await expect(page.getByTestId("view-album")).toBeVisible();
    await expect.poll(() => page.getByTestId("view-album").evaluate((el) => el.scrollTop)).toBe(left);
    await expect(page.getByTestId("detail-bar")).toHaveClass(/stuck/);
  });

  test("a long playlist windows its rows against the page instead of scrolling on its own", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    const name = "Everything at once";
    await page.evaluate(async (name) => {
      const servers = (await window.hocket.query({ type: "accounts" })) as { data: { id: string }[] };
      const source = servers.data[0]!.id;
      const r = (await window.hocket.query({ type: "tracks", data: { server_id: source, filter: undefined, sort: "title", descending: false, page: { offset: 0, limit: 400 } } })) as { data: { items: { id: string; source: string }[] } };
      await window.hocket.dispatch({ type: "createPlaylist", data: { account_id: source, name, tracks: r.data.items.map((x) => ({ source: x.source, id: x.id })) } });
    }, name);
    await openPlace(page, "playlists");
    await page.getByTestId("grid-tile").filter({ hasText: name }).dblclick();
    const view = page.getByTestId("view-playlist");
    await expect(view.getByTestId("track-row").first()).toBeVisible();
    const total = Number(await view.getByTestId("playlist-tracks").getAttribute("aria-rowcount")) - 1;
    expect(total).toBeGreaterThan(300);
    expect(await scrollers(page)).toEqual(["view-playlist"]);
    // Only a window of the rows is drawn, and scrolling the page to the end draws the last.
    expect(await view.getByTestId("track-row").count()).toBeLessThan(80);
    await view.evaluate((el) => { el.scrollTop = el.scrollHeight; });
    await expect(view.locator(`[data-testid=track-row][data-index="${total - 1}"]`)).toBeVisible();
    // The last row ends where the page does (above its bottom padding), not past it.
    const [rowBottom, end] = await view.evaluate((el) => [el.querySelector("[data-testid=track-row]:last-child")!.getBoundingClientRect().bottom, el.querySelector(".table")!.getBoundingClientRect().bottom]);
    expect(Math.abs(end - rowBottom)).toBeLessThan(2);
  });
});
