// Desktop feel: the content pane is laid out from the sidebar width (dragging
// the splitter moves it), chrome is not selectable, nothing is underlined or
// pointer-cursored, the UI font is the 13px system stack, and focus rings
// appear only for keyboard focus.
import { completeSetup, expect, test } from "./fixtures";

test.describe("window chrome", () => {
  test("dragging the sidebar splitter moves the content pane and persists", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    const handle = page.getByTestId("sidebar-resize");
    const content = page.getByTestId("content");
    const sidebar = page.getByTestId("sidebar");
    const before = (await content.boundingBox())!;
    const sb = (await sidebar.boundingBox())!;
    expect(Math.round(before.x)).toBe(Math.round(sb.x + sb.width));
    const h = (await handle.boundingBox())!;
    await page.mouse.move(h.x + h.width / 2, h.y + h.height / 2);
    await page.mouse.down();
    await page.mouse.move(h.x + h.width / 2 + 90, h.y + h.height / 2, { steps: 6 });
    await page.mouse.up();
    const after = (await content.boundingBox())!;
    const sbAfter = (await sidebar.boundingBox())!;
    expect(after.x - before.x).toBeGreaterThan(60);
    expect(Math.round(after.x)).toBe(Math.round(sbAfter.x + sbAfter.width));
    expect(after.width).toBeLessThan(before.width);
    await expect(handle).toHaveAttribute("aria-valuenow", String(Math.round(sbAfter.width)));
    // Remembered per device.
    await page.reload();
    await expect(page.getByTestId("app")).toBeVisible();
    expect(Math.round((await sidebar.boundingBox())!.width)).toBe(Math.round(sbAfter.width));
    expect(Math.round((await content.boundingBox())!.x)).toBe(Math.round(after.x));
  });

  test("chrome is native-feeling: no underlines, no text selection, default cursor, system 13px font, keyboard-only focus rings", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    const styleOf = (testId: string, props: string[]) => page.getByTestId(testId).first().evaluate((el, ps) => { const cs = getComputedStyle(el); return Object.fromEntries(ps.map((p) => [p, cs.getPropertyValue(p)])); }, props);
    const nav = await styleOf("nav-albums", ["text-decoration-line", "cursor", "user-select", "font-size", "font-family"]);
    expect(nav["text-decoration-line"]).toBe("none");
    expect(nav.cursor).toBe("default");
    expect(nav["user-select"]).toBe("none");
    expect(nav["font-size"]).toBe("13px");
    expect(nav["font-family"]!.startsWith("system-ui")).toBe(true);
    const btn = await styleOf("play-pause", ["cursor", "user-select", "text-decoration-line"]);
    expect(btn.cursor).toBe("default");
    expect(btn["user-select"]).toBe("none");
    expect(btn["text-decoration-line"]).toBe("none");
    const chrome = await styleOf("topbar", ["user-select", "cursor"]);
    expect(chrome["user-select"]).toBe("none");
    expect(chrome.cursor).toBe("default");
    // Real text input keeps a text cursor and selection.
    const input = await styleOf("search-input", ["cursor", "user-select"]);
    expect(input.cursor).toBe("text");
    expect(input["user-select"]).toBe("text");
    // No unicode glyph icons anywhere in the chrome: every icon is an SVG.
    const glyphs = await page.evaluate(() => {
      const bad: string[] = [];
      const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
      for (let n = walker.nextNode(); n; n = walker.nextNode()) {
        const t = n.textContent ?? "";
        if (/[←-⇿⌀-⏿■-➿⬀-⯿\u{1F300}-\u{1FAFF}]/u.test(t)) bad.push(t.trim());
      }
      return bad;
    });
    expect(glyphs).toEqual([]);
    expect(await page.locator("svg.icon").count()).toBeGreaterThan(10);
    // Mouse click gives no focus ring; Tab does.
    const albums = page.getByTestId("nav-albums");
    await albums.click();
    expect(await albums.evaluate((el) => getComputedStyle(el).outlineStyle)).toBe("none");
    await page.getByTestId("content").click();
    await page.keyboard.press("Tab");
    const focused = await page.evaluate(() => { const el = document.activeElement as HTMLElement | null; return el ? getComputedStyle(el).outlineStyle : "none"; });
    expect(focused).not.toBe("none");
  });

  test("album tiles are equal-width squares that stay inside the grid, and the idle player bar fits its row", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    // The idle player bar: every control inside the bar, bar exactly its row height.
    const bar = (await page.getByTestId("player-bar").boundingBox())!;
    const win = await page.evaluate(() => ({ w: window.innerWidth, h: window.innerHeight }));
    expect(Math.round(bar.y + bar.height)).toBe(win.h);
    expect(bar.height).toBeLessThanOrEqual(90);
    for (const id of ["play-pause", "shuffle", "next", "toggle-lyrics", "connect-button"]) {
      const b = (await page.getByTestId(id).boundingBox())!;
      expect(b.y, id).toBeGreaterThanOrEqual(bar.y);
      expect(b.y + b.height, id).toBeLessThanOrEqual(bar.y + bar.height);
    }
    await page.getByTestId("nav-albums").click();
    const tiles = page.getByTestId("grid-tile");
    await expect(tiles.first().locator("img")).toBeVisible();
    // Narrow the window so the tiles are smaller than the 300 px artwork.
    await page.setViewportSize({ width: 1000, height: 760 });
    await page.waitForTimeout(300);
    // Real libraries have long titles (the fake's are short): a nowrap title
    // must truncate, not widen its column.
    await tiles.first().locator(".t1").evaluate((el) => { el.textContent = "A Very Long Album Title (Deluxe Anniversary Bonus Track Edition)"; });
    const check = async () => {
      const geo = await page.locator("[role=grid]").filter({ has: tiles.first() }).evaluate((grid) => {
        const out = { overflow: grid.scrollWidth - grid.clientWidth, tiles: [] as { w: number; art: { x: number; y: number; w: number; h: number }; tile: { x: number; y: number; w: number; h: number } }[] };
        for (const t of Array.from(grid.querySelectorAll('[data-testid="grid-tile"]')).slice(0, 8)) {
          const a = t.querySelector(".art")!.getBoundingClientRect();
          const r = t.getBoundingClientRect();
          out.tiles.push({ w: Math.round(r.width), art: { x: a.x, y: a.y, w: a.width, h: a.height }, tile: { x: r.x, y: r.y, w: r.width, h: r.height } });
        }
        return out;
      });
      expect(geo.overflow).toBeLessThanOrEqual(0);
      expect(geo.tiles.length).toBeGreaterThan(3);
      const w0 = geo.tiles[0]!.w;
      for (const t of geo.tiles) {
        expect(Math.abs(t.w - w0)).toBeLessThanOrEqual(1);
        expect(Math.abs(t.art.w - t.art.h)).toBeLessThanOrEqual(1);
        expect(t.art.x).toBeGreaterThanOrEqual(t.tile.x);
        expect(t.art.x + t.art.w).toBeLessThanOrEqual(t.tile.x + t.tile.w + 0.5);
        expect(t.art.y + t.art.h).toBeLessThanOrEqual(t.tile.y + t.tile.h + 0.5);
      }
    };
    await check();
    // Still true after the sidebar is dragged wider.
    const h = (await page.getByTestId("sidebar-resize").boundingBox())!;
    await page.mouse.move(h.x + h.width / 2, h.y + h.height / 2);
    await page.mouse.down();
    await page.mouse.move(h.x + h.width / 2 + 100, h.y + h.height / 2, { steps: 5 });
    await page.mouse.up();
    await page.waitForTimeout(200);
    await check();
  });
});
