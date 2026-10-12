// Screenshots of the now playing surfaces (player bar, side panel, fullscreen
// player) for reviewing the layout by eye against the prototypes in
// docs/plans/prototypes. Not an assertion test: it runs only with an output
// folder. With HOCKET_FAKE_COVERS (a folder of covers; album n wears cover
// n - 1, so put a busy card cover first, then one with flat edges and one to
// mirror both ways, the twelfth one that carries on to the right only, then a
// white one with pixel squares and one of columns, both carried on below, and reflections below:
// a lake, a night sky and a pastel one)
// it also shoots the immersive layouts, against a native addon built
// with `pnpm gen` (the fake core borrows its classifier). HOCKET_SCREENSHOT_ONLY limits which
// shots are written (comma-separated name fragments):
//
//   pnpm build && HOCKET_SCREENSHOT_DIR=/tmp/shots HOCKET_FAKE_COVERS=/path/to/covers \
//     xvfb-run -a -s "-screen 0 2000x1600x24" pnpm test:e2e now-playing-screenshots
import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test, type ElectronApplication, type Page } from "@playwright/test";
import type { DeviceInfo, ServerInfo } from "../src/core/api";
import { completeSetup, expect, launchFake } from "./fixtures";

const out = process.env.HOCKET_SCREENSHOT_DIR;
const covers = process.env.HOCKET_FAKE_COVERS;
/** HOCKET_SCREENSHOT_ONLY=fs-card,sidebar-lyrics: write only the shots whose names contain one of these. */
const only = process.env.HOCKET_SCREENSHOT_ONLY?.split(",").map((s) => s.trim()).filter(Boolean);
const wanted = (name: string) => !only?.length || only.some((f) => name.includes(f));

test.use({ trace: "off" });

async function size(app: ElectronApplication, width: number, height: number): Promise<void> {
  await app.evaluate(({ BrowserWindow }, s) => BrowserWindow.getAllWindows()[0]?.setContentSize(s.width, s.height), { width, height });
}

/** Waits for the finite animations to finish, so a shot is never caught mid-transition. */
async function settle(page: Page): Promise<void> {
  await page.waitForTimeout(400);
  // Not the lyrics' own (they last as long as a line does).
  await page.evaluate(() => Promise.race([
    Promise.all(document.getAnimations().filter((a) => {
      const target = (a.effect as KeyframeEffect | null)?.target;
      return a.effect?.getTiming().iterations !== Number.POSITIVE_INFINITY && !(target instanceof Element && target.closest(".amll-host, .lyrics-pane"));
    }).map((a) => a.finished.catch(() => undefined))),
    new Promise((r) => setTimeout(r, 3000)),
  ]));
  await page.waitForTimeout(150);
}

async function setSetting(page: Page, key: string, value: unknown): Promise<void> {
  await page.evaluate(({ key, value }) => window.hocket.dispatch({ type: "setSetting", data: { key, value: JSON.stringify(value) } }), { key, value });
}

async function playAlbum(page: Page, index: number, label: string): Promise<void> {
  const servers = (await page.evaluate(() => window.hocket.query({ type: "servers" }))) as { data: ServerInfo[] };
  const serverId = servers.data[0]!.id;
  // The fake library's albums are al-1, al-2, …; with HOCKET_FAKE_COVERS album n wears cover n - 1.
  const id = `al-${index + 1}`;
  await page.evaluate(({ serverId, id, label }) => window.hocket.dispatch({ type: "playContext", data: { args: { context: { serverId, kind: { type: "album", data: { id } }, label, sort: "default" }, shuffle: false, saveOutgoing: true } } }), { serverId, id, label });
  await expect.poll(async () => ((await page.evaluate(() => window.hocket.query({ type: "snapshot" }))) as { data: { queue: { current?: { track: { albumId?: string } } } } }).data.queue.current?.track.albumId).toBe(id);
  // A track with synced lyrics, a little way in, so the lyrics views have lines to show.
  for (let i = 0; i < 8; i++) {
    const tier = await page.evaluate(async () => {
      const snap = (await window.hocket.query({ type: "snapshot" })) as { data: { queue: { current?: { track: { id: string } } } } };
      const id = snap.data.queue.current?.track.id;
      if (!id) return undefined;
      const r = (await window.hocket.query({ type: "lyrics", data: { track_id: id } })) as { data?: { tier?: string } };
      return r.data?.tier;
    });
    if (tier === "line" || tier === "syllable") break;
    await page.evaluate(() => window.hocket.dispatch({ type: "next" }));
    await page.waitForTimeout(300);
  }
  await page.evaluate(() => window.hocket.dispatch({ type: "seekTo", data: { position_ms: 32_000 } }));
}

