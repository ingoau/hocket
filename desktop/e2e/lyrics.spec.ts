// Syllable-synced lyrics: the FakeCore serves a real enhanced (syllable tier +
// background-vocal agent) document for one track; the AMLL renderer must sweep
// word by word, show the background line as a sub-line, and size itself from
// the device-local preference.
import { completeSetup, expect, test } from "./fixtures";

async function playShowcase(page: import("@playwright/test").Page): Promise<void> {
  const servers = await page.evaluate(async () => (await window.hocket.query({ type: "servers" })) as { type: string; data: { id: string }[] });
  const serverId = servers.data[0]!.id;
  await page.evaluate((sid) => window.hocket.dispatch({ type: "playTracks", data: { server_id: sid, track_ids: ["tr-1"], start_index: 0, label: "e2e", shuffle: false } }), serverId);
  await expect(page.getByTestId("queue-row-current")).toContainText("Word by Word");
}

test.describe("lyrics", () => {
  test("an enhanced document renders at syllable tier with a word-level sweep and background sub-lines", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playShowcase(page);
    const view = page.getByTestId("lyrics-view");
    await expect(view).toBeVisible();
    await expect(view).toHaveAttribute("data-tier", "syllable");
    await expect(page.getByTestId("lyrics-tools")).toContainText("Word-synced");
    const host = page.getByTestId("amll-host");
    // The first line becomes active at 1.5 s; its words are separate spans
    // that AMLL animates one at a time from each syllable's start to end.
    const active = host.locator('[class*="lyricLine"][class*="active"]:not([class*="lyricBgLine"])').first();
    await expect(active).toBeVisible({ timeout: 10_000 });
    // AMLL wraps each word in a span; syllables that belong to one word are
    // separate spans inside that wrapper. "title" is one word of two syllables.
    const words = active.locator('[class*="lyricMainLine"] > span');
    await expect.poll(() => words.count()).toBe(6);
    expect((await words.allTextContents()).map((t) => t.trim())).toEqual(["I", "lost", "my", "rank", "and", "title"]);
    const syllables = active.locator('[class*="lyricMainLine"] span:not(:has(span))');
    expect((await syllables.allTextContents()).map((t) => t.trim())).toEqual(["I", "lost", "my", "rank", "and", "ti", "tle"]);
    // Word-level sync: every syllable carries its own mask sweep driven by a
    // Web Animation from that syllable's start to its end.
    const sweeping = await syllables.evaluateAll((els) => els.map((el) => ({ mask: (el as HTMLElement).style.maskImage !== "", animations: el.getAnimations().length })));
    expect(sweeping).toHaveLength(7);
    expect(sweeping.every((w) => w.mask)).toBe(true);
    expect(sweeping.filter((w) => w.animations > 0).length).toBeGreaterThan(4);
    const [ti, tle] = await syllables.evaluateAll((els) => els.slice(-2).map((el) => el.getBoundingClientRect()));
    expect(Math.abs(tle!.left - ti!.right)).toBeLessThan(4);
    // The background vocal is an AMLL background sub-line, not a main line.
    const lineKinds = await host.evaluate((root) => {
      const out: { bg: boolean; text: string }[] = [];
      for (const el of Array.from(root.querySelectorAll("[class]"))) {
        const classes = Array.from(el.classList);
        if (!classes.some((c) => c.endsWith("_lyricLine"))) continue;
        out.push({ bg: classes.some((c) => c.endsWith("_lyricBgLine")), text: (el.textContent ?? "").replace(/\s+/g, " ").trim() });
      }
      return out;
    });
    expect(lineKinds.filter((l) => l.bg && l.text === "(Yeah, yeah)").length).toBe(2);
    expect(lineKinds.filter((l) => !l.bg && l.text === "(Yeah, yeah)").length).toBe(0);
    expect(lineKinds.filter((l) => l.bg).length).toBe(4);
    // 16 main lines (+ AMLL's own bottom spacer line).
    expect(lineKinds.filter((l) => !l.bg && l.text).length).toBe(16);
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
    // Far below AMLL's own 5vh default and never below 13px.
    expect(medium).toBeLessThan(vh * 0.05);
    expect(medium).toBeGreaterThanOrEqual(13);
    expect(medium).toBeLessThanOrEqual(19);
    await expect(page.getByTestId("amll-host")).toHaveAttribute("data-size", "medium");
    await page.getByTestId("lyrics-pane").hover();
    await page.getByTestId("lyrics-size-small").click();
    await expect(page.getByTestId("amll-host")).toHaveAttribute("data-size", "small");
    const small = await fontPx();
    expect(small).toBeLessThan(medium);
    // The Appearance page shows the same device-local preference.
    await page.getByTestId("nav-settings").click();
    await page.getByTestId("settings-nav-appearance").click();
    await expect(page.getByTestId("setting-lyrics-size")).toHaveValue("small");
    await page.getByTestId("setting-lyrics-size").selectOption("large");
    await expect(page.getByTestId("amll-host")).toHaveAttribute("data-size", "large");
    expect(await fontPx()).toBeGreaterThan(medium);
    // Persisted on this device.
    await page.reload();
    await expect(page.getByTestId("app")).toBeVisible();
    await expect(page.getByTestId("amll-host")).toHaveAttribute("data-size", "large");
    // The fullscreen player keeps its large layout regardless.
    await page.getByTestId("content").click();
    await page.keyboard.press("f");
    // It opens on the artwork; L shows the lyrics in its place.
    await page.keyboard.press("l");
    const fs = page.getByTestId("fullscreen-player").locator(".amll-lyric-player").first();
    await expect(fs).toBeVisible();
    expect(await fs.evaluate((el) => Number.parseFloat(getComputedStyle(el).fontSize))).toBeGreaterThan(medium * 1.5);
  });
});
