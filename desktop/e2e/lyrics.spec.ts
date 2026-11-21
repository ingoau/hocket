// Syllable-synced lyrics: the FakeCore serves a real enhanced (syllable tier +
// background-vocal agent) document for one track; the AMLL renderer must sweep
// word by word, show the background line as a sub-line, and size itself from
// the device-local preference.
import { completeSetup, expect, test, chooseOption, showSideView } from "./fixtures";

/** AMLL's lines, main lines only: not the group wrappers (`_lyricLineWrapper`) or background vocals. */
const LINES = '[class*="_lyricLine"]:not([class*="_lyricLineWrapper"]):not([class*="_lyricBgLine"])';

async function playShowcase(page: import("@playwright/test").Page): Promise<void> {
  const servers = await page.evaluate(async () => (await window.hocket.query({ type: "accounts" })) as { type: string; data: { id: string }[] });
  const serverId = servers.data[0]!.id;
  await page.evaluate((sid) => window.hocket.dispatch({ type: "playTracks", data: { tracks: ["tr-1"].map((id) => ({ source: sid, id })), start_index: 0, label: "e2e", shuffle: false } }), serverId);
  await expect(page.getByTestId("player-bar").locator(".now .title")).toContainText("Word by Word");
  await showSideView(page, "lyrics");
}