async function handoff(page: Page, to: "remote" | "self"): Promise<void> {
  await expect.poll(async () => ((await page.evaluate(() => window.hocket.query({ type: "devices" }))) as { data: DeviceInfo[] }).data.length).toBeGreaterThan(1);
  const devices = ((await page.evaluate(() => window.hocket.query({ type: "devices" }))) as { data: DeviceInfo[] }).data;
  const target = devices.find((d) => (to === "self" ? d.isSelf : !d.isSelf))!;
  await page.evaluate((id) => window.hocket.dispatch({ type: "handoffTo", data: { device_id: id } }), target.id);
}

/** The fullscreen player on `mode` (clicking its pill toggles, like a user). */
async function fsMode(page: Page, mode: "art" | "lyrics" | "queue" | "about"): Promise<void> {
  const fs = page.getByTestId("fullscreen-player");
  const on = await fs.getAttribute("data-mode");
  if (on === mode) return;
  await fs.getByTestId(mode === "art" ? `fs-mode-${on}` : `fs-mode-${mode}`).click();
  await expect(fs).toHaveAttribute("data-mode", mode);
}

test("now playing surfaces", async () => {
  test.skip(!out, "set HOCKET_SCREENSHOT_DIR");
  test.setTimeout(0);
  mkdirSync(out!, { recursive: true });
  for (const theme of ["light", "dark"] as const) {
    const userData = mkdtempSync(join(tmpdir(), "hocket-np-shots-"));
    const { app, page } = await launchFake(userData, covers ? { HOCKET_FAKE_COVERS: covers } : {});
    const shot = async (name: string) => {
      if (!wanted(name)) return;
      await settle(page);
      await page.screenshot({ path: join(out!, `desktop-${theme}-${name}.png`) });
    };
    try {
      await completeSetup(page);
      await page.addStyleTag({ content: '[data-testid="toasts"], [data-testid="dev-banner"] { display: none !important; }' });
      await setSetting(page, "display.theme", theme);
      await size(app, 1456, 912);
      // Two albums first, so the queue selector has a saved queue to offer.
      await playAlbum(page, 3, "Late night drive");
      await playAlbum(page, 0, "Road trip");
      await page.evaluate(() => window.hocket.dispatch({ type: "dismissResumeOffer" }));
      await page.mouse.move(700, 450);

      // The bar and the side panel.
      await expect(page.getByTestId("right-panel")).toHaveAttribute("data-view", "overview");
      await shot("sidebar-overview");
      await shot("sidebar-overview-immersive-card");
      if (wanted("sidebar-artist-menu")) {
        await page.getByTestId("sidebar-artist").click();
        await expect(page.getByTestId("artist-menu")).toBeVisible();
        await shot("sidebar-artist-menu");
        await page.keyboard.press("Escape");
        await expect(page.getByTestId("artist-menu")).toHaveCount(0);
      }
      await page.getByTestId("toggle-side-panel").click();
      await expect(page.getByTestId("right-panel")).toHaveCount(0);
      await shot("bar-sidebar-closed");
      await page.getByTestId("toggle-lyrics").click();
      await expect(page.getByTestId("right-panel")).toHaveAttribute("data-view", "lyrics");
      // Shoot on a sung line, not in a gap between two (where no line is current): retake until
      // a line was current both before and after the shot.
      await settle(page);
      const current = page.getByTestId("right-panel").locator('[class*="_lyricLine"][class*="_active"]:not([class*="_lyricBgLine"])');
      for (let i = 0; i < 20 && wanted("sidebar-lyrics"); i++) {
        await expect(current.first()).toBeVisible({ timeout: 15_000 });
        await page.waitForTimeout(400);
        if (!(await current.count())) continue;
        await page.screenshot({ path: join(out!, `desktop-${theme}-sidebar-lyrics.png`) });
        if (await current.count()) break;
      }
      await page.getByTestId("lyrics-options").click();
      await expect(page.getByTestId("lyrics-tools")).toBeVisible();
      await shot("sidebar-lyrics-options");
      await page.keyboard.press("Escape");
      if (wanted("bar-lyrics-options")) {
        // The bar's lyrics toggle, right-clicked: the same options, above it.
        await expect(page.getByTestId("lyrics-tools")).toHaveCount(0);
        await page.getByTestId("toggle-lyrics").click({ button: "right" });
        await expect(page.getByTestId("lyrics-tools")).toBeVisible();
        await shot("bar-lyrics-options");
        await page.keyboard.press("Escape");
        await expect(page.getByTestId("lyrics-tools")).toHaveCount(0);
      }
      await page.getByTestId("toggle-queue").click();
      await expect(page.getByTestId("right-panel")).toHaveAttribute("data-view", "queue");
      await shot("sidebar-queue");
      await page.getByTestId("queue-selector").click();
      await expect(page.getByTestId("queue-selector-menu")).toBeVisible();
      await shot("sidebar-queue-selector");
      await page.keyboard.press("Escape");
      await page.getByTestId("sidebar-back").click();
      await page.getByTestId("sidebar-about-card-link").click();
      await expect(page.getByTestId("right-panel")).toHaveAttribute("data-view", "about");
      await shot("sidebar-about");
      await page.getByTestId("sidebar-back").click();
      await page.getByTestId("right-panel").locator(".side-scroll").evaluate((el) => el.scrollTo({ top: el.scrollHeight }));
      await shot("sidebar-overview-scrolled");
      await page.getByTestId("right-panel").locator(".side-scroll").evaluate((el) => el.scrollTo({ top: 0 }));
      await handoff(page, "remote");
      await expect(page.getByTestId("connect-button-name")).toBeVisible();
      await shot("connect-remote");
      if (wanted("bar-connect-remote")) await page.getByTestId("player-bar").screenshot({ path: join(out!, `desktop-${theme}-bar-connect-remote.png`) });
      await handoff(page, "self");
      await expect(page.getByTestId("connect-button-name")).toHaveCount(0);
      if (wanted("bar-connect-local")) {
        await settle(page);
        await page.getByTestId("player-bar").screenshot({ path: join(out!, `desktop-${theme}-bar-connect-local.png`) });
      }

      // Narrow: the bar's two rows, the side panel a drawer.
      await size(app, 860, 760);
      await shot("narrow-bar");
      await page.getByTestId("toggle-queue").click();
      await expect(page.getByTestId("right-panel")).toHaveAttribute("data-view", "queue");
      await shot("narrow-drawer-queue");
      await page.keyboard.press("Escape");
      await size(app, 1456, 912);

      // Fullscreen, card layout (cover 0 is busy: a card; "never" makes sure).
      await setSetting(page, "display.immersiveArtwork", "never");
      await page.getByTestId("toggle-fullscreen").click();
      await page.getByTestId("fullscreen-player").waitFor();
      await shot("fs-card-queue-default");
      for (const mode of ["art", "lyrics", "queue", "about"] as const) {
        await fsMode(page, mode);
        await shot(`fs-card-${mode}`);
      }
      await fsMode(page, "lyrics");
      await page.getByTestId("fullscreen-player").getByTestId("fs-mode-lyrics").click({ button: "right" });
      await shot("fs-card-lyrics-options");
      await page.keyboard.press("Escape");
      await fsMode(page, "queue");
      await page.getByTestId("fullscreen-player").getByTestId("queue-selector").click();
      await shot("fs-card-queue-selector");
      await page.keyboard.press("Escape");
      await fsMode(page, "art");
      await page.getByTestId("fs-pill-volume").click();
      await shot("fs-card-volume");
      await page.keyboard.press("Escape");
      if (wanted("fs-card-artist-menu")) {
        await page.getByTestId("fs-artist").click();
        await expect(page.getByTestId("artist-menu")).toBeVisible();
        await shot("fs-card-artist-menu");
        await page.keyboard.press("Escape");
        await expect(page.getByTestId("artist-menu")).toHaveCount(0);
      }
      await size(app, 1600, 1000);
      await shot("fs-card-art-1600");
      await fsMode(page, "lyrics");
      await shot("fs-card-lyrics-1600");
      await size(app, 900, 1400);
      await shot("fs-tall-card-lyrics");
      await fsMode(page, "art");
      await shot("fs-tall-card-art");
      await size(app, 1456, 912);
      await page.keyboard.press("Escape");
      await size(app, 1600, 1000);
      await shot("sidebar-overview-1600");
      await size(app, 1456, 912);

      // Immersive (cover 1 has flat edges to extend, cover 2 is a landscape to mirror, both centred
      // in the fullscreen player; cover 11 carries on to the right only, so it stays on the left).
      if (covers) {
        await setSetting(page, "display.immersiveArtwork", "automatic");
        // [cover, name, the side panel's shot and the bottom edge's style it must show, or none].
        // The landscape (2) mirrors both ways in the fullscreen player but ends in a flat band at the
        // bottom (an extension in the side panel); the lake (14) is it without the band, mirrored.
        const entries = [[1, "extend", "extend", "extend"], [2, "mirror", undefined, undefined], [11, "right", undefined, undefined], [12, "extend-white", "extend-white", "extend"], [13, "extend-columns", "extend-columns", "extend"], [14, "lake", "mirror", "mirror"], [15, "night", "mirror-dark", "mirror"], [16, "pastel", "mirror-light", "mirror"]] as const;
        for (const [index, name, side, style] of entries) {
          await playAlbum(page, index, "Road trip");
          if (side && style) {
            const panel = page.getByTestId("right-panel");
            if (!(await panel.count())) await page.getByTestId("toggle-side-panel").click();
            if ((await panel.getAttribute("data-view")) !== "overview") await page.getByTestId("sidebar-back").click();
            await expect(panel).toHaveAttribute("data-view", "overview");
            await expect(page.getByTestId("side-overview")).toHaveClass(/immersive/);
            await expect(page.getByTestId("sidebar-continuation")).toHaveCount(1);
            await expect(page.getByTestId("sidebar-continuation")).toHaveAttribute("data-style", style);
            await shot(`sidebar-overview-immersive-${side}`);
            if (side === "mirror" && wanted("sidebar-overview-immersive-mirror-nolyrics")) {
              // A song without lyrics: the fade runs below the title instead.
              for (let i = 0; i < 12; i++) {
                const tier = await page.evaluate(async () => {
                  const snap = (await window.hocket.query({ type: "snapshot" })) as { data: { queue: { current?: { track: { id: string } } } } };
                  const r = (await window.hocket.query({ type: "lyrics", data: { track_id: snap.data.queue.current!.track.id } })) as { data?: { tier?: string } | null };
                  return r.data?.tier;
                });
                if (!tier) break;
                await page.evaluate(() => window.hocket.dispatch({ type: "next" }));
                await page.waitForTimeout(300);
              }
              await expect(page.getByTestId("sidebar-lyrics-card")).toHaveCount(0);
              await shot("sidebar-overview-immersive-mirror-nolyrics");
              await playAlbum(page, index, "Road trip");
            }
          }
          // (The white, columns and lake covers are for the side panel.)
          if (!["extend", "mirror", "right"].includes(name)) continue;
          await page.getByTestId("toggle-fullscreen").click();
          const fs = page.getByTestId("fullscreen-player");
          await fs.waitFor();
          await fsMode(page, "art");
          await expect(fs).toHaveAttribute("data-layout", "immersive");
          console.log(`${theme} ${name}: continuation style`, await fs.getByTestId("np-continuation").first().getAttribute("data-style"), "sides", await fs.getAttribute("data-sides"));
          await shot(`fs-immersive-${name}-art`);
          // Idle: the controls fade after a few seconds without the mouse.
          await page.mouse.move(700, 400);
          await expect(fs).toHaveAttribute("data-idle", "", { timeout: 8000 });
          await page.waitForTimeout(1200);
          if (wanted(`fs-immersive-${name}-idle`)) await page.screenshot({ path: join(out!, `desktop-${theme}-fs-immersive-${name}-idle.png`) });
          await page.mouse.move(600, 300);
          if (name === "mirror" && wanted("fs-immersive-mirror-mid")) {
            // Halfway through sliding from centred to the left as the queue opens.
            await settle(page);
            await fs.getByTestId("fs-mode-queue").click();
            await page.waitForTimeout(140);
            await page.screenshot({ path: join(out!, `desktop-${theme}-fs-immersive-mirror-mid.png`) });
            await fsMode(page, "art");
          }
          for (const mode of ["lyrics", "queue", "about"] as const) {
            await fsMode(page, mode);
            await shot(`fs-immersive-${name}-${mode}`);
          }
          await fsMode(page, "art");
          if (name === "mirror") {
            await size(app, 1600, 1000);
            await shot(`fs-immersive-${name}-art-1600`);
          }
          await size(app, 900, 1400);
          await shot(`fs-tall-${name}`);
          await size(app, 1456, 912);
          await page.keyboard.press("Escape");
        }
      }
    } finally {
      await app.close().catch(() => undefined);
      rmSync(userData, { recursive: true, force: true });
    }
  }
});
