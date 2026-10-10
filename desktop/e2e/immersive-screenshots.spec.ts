// Screenshots of the fullscreen player with real covers and the immersive-artwork
// classifier, for reviewing hocket_core::artwork by eye. Not an assertion test: it
// runs only with a folder of covers and an output folder, against a native addon
// built with `pnpm gen` (the fake core borrows its classifier):
//
//   pnpm build && HOCKET_FAKE_COVERS=/path/to/covers HOCKET_SCREENSHOT_DIR=/tmp/shots \
//     xvfb-run -a -s "-screen 0 2000x1600x24" pnpm test:e2e immersive-screenshots
//
// Each album of the fake library wears the next cover; every one is shot in a wide
// window (artwork full height on the left) and a tall one (full width on top). Song,
// artist and album names are replaced by placeholders. HOCKET_SCREENSHOT_LIMIT caps
// how many covers are shot; HOCKET_SCREENSHOT_THEME=light|dark sets the app's theme
// (default: the system's). Covers already in the output folder are skipped, so a run
// picks up where an interrupted one stopped.
import { existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "@playwright/test";
import type { AlbumPage, ServerInfo } from "../src/core/api";
import { completeSetup, launchFake } from "./fixtures";

const covers = process.env.HOCKET_FAKE_COVERS;
const out = process.env.HOCKET_SCREENSHOT_DIR;
const limit = Number(process.env.HOCKET_SCREENSHOT_LIMIT) || Number.POSITIVE_INFINITY;
const SHAPES = [
  { name: "wide", width: 1600, height: 900 },
  { name: "tall", width: 900, height: 1400 },
];

// A trace of a run this long would hold every frame of hundreds of screenshots.
test.use({ trace: "off" });

test("fullscreen player with real covers", async () => {
  test.skip(!covers || !out, "set HOCKET_FAKE_COVERS and HOCKET_SCREENSHOT_DIR");
  test.setTimeout(0);
  mkdirSync(out!, { recursive: true });
  const total = Math.min(readdirSync(covers!).filter((f) => !f.startsWith(".")).length, limit);
  const shot = (i: number) => SHAPES.every((shape) => existsSync(join(out!, `${String(i).padStart(3, "0")}-${shape.name}.png`)));
  let offset = 0;
  let failures = 0;
  let lastFailure = -1;
  while (offset < total) {
    while (offset < total && shot(offset)) offset++;
    if (offset >= total) break;
    const userData = mkdtempSync(join(tmpdir(), "hocket-shots-"));
    const { app, page } = await launchFake(userData, { HOCKET_FAKE_COVERS: covers!, HOCKET_FAKE_COVERS_OFFSET: String(offset) });
    try {
      await completeSetup(page);
      // The play commands' own "Play Album / Undo" toasts would pile up over the artwork.
      await page.addStyleTag({ content: '[data-testid="toasts"] { display: none !important; }' });
      const theme = process.env.HOCKET_SCREENSHOT_THEME;
      if (theme === "light" || theme === "dark") await page.evaluate((value) => window.hocket.dispatch({ type: "setSetting", data: { key: "display.theme", value: JSON.stringify(value) } }), theme);
      const servers = (await page.evaluate(() => window.hocket.query({ type: "servers" }))) as { data: ServerInfo[] };
      const serverId = servers.data[0]!.id;
      const albums = ((await page.evaluate((id) => window.hocket.query({ type: "albums", data: { server_id: id, sort: "default", descending: false, page: { offset: 0, limit: 1000 } } }), serverId)) as { data: AlbumPage }).data.items;
      const count = Math.min(albums.length, total - offset);
      for (let i = 0; i < count; i++) {
        if (shot(offset + i)) continue;
        const album = albums[i]!;
        await page.evaluate(({ serverId, id }) => window.hocket.dispatch({ type: "playContext", data: { args: { context: { serverId, kind: { type: "album", data: { id } }, label: "Album", sort: "default" }, shuffle: false, saveOutgoing: false } } }), { serverId, id: album.id });
        if (i === 0) {
          await page.keyboard.press("f");
          await page.getByTestId("fullscreen-player").waitFor();
        }
        for (const shape of SHAPES) {
          await app.evaluate(({ BrowserWindow }, s) => BrowserWindow.getAllWindows()[0]?.setContentSize(s.width, s.height), shape);
          // Artwork, classifier and the continuation's canvas: let them land.
          await page.waitForTimeout(900);
          // A new continuation fades in: shoot it once it has, not halfway over the fluid background.
          await page.evaluate(() => Promise.all(document.querySelector('[data-testid="np-continuation"]')?.getAnimations().map((a) => a.finished) ?? []));
          await page.evaluate(() => {
            const set = (sel: string, text: string) => document.querySelectorAll(sel).forEach((el) => (el.textContent = text));
            set("#fs-title", "Song Name");
            set(".np-text .t2", "Artist • Album");
            set('[data-testid="np-source"]', "Album");
          });
          await page.screenshot({ path: join(out!, `${String(offset + i).padStart(3, "0")}-${shape.name}.png`) });
        }
      }
      offset += Math.max(count, 1);
    } catch (error) {
      // An app that died mid-batch: relaunch from the first cover not yet shot, and
      // give up only when relaunches stop getting anywhere.
      console.warn(`batch at ${offset} failed:`, error);
      if (offset > lastFailure) failures = 0;
      lastFailure = offset;
      if (++failures > 3) throw error;
    } finally {
      await app.close().catch(() => undefined);
      rmSync(userData, { recursive: true, force: true });
    }
  }
});
