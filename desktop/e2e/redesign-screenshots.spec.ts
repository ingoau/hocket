// Screenshots of the redesigned screens (Queues, Play on, Settings, the
// detail pages, Search, Setup, Downloads, Stats, Filters, Song info) for
// reviewing by eye against the final designs. Not an assertion test: it runs
// only with an output folder. HOCKET_SCREENSHOT_ONLY limits which shots are
// written (comma-separated name fragments):
//
//   pnpm build && HOCKET_SCREENSHOT_DIR=/tmp/shots xvfb-run -a pnpm test:e2e redesign-screenshots
import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test, type Page } from "@playwright/test";
import type { ServerInfo } from "../src/core/api";
import { completeSetup, expect, launchFake, resizeWindow } from "./fixtures";

const out = process.env.HOCKET_SCREENSHOT_DIR;
const only = process.env.HOCKET_SCREENSHOT_ONLY?.split(",").map((s) => s.trim()).filter(Boolean);
const wanted = (name: string) => !only?.length || only.some((f) => name.includes(f));

test.use({ trace: "off" });

async function settle(page: Page): Promise<void> {
  await page.waitForTimeout(400);
  await page.evaluate(() => Promise.race([
    Promise.all(document.getAnimations().filter((a) => a.effect?.getTiming().iterations !== Number.POSITIVE_INFINITY).map((a) => a.finished.catch(() => undefined))),
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
  const id = `al-${index + 1}`;
  await page.evaluate(({ serverId, id, label }) => window.hocket.dispatch({ type: "playContext", data: { args: { context: { serverId, kind: { type: "album", data: { id } }, label, sort: "default" }, shuffle: false, saveOutgoing: true } } }), { serverId, id, label });
  await expect.poll(async () => ((await page.evaluate(() => window.hocket.query({ type: "snapshot" }))) as { data: { queue: { current?: { track: { albumId?: string } } } } }).data.queue.current?.track.albumId).toBe(id);
}

test("redesigned screens", async () => {
  test.skip(!out, "set HOCKET_SCREENSHOT_DIR");
  test.setTimeout(0);
  mkdirSync(out!, { recursive: true });
  for (const theme of ["light", "dark"] as const) {
    const userData = mkdtempSync(join(tmpdir(), "hocket-redesign-shots-"));
    const { app, page } = await launchFake(userData);
    const shot = async (name: string) => {
      if (!wanted(name)) return;
      await settle(page);
      await page.screenshot({ path: join(out!, `desktop-${theme}-${name}.png`) });
    };
    try {
      await completeSetup(page);
      await page.addStyleTag({ content: '[data-testid="toasts"], [data-testid="dev-banner"] { display: none !important; }' });
      await setSetting(page, "display.theme", theme);
      await resizeWindow(app, page, 1456, 912);
      await playAlbum(page, 3, "Late night drive");
      await playAlbum(page, 0, "Road trip");

      // Queues: the side panel's queue name opens the page.
      await page.getByTestId("toggle-queue").click();
      await page.getByTestId("right-panel").getByTestId("queue-selector").click();
      await expect(page.getByTestId("saved-queues")).toBeVisible();
      await shot("queues");
      await page.getByTestId("sidebar-back").click();

      // Play on.
      await page.getByTestId("connect-button").click();
      await expect(page.getByTestId("connect-self")).toBeVisible();
      await shot("connect");
      await page.keyboard.press("Escape");

      // Search: the kind chips and sections, then the recent searches.
      await page.getByTestId("search-input").fill("the");
      await page.keyboard.press("Enter");
      await expect(page.getByTestId("view-search")).toBeVisible();
      await page.getByTestId("search-input").blur();
      await shot("search");
      await page.getByTestId("search-input").fill("");
      await page.getByTestId("search-input").click();
      await expect(page.getByTestId("search-recent").first()).toBeVisible();
      await shot("search-recent");
      await page.keyboard.press("Escape");

      // Settings: grouped nav with the account card.
      await page.getByTestId("nav-settings").click();
      await expect(page.getByTestId("view-settings")).toBeVisible();
      await shot("settings");
      await page.getByTestId("settings-nav-audio").click();
      await shot("settings-audio");
    } finally {
      await app.close().catch(() => undefined);
      rmSync(userData, { recursive: true, force: true });
    }
  }
});