test.describe("lyrics", () => {
  test("an enhanced document renders at syllable tier with a word-level sweep and background sub-lines", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playShowcase(page);
    const view = page.getByTestId("lyrics-view");
    await expect(view).toBeVisible();
    await expect(view).toHaveAttribute("data-tier", "syllable");
    // The sync type is in the view's Lyrics options, not over the lyrics.
    await expect(view.getByTestId("lyrics-tools")).toHaveCount(0);
    await page.getByTestId("lyrics-options").click();
    await expect(page.getByTestId("lyrics-tools")).toContainText("Word-synced");
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("lyrics-tools")).toHaveCount(0);
    await expect(page.getByTestId("lyrics-options")).toBeFocused();
    const host = page.getByTestId("amll-host");
    // The first line becomes active at 1.5 s; its words are separate spans
    // that AMLL animates one at a time from each syllable's start to end.
    const active = host.locator('[class*="lyricLine"][class*="active"]:not([class*="lyricBgLine"])').first();
    await expect(active).toBeVisible({ timeout: 10_000 });
    // Read that line by its text from here on: playback moves on to the next line within seconds.
    const first = host.locator(LINES).filter({ hasText: "rank and title" }).first();
    // AMLL wraps each word in a span; syllables that belong to one word are
    // separate spans inside that wrapper. "title" is one word of two syllables.
    const words = first.locator('[class*="lyricMainLine"] > span');
    await expect.poll(() => words.count()).toBe(6);
    expect((await words.allTextContents()).map((t) => t.trim())).toEqual(["I", "lost", "my", "rank", "and", "title"]);
    const syllables = first.locator('[class*="lyricMainLine"] span:not(:has(span))');
    expect((await syllables.allTextContents()).map((t) => t.trim())).toEqual(["I", "lost", "my", "rank", "and", "ti", "tle"]);
    // Word-level sync: every syllable carries its own mask sweep driven by a
    // Web Animation from that syllable's start to its end.
    const sweeping = await syllables.evaluateAll((els) => els.map((el) => ({ mask: (el as HTMLElement).style.maskImage !== "", animations: el.getAnimations().length })));
    expect(sweeping).toHaveLength(7);
    expect(sweeping.every((w) => w.mask)).toBe(true);
    expect(sweeping.filter((w) => w.animations > 0).length).toBeGreaterThan(4);
    const [ti, tle] = await syllables.evaluateAll((els) => els.slice(-2).map((el) => el.getBoundingClientRect()));
    expect(Math.abs(tle!.left - ti!.right)).toBeLessThan(4);
    // The background vocal is an AMLL background sub-line, not a main line. AMLL only builds the
    // lines within its height plus a 300 px overscan, and at the medium size the last line sits
    // just inside that (how far depends on the fonts and on where the playhead has scrolled to), so
    // the count is taken at the small size, where every line of this document is in reach.
    await page.getByTestId("lyrics-options").click();
    await page.getByTestId("lyrics-size-small").click();
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("amll-host")).toHaveAttribute("data-size", "small");
    const kinds = () => host.evaluate((root) => {
      const out: { bg: boolean; text: string }[] = [];
      for (const el of Array.from(root.querySelectorAll("[class]"))) {
        const classes = Array.from(el.classList);
        if (!classes.some((c) => c.endsWith("_lyricLine"))) continue;
        out.push({ bg: classes.some((c) => c.endsWith("_lyricBgLine")), text: (el.textContent ?? "").replace(/\s+/g, " ").trim() });
      }
      return out;
    });
    // 16 main lines (+ AMLL's own bottom spacer line), once the resized lines have settled.
    await expect.poll(async () => (await kinds()).filter((l) => !l.bg && l.text).length).toBe(16);
    const lineKinds = await kinds();
    expect(lineKinds.filter((l) => l.bg && l.text === "(Yeah, yeah)").length).toBe(2);
    expect(lineKinds.filter((l) => !l.bg && l.text === "(Yeah, yeah)").length).toBe(0);
    expect(lineKinds.filter((l) => l.bg).length).toBe(4);
    // The one cue-less line is present as a main line (line tier for that line only; see lyrics-map.test.ts for its timing).
    expect(lineKinds.filter((l) => !l.bg && l.text === "We've seen it several times").length).toBe(1);
  });

  test("in-window lyrics are viewport-sized and follow the device-local size preference", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playShowcase(page);
    const player = page.getByTestId("amll-host").locator(".amll-lyric-player").first();
    await expect(player).toBeVisible();
    const fontPx = () => player.evaluate((el) => Number.parseFloat(getComputedStyle(el).fontSize));
    const vh = await page.evaluate(() => window.innerHeight);
    const medium = await fontPx();
    // Far below AMLL's own 5vh default and never below 13px (the side panel's lyrics view has the
    // pane's whole height, so its lines are a little larger than the old half-height pane's).
    expect(medium).toBeLessThan(vh * 0.05);
    expect(medium).toBeGreaterThanOrEqual(13);
    expect(medium).toBeLessThanOrEqual(24);
    await expect(page.getByTestId("amll-host")).toHaveAttribute("data-size", "medium");
    await page.getByTestId("lyrics-options").click();
    await page.getByTestId("lyrics-size-small").click();
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("amll-host")).toHaveAttribute("data-size", "small");
    const small = await fontPx();
    expect(small).toBeLessThan(medium);
    // The Appearance page shows the same device-local preference.
    await page.getByTestId("nav-settings").click();
    await page.getByTestId("settings-nav-appearance").click();
    await expect(page.getByTestId("setting-lyrics-size")).toHaveAttribute("data-value", "small");
    await chooseOption(page.getByTestId("setting-lyrics-size"), "large");
    await expect(page.getByTestId("amll-host")).toHaveAttribute("data-size", "large");
    expect(await fontPx()).toBeGreaterThan(medium);
    // Persisted on this device.
    await page.reload();
    await expect(page.getByTestId("app")).toBeVisible();
    await expect(page.getByTestId("amll-host")).toHaveAttribute("data-size", "large");
    // The fullscreen player keeps its large layout regardless.
    await page.getByTestId("content").click();
    await page.keyboard.press("f");
    // It opens on the queue (the first time); L shows the lyrics instead.
    await page.keyboard.press("l");
    const fs = page.getByTestId("fullscreen-player").locator(".amll-lyric-player").first();
    await expect(fs).toBeVisible();
    expect(await fs.evaluate((el) => Number.parseFloat(getComputedStyle(el).fontSize))).toBeGreaterThan(medium * 1.5);
  });

  test("the side panel's lyrics behave like the fullscreen player's, dimmed for the theme's pane", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playShowcase(page);
    const side = page.getByTestId("right-panel").getByTestId("amll-host");
    await expect(side.locator('[class*="_lyricLine"][class*="_active"]:not([class*="_lyricBgLine"])').first()).toBeVisible({ timeout: 10_000 });

    /** How a lyrics view is set up: springs, blurred distant lines, and where the current line sits in the pane (0 top, 1 bottom). */
    const setup = (host: import("@playwright/test").Locator) => host.evaluate((root, sel) => {
      const player = root.querySelector<HTMLElement>(".amll-lyric-player")!;
      const lines = Array.from(root.querySelectorAll<HTMLElement>(sel));
      const active = lines.find((l) => /_active\b/.test(l.className) && l.textContent?.trim());
      const box = root.getBoundingClientRect();
      const a = active?.getBoundingClientRect();
      return {
        spring: !Array.from(player.classList).some((c) => c.endsWith("_disableSpring")),
        // AMLL writes blur(0px) on every line when blur is off.
        blurred: lines.some((l) => Number(/blur\(([\d.]+)px/.exec(l.style.filter)?.[1] ?? 0) > 0),
        position: a ? (a.top - box.top + a.height / 2) / box.height : -1,
      };
    }, LINES);
    // The current line settles in the middle of the pane (between lines there is none, so poll).
    const centred = (host: import("@playwright/test").Locator) => expect.poll(async () => Math.abs((await setup(host)).position - 0.5), { timeout: 15_000 }).toBeLessThan(0.08);
    await centred(side);
    const sideSetup = await setup(side);
    expect(sideSetup.spring).toBe(true);
    expect(sideSetup.blurred).toBe(true);

    // The words' mask alphas, evaluated: AMLL's levels, mapped onto --lyrics-dim..1 on the pane.
    const alphas = (host: import("@playwright/test").Locator) => host.evaluate((root, sel) => {
      const probe = document.createElement("div");
      document.body.append(probe);
      const resolve = (v: string) => { probe.style.opacity = v.trim(); return Number(getComputedStyle(probe).opacity); };
      const lines = Array.from(root.querySelectorAll<HTMLElement>(sel)).filter((l) => l.textContent?.trim());
      const word = (l: HTMLElement) => l.querySelector<HTMLElement>('[class*="_lyricMainLine"] > span')!;
      const inactive = lines.filter((l) => !/_active\b/.test(l.className)).map((l) => resolve(getComputedStyle(word(l)).getPropertyValue("--dark-mask-alpha")));
      const out = { inactive: Math.min(...inactive), dim: Number(getComputedStyle(root).getPropertyValue("--lyrics-dim")) };
      probe.remove();
      return out;
    }, LINES);
    const sideAlpha = await alphas(side);
    expect(sideAlpha.dim).toBeGreaterThan(0.4);
    // Never fainter than the 4.5:1 floor (AMLL's own 0.2 would be nearly invisible on the pane).
    expect(sideAlpha.inactive).toBeGreaterThanOrEqual(sideAlpha.dim - 0.01);
    expect(sideAlpha.inactive).toBeLessThan(1);

    // Click-to-seek, while paused: the player seeks to the line, and the springs still carry it to the middle.
    await page.evaluate(() => window.hocket.dispatch({ type: "pause" }));
    const target = side.locator(`${LINES}:not([class*="_active"])`).filter({ hasText: "You want to love me" }).first();
    await target.click();
    await expect.poll(() => page.getByTestId("right-panel").getByTestId("lyrics-sr-list").locator('li[aria-current="true"]').textContent(), { timeout: 10_000 }).toContain("You want to love me");
    await centred(side);
    await page.evaluate(() => window.hocket.dispatch({ type: "play" }));

    // Fullscreen: the same setup, over its blur with AMLL's own levels.
    await page.getByTestId("content").click();
    await page.keyboard.press("f");
    await page.keyboard.press("l");
    const fs = page.getByTestId("fullscreen-player").getByTestId("amll-host");
    await expect(fs.locator('[class*="_lyricLine"][class*="_active"]:not([class*="_lyricBgLine"])').first()).toBeVisible({ timeout: 10_000 });
    await centred(fs);
    const fsSetup = await setup(fs);
    expect(fsSetup.spring).toBe(true);
    expect(fsSetup.blurred).toBe(true);
    expect((await alphas(fs)).inactive).toBeLessThan(0.45);
  });
});
