// Screenshots of the now playing surfaces (player bar, side panel, fullscreen
// player) for reviewing the layout by eye against the prototypes. Not an
// assertion test: it runs only with an output folder.
//
//   pnpm build && HOCKET_SCREENSHOT_DIR=/tmp/shots HOCKET_FAKE_COVERS=/path/to/covers \
//     xvfb-run -a -s "-screen 0 2000x1600x24" pnpm test:e2e now-playing-screenshots
import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test, type ElectronApplication, type Page } from "@playwright/test";
import type { ServerInfo } from "../src/core/api";
import { completeSetup, expect, launchFake } from "./fixtures";

const out = process.env.HOCKET_SCREENSHOT_DIR;
const covers = process.env.HOCKET_FAKE_COVERS;

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

async function playAlbum(page: Page, index: number): Promise<void> {
  const servers = (await page.evaluate(() => window.hocket.query({ type: "servers" }))) as { data: ServerInfo[] };
  const serverId = servers.data[0]!.id;
  // The fake library's albums are al-1, al-2, …; with HOCKET_FAKE_COVERS album n wears cover n - 1.
  const id = `al-${index + 1}`;
  await page.evaluate(({ serverId, id }) => window.hocket.dispatch({ type: "playContext", data: { args: { context: { serverId, kind: { type: "album", data: { id } }, label: "Road trip", sort: "default" }, shuffle: false, saveOutgoing: false } } }), { serverId, id });
  await expect(page.getByTestId("queue-row-current").first()).toBeVisible();
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

test("now playing surfaces", async () => {
  test.skip(!out, "set HOCKET_SCREENSHOT_DIR");
  test.setTimeout(0);
  mkdirSync(out!, { recursive: true });
  for (const theme of ["light", "dark"] as const) {
    const userData = mkdtempSync(join(tmpdir(), "hocket-np-shots-"));
    const { app, page } = await launchFake(userData, covers ? { HOCKET_FAKE_COVERS: covers } : {});
    const shot = async (name: string) => {
      await settle(page);
      await page.screenshot({ path: join(out!, `desktop-${theme}-${name}.png`) });
    };
    try {
      await completeSetup(page);
      await page.addStyleTag({ content: '[data-testid="toasts"], [data-testid="dev-banner"] { display: none !important; }' });
      await setSetting(page, "display.theme", theme);
      await size(app, 1456, 912);
      await playAlbum(page, 0);
      await shot("bar-panel");
      await page.getByTestId("tab-recent").click();
      await shot("panel-recent");
      await page.getByTestId("tab-queue").click();

      // Fullscreen, card layout.
      await setSetting(page, "display.immersiveArtwork", "never");
      await page.keyboard.press("f");
      await page.getByTestId("fullscreen-player").waitFor();
      for (const mode of ["art", "lyrics", "queue", "about"] as const) {
        const on = await page.getByTestId("fullscreen-player").getAttribute("data-mode");
        if (on !== mode) await page.getByTestId(mode === "art" ? `fs-mode-${on}` : `fs-mode-${mode}`).click();
        await shot(`fs-card-${mode}`);
      }
      await page.keyboard.press("Escape");

      // Fullscreen, immersive (with HOCKET_FAKE_COVERS: cover 1 is flat, cover 2 a landscape).
      if (covers) {
        await setSetting(page, "display.immersiveArtwork", "automatic");
        for (const [index, name] of [[1, "extend"], [2, "mirror"]] as const) {
          await playAlbum(page, index);
          await page.keyboard.press("f");
          await page.getByTestId("fullscreen-player").waitFor();
          const on = await page.getByTestId("fullscreen-player").getAttribute("data-mode");
          if (on !== "art") await page.getByTestId(`fs-mode-${on}`).click();
          await page.waitForTimeout(800);
          await shot(`fs-immersive-${name}-art`);
          await page.getByTestId("fs-mode-queue").click();
          await shot(`fs-immersive-${name}-queue`);
          await page.getByTestId("fs-mode-queue").click();
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
