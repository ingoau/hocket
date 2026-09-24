// Reduced motion, screen-reader-only lyrics and announcements, contrast over
// the fullscreen fluid background (measured from the rendered pixels) and over
// artwork-tinted surfaces, forced colours, and reflow at 200% zoom.
import type { Page } from "@playwright/test";
import { completeSetup, expect, test } from "./fixtures";
import { expectNoViolations, playShowcase, serverId, setTheme, setZoom } from "./a11y-helpers";

async function playAlbum(page: Page, nth = 0): Promise<void> {
  await page.getByTestId("nav-albums").click();
  await page.getByTestId("grid-tile").nth(nth).dblclick();
  await page.getByTestId("album-play").click();
  await expect(page.getByTestId("queue-row-current")).toBeVisible();
}

async function runningAnimations(page: Page, within?: string): Promise<number> {
  return page.evaluate((sel) => document.getAnimations().filter((a) => {
    if (a.playState !== "running") return false;
    const target = (a.effect as KeyframeEffect | null)?.target as Element | null;
    return !sel || !!target?.closest(sel);
  }).length, within);
}

test.describe("motion, contrast and zoom", () => {
  test.setTimeout(180_000);

  test("reduced motion: lyrics are a statically highlighted list, the fullscreen background a still, nothing animates", async ({ hocket }) => {
    const { page } = hocket;
    await page.emulateMedia({ reducedMotion: "reduce" });
    await completeSetup(page);
    await playShowcase(page);
    const view = page.getByTestId("lyrics-view");
    await expect(view).toHaveAttribute("data-tier", "syllable");
    await expect(view).toHaveAttribute("data-mode", "plain");
    await expect(page.getByTestId("amll-host")).toHaveCount(0);
    const list = page.getByTestId("lyrics-plain");
    await expect(list).toBeVisible();
    await expect(list).toHaveAttribute("aria-label", "Lyrics lines");
    // Static highlight: exactly one current main line, drawn bold, no sweep masks.
    const current = list.locator('li[aria-current="true"]');
    await expect(current).toHaveCount(1, { timeout: 10_000 });
    await expect(current).toHaveText("I lost my rank and title");
    expect(await current.evaluate((el) => getComputedStyle(el).fontWeight)).toBe("700");
    expect(await list.evaluate((el) => Array.from(el.querySelectorAll("*")).some((x) => (x as HTMLElement).style.maskImage))).toBe(false);
    // Background vocals are sub-lines, named as such.
    await expect(list.locator("li.bg").first()).toHaveText(/^Background: \(Yeah, yeah\)/);
    await expect(list.locator("li.bg").first().locator(".sr-only")).toHaveText(/^Background:\s*$/);
    // The highlight moves on with playback, still one line.
    await expect.poll(async () => current.textContent(), { timeout: 15_000 }).not.toBe("I lost my rank and title");
    await expect(current).toHaveCount(1);
    expect(await runningAnimations(page)).toBe(0);
    // Transitions are off too.
    expect(await page.getByTestId("play-pause").evaluate((el) => getComputedStyle(el).transitionDuration)).toBe("0s");
    // Fullscreen: a still background instead of the animated fluid one, plain lyrics, nothing animating.
    await page.getByTestId("content").click();
    await page.keyboard.press("f");
    const fs = page.getByTestId("fullscreen-player");
    await expect(fs).toBeVisible();
    await expect(fs.getByTestId("fs-still")).toBeVisible();
    await expect(fs.getByTestId("fluid-bg")).toHaveCount(0);
    await expect(fs.getByTestId("lyrics-view")).toHaveAttribute("data-mode", "plain");
    await page.waitForTimeout(500);
    expect(await runningAnimations(page)).toBe(0);
  });

  test("default motion: AMLL is decorative, a labelled list mirrors it, no line is announced; the track change is", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    const announcer = page.getByTestId("now-playing-announcer");
    await expect(announcer).toHaveAttribute("aria-live", "polite");
    await expect(announcer).toHaveText("");
    await playShowcase(page);
    await expect(announcer).toHaveText(/^Now playing: Word by Word by .+/);
    const view = page.getByTestId("lyrics-view");
    await expect(view).toHaveAttribute("data-mode", "animated");
    await expect(page.getByTestId("amll-host")).toHaveAttribute("aria-hidden", "true");
    const sr = page.getByTestId("lyrics-sr-list");
    await expect(sr).toHaveAttribute("aria-label", "Lyrics lines");
    await expect(sr.locator('li[aria-current="true"]')).toHaveCount(1, { timeout: 10_000 });
    // Nothing in the lyrics is a live region.
    expect(await view.locator('[aria-live], [role="status"], [role="alert"], [role="log"], [role="marquee"], [role="timer"]').count()).toBe(0);
    // The announcer stays silent while the position (and the lyrics) move on.
    const first = await announcer.textContent();
    const lineBefore = await sr.locator('li[aria-current="true"]').textContent();
    await expect.poll(async () => sr.locator('li[aria-current="true"]').textContent(), { timeout: 15_000 }).not.toBe(lineBefore);
    await expect(announcer).toHaveText(first!);
    expect(first).not.toMatch(/\d+:\d\d/);
    // A track change is announced.
    const sid = await serverId(page);
    await page.evaluate((s) => window.hocket.dispatch({ type: "playTracks", data: { server_id: s, track_ids: ["tr-2"], start_index: 0, label: "e2e", shuffle: false } }), sid);
    await expect(announcer).not.toHaveText(first!);
    await expect(announcer).toHaveText(/^Now playing: /);
  });

  test("the in-app switch gives the plain, full-contrast lyrics list without any system preference", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playShowcase(page);
    await expect(page.getByTestId("lyrics-view")).toHaveAttribute("data-mode", "animated");
    await page.getByTestId("nav-settings").click();
    await page.getByTestId("settings-nav-appearance").click();
    const toggle = page.getByTestId("setting-lyrics-animated");
    await expect(toggle).toBeChecked();
    await toggle.focus();
    await page.keyboard.press("Space");
    await expect(toggle).not.toBeChecked();
    await expect(page.getByTestId("lyrics-view")).toHaveAttribute("data-mode", "plain");
    await expect(page.getByTestId("lyrics-plain").locator('li[aria-current="true"]')).toHaveCount(1, { timeout: 10_000 });
    // Remembered on this device.
    await page.reload();
    await expect(page.getByTestId("lyrics-view")).toHaveAttribute("data-mode", "plain");
  });

  test("fullscreen text reaches 4.5:1 over the fluid background, measured from the pixels, in both themes and on several covers", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    const sid = await serverId(page);
    for (const theme of ["light", "dark"] as const) {
      await setTheme(page, theme);
      for (const track of ["tr-1", "tr-40", "tr-77"]) {
        await page.evaluate(({ s, id }) => window.hocket.dispatch({ type: "playTracks", data: { server_id: s, track_ids: [id], start_index: 0, label: "e2e", shuffle: false } }), { s: sid, id: track });
        await page.getByTestId("toggle-fullscreen").click();
        const fs = page.getByTestId("fullscreen-player");
        await expect(fs.getByTestId("fs-scrim")).toHaveAttribute("data-measured", "true");
        await expect(fs.getByTestId("fluid-bg")).toHaveAttribute("data-source", "artwork");
        await page.waitForTimeout(600);
        const texts = await page.evaluate(() => {
          const out: { what: string; rect: { x: number; y: number; w: number; h: number }; color: string }[] = [];
          for (const sel of [".fullscreen .info .t1", ".fullscreen .info .t2", ".fullscreen .seek > span", ".fullscreen .side .tab"]) {
            for (const el of Array.from(document.querySelectorAll<HTMLElement>(sel))) {
              const r = el.getBoundingClientRect();
              if (r.width && r.height && el.textContent?.trim()) out.push({ what: `${sel} "${el.textContent.trim().slice(0, 20)}"`, rect: { x: r.x, y: r.y, w: r.width, h: r.height }, color: getComputedStyle(el).color });
            }
          }
          return out;
        });
        expect(texts.length).toBeGreaterThan(5);
        // Photograph the background alone: hide everything drawn over it.
        await page.addStyleTag({ content: ".fullscreen .left, .fullscreen .side, .fullscreen .close { visibility: hidden !important; }" }).then((h) => h.evaluate((el) => (el as Element).setAttribute("data-e2e-hide", "")));
        const png = (await page.screenshot()).toString("base64");
        await page.evaluate(() => document.querySelector("[data-e2e-hide]")?.remove());
        const worst = await page.evaluate(async ({ png, texts }) => {
          const bytes = Uint8Array.from(atob(png), (c) => c.charCodeAt(0));
          const bmp = await createImageBitmap(new Blob([bytes], { type: "image/png" }));
          const canvas = new OffscreenCanvas(bmp.width, bmp.height);
          const ctx = canvas.getContext("2d")!;
          ctx.drawImage(bmp, 0, 0);
          const scale = bmp.width / window.innerWidth;
          const lin = (v: number) => { const c = v / 255; return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4; };
          const lum = (r: number, g: number, b: number) => 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
          return texts.map((t) => {
            const m = /rgba?\(([\d.]+),\s*([\d.]+),\s*([\d.]+)(?:,\s*([\d.]+))?/.exec(t.color)!;
            const [fr, fg, fb, fa] = [Number(m[1]), Number(m[2]), Number(m[3]), m[4] === undefined ? 1 : Number(m[4])];
            const x0 = Math.floor(t.rect.x * scale), y0 = Math.floor(t.rect.y * scale);
            const w = Math.max(1, Math.floor(t.rect.w * scale)), h = Math.max(1, Math.floor(t.rect.h * scale));
            const { data } = ctx.getImageData(x0, y0, w, h);
            let min = Infinity;
            for (let i = 0; i < data.length; i += 4) {
              const [br, bg, bb] = [data[i]!, data[i + 1]!, data[i + 2]!];
              // The text colour as composited over this background pixel.
              const tr = fr * fa + br * (1 - fa), tg = fg * fa + bg * (1 - fa), tb = fb * fa + bb * (1 - fa);
              const a = lum(tr, tg, tb), b = lum(br, bg, bb);
              min = Math.min(min, (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05));
            }
            return { what: t.what, min: Math.round(min * 100) / 100 };
          });
        }, { png, texts });
        for (const w of worst) expect(w.min, `${theme} ${track} ${w.what}`).toBeGreaterThanOrEqual(4.5);
        await page.keyboard.press("Escape");
        await expect(fs).toHaveCount(0);
      }
    }
  });

  test("any accent colour keeps text on accent-tinted surfaces, fills and toasts at 4.5:1 (axe), light and dark", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await page.evaluate(() => window.hocket.dispatch({ type: "setSetting", data: { key: "display.dynamicColour", value: "false" } }));
    await playAlbum(page, 2);
    for (const theme of ["light", "dark"] as const) {
      await setTheme(page, theme);
      for (const accent of ["#ffff00", "#000080", "#00ffff", "#ff2020", "#ffffff", "#101010"]) {
        await page.evaluate((a) => window.hocket.dispatch({ type: "setSetting", data: { key: "display.accent", value: JSON.stringify(a) } }), accent);
        await expect.poll(() => page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue("--accent").trim())).toBe(accent);
        // Album detail: the playing row (accent text), selected rows (tint), the primary button (fill), the current queue row (tint).
        const rows = page.getByTestId("track-row");
        await rows.nth(1).locator(".td.title").click();
        await rows.nth(3).locator(".td.title").click({ modifiers: ["Shift"] });
        await expect(page.locator('[data-testid="track-row"].selected')).toHaveCount(3);
        await expectNoViolations(page, `${theme} accent ${accent}: rows, queue, buttons`);
        // A menu's active item is an accent fill.
        await rows.nth(1).locator(".td.title").click({ button: "right" });
        await page.keyboard.press("ArrowDown");
        await expectNoViolations(page, `${theme} accent ${accent}: menu`);
        await page.keyboard.press("Escape");
        // A toast's action is the accent on the inverse surface.
        await page.getByTestId("shuffle").click();
        await expect(page.getByTestId("toast-action").first()).toBeVisible();
        await expectNoViolations(page, `${theme} accent ${accent}: toast`);
        await page.getByTestId("shuffle").click();
      }
    }
  });

  test("forced colours: selection, focus, toggles and the seek fill use system colours; the fluid background gives way", async ({ hocket }) => {
    const { page } = hocket;
    await page.emulateMedia({ forcedColors: "active" });
    await completeSetup(page);
    await playAlbum(page);
    const system = await page.evaluate(() => {
      const probe = document.createElement("div");
      probe.style.cssText = "forced-color-adjust:none;background:Highlight;color:HighlightText;outline:1px solid Highlight";
      document.body.append(probe);
      const cs = getComputedStyle(probe);
      const out = { highlight: cs.backgroundColor, highlightText: cs.color };
      probe.remove();
      return out;
    });
    const row = page.getByTestId("track-row").nth(1);
    await row.locator(".td.title").click();
    await expect(row).toHaveClass(/selected/);
    expect(await row.evaluate((el) => getComputedStyle(el).backgroundColor)).toBe(system.highlight);
    expect(await row.evaluate((el) => getComputedStyle(el).color)).toBe(system.highlightText);
    // Every cell's text too (not CanvasText on Highlight).
    for (const color of await row.locator(".td, .td a").evaluateAll((els) => els.map((el) => getComputedStyle(el).color))) expect(color).toBe(system.highlightText);
    // The keyboard focus ring.
    await page.keyboard.press("ArrowDown");
    const next = page.getByTestId("track-row").nth(2);
    await expect(next).toBeFocused();
    const ring = await next.evaluate((el) => ({ style: getComputedStyle(el).outlineStyle, width: getComputedStyle(el).outlineWidth, color: getComputedStyle(el).outlineColor }));
    expect(ring.style).not.toBe("none");
    expect(Number.parseFloat(ring.width)).toBeGreaterThanOrEqual(2);
    expect(ring.color).toBe(system.highlight);
    // The seek fill and a pressed toggle.
    expect(await page.locator(".player .seek .fill").evaluate((el) => getComputedStyle(el).backgroundColor)).toBe(system.highlight);
    await page.getByTestId("shuffle").click();
    await expect(page.getByTestId("shuffle")).toHaveAttribute("aria-pressed", "true");
    expect(await page.getByTestId("shuffle").evaluate((el) => getComputedStyle(el).outlineStyle)).toBe("solid");
    // Current nav item.
    expect(await page.getByTestId("nav-albums").evaluate((el) => getComputedStyle(el).backgroundColor)).toBe(system.highlight);
    // Fullscreen: no artwork background behind system-coloured text, plain lyrics.
    await page.getByTestId("toggle-fullscreen").click();
    const fs = page.getByTestId("fullscreen-player");
    await expect(fs).toBeVisible();
    for (const id of ["fluid-bg", "fs-still", "fs-scrim"]) {
      const el = fs.getByTestId(id);
      if (await el.count()) await expect(el).toBeHidden();
    }
    await expectNoViolations(page, "forced colours fullscreen");
  });

  test("200% zoom: the layout reflows with no sideways scrolling and no clipped controls", async ({ hocket }) => {
    const { app, page } = hocket;
    await setZoom(app, 2);
    await expect.poll(() => page.evaluate(() => window.innerWidth)).toBeLessThanOrEqual(900);
    const check = async (label: string) => {
      await page.waitForTimeout(300);
      const problems = await page.evaluate(() => {
        const out: string[] = [];
        const vw = document.documentElement.clientWidth;
        if (document.documentElement.scrollWidth > vw + 1) out.push(`document scrolls sideways: ${document.documentElement.scrollWidth} > ${vw}`);
        const name = (el: Element) => `${el.tagName.toLowerCase()}${el.id ? `#${el.id}` : ""}${el.getAttribute("data-testid") ? `[${el.getAttribute("data-testid")}]` : ""}.${(el.getAttribute("class") ?? "").split(" ").join(".")} "${(el.getAttribute("aria-label") ?? el.textContent ?? "").trim().slice(0, 30)}"`;
        for (const el of Array.from(document.querySelectorAll<HTMLElement>("body *"))) {
          if (el.closest("[inert]")) continue;
          const cs = getComputedStyle(el);
          if (cs.display === "none" || cs.visibility === "hidden") continue;
          if ((cs.overflowX === "auto" || cs.overflowX === "scroll") && el.scrollWidth > el.clientWidth + 1 && el.clientWidth > 0) out.push(`scrolls sideways: ${name(el)} (${el.scrollWidth} > ${el.clientWidth})`);
        }
        // Keyboard controls: everything in the Tab order plus the current stop of each roving widget.
        // (tabindex=-1 links and buttons inside rows and tiles are pointer shortcuts to what the row's
        // menu offers; a truncated artist link in a narrow cell is text-overflow, not a clipped control.)
        const focusables = Array.from(document.querySelectorAll<HTMLElement>('a[href], button, input, select, textarea, [tabindex], [role="slider"]')).filter((el) => el.tabIndex >= 0 && !(el as HTMLButtonElement).disabled);
        for (const el of focusables) {
          if (el.closest("[inert]") || el.closest(".sr-only") || el.classList.contains("skip-link")) continue;
          const r = el.getBoundingClientRect();
          if (!r.width || !r.height) continue;
          const cs = getComputedStyle(el);
          if (cs.visibility === "hidden") continue;
          if (r.left < -1 || r.right > vw + 1) { out.push(`off-screen sideways: ${name(el)} ${Math.round(r.left)}–${Math.round(r.right)} of ${vw}`); continue; }
          // Clipped by an ancestor that hides overflow. Inside a vertical scroller the element can be
          // scrolled into view, so vertical clipping only counts outside of one.
          let scrollerY = false;
          for (let a = el.parentElement; a && a !== document.body; a = a.parentElement) {
            const acs = getComputedStyle(a);
            const ar = a.getBoundingClientRect();
            const hidesX = acs.overflowX === "hidden" || acs.overflowX === "clip";
            const hidesY = acs.overflowY === "hidden" || acs.overflowY === "clip";
            if (acs.overflowY === "auto" || acs.overflowY === "scroll") scrollerY = true;
            if (hidesX && (r.left < ar.left - 1 || r.right > ar.right + 1)) { out.push(`clipped sideways by ${name(a)}: ${name(el)}`); break; }
            if (hidesY && !scrollerY && (r.top < ar.top - 1 || r.bottom > ar.bottom + 1)) { out.push(`clipped vertically by ${name(a)}: ${name(el)}`); break; }
          }
        }
        return out;
      });
      expect(problems, label).toEqual([]);
    };
    // The setup form scrolls instead of being cut off top and bottom.
    await expect(page.getByTestId("setup")).toBeVisible();
    await check("200% setup");
    await page.getByTestId("setup-connect").scrollIntoViewIfNeeded();
    await expect(page.getByTestId("setup-connect")).toBeInViewport();
    await setZoom(app, 1);
    await completeSetup(page);
    await playShowcase(page);
    await setZoom(app, 2);
    await expect.poll(() => page.evaluate(() => window.innerWidth)).toBeLessThanOrEqual(900);
    for (const view of ["home", "albums", "songs", "playlists", "genres", "downloads", "filters", "stats"]) {
      await page.getByTestId(`nav-${view}`).click();
      await expect(page.getByTestId(`view-${view}`)).toBeVisible();
      await check(`200% ${view}`);
    }
    // The sidebar is an icon rail whose items keep their names.
    await expect(page.getByTestId("nav-albums")).toHaveAccessibleName("Albums");
    expect((await page.getByTestId("sidebar").boundingBox())!.width).toBeLessThan(60);
    await page.getByTestId("nav-albums").click();
    await page.getByTestId("grid-tile").first().dblclick();
    await expect(page.getByTestId("view-album")).toBeVisible();
    await check("200% album detail");
    await page.getByTestId("nav-filters").click();
    await page.getByTestId("new-filter").click();
    await check("200% filter builder");
    await page.getByTestId("nav-settings").click();
    for (const section of ["general", "audio", "transcoding", "connect", "storage", "lyrics", "appearance", "customisation", "shortcuts", "backup", "diagnostics", "about"]) {
      await page.getByTestId(`settings-nav-${section}`).click();
      await check(`200% settings/${section}`);
    }
    // The side panel is a drawer, closed until asked for; Q opens it, Escape closes it.
    await expect(page.getByTestId("right-panel")).toHaveCount(0);
    await page.getByTestId("content").click();
    await page.keyboard.press("q");
    await expect(page.getByTestId("right-panel")).toBeVisible();
    await check("200% queue drawer");
    await page.getByTestId("queue-timeline").focus();
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("right-panel")).toHaveCount(0);
    await expect(page.getByTestId("toggle-side-panel")).toBeFocused();
    // Overlays.
    await page.keyboard.press("f");
    await expect(page.getByTestId("fullscreen-player")).toBeVisible();
    await check("200% fullscreen");
    await page.keyboard.press("Escape");
    await page.keyboard.press("Control+k");
    await page.getByTestId("palette-input").fill("s");
    await check("200% palette");
    await page.keyboard.press("Escape");
    await page.getByTestId("nav-songs").click();
    await page.getByTestId("track-row").first().click({ button: "right" });
    await expect(page.getByTestId("context-menu")).toBeVisible();
    await check("200% context menu");
    await page.locator('[data-testid="context-menu"] [data-action="addToPlaylist"]').click();
    await expect(page.getByTestId("dialog-addToPlaylist")).toBeVisible();
    await check("200% dialog");
    await page.keyboard.press("Escape");
    await setZoom(app, 1);
  });
});
