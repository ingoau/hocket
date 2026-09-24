import AxeBuilder from "@axe-core/playwright";
import { completeSetup, expect, test } from "./fixtures";
import type { Page } from "@playwright/test";

async function scan(page: Page, name: string) {
  const r = await new AxeBuilder({ page }).setLegacyMode(true).analyze();
  for (const v of r.violations) {
    console.log(`[${name}] ${v.impact} ${v.id}: ${v.help} (${v.nodes.length})`);
    for (const n of v.nodes.slice(0, 4)) console.log(`     ${n.target.join(" ")} :: ${n.failureSummary?.split("\n").slice(1, 3).join(" | ")}`);
  }
}

test("explore", async ({ hocket }) => {
  const { page } = hocket;
  await scan(page, "setup");
  await completeSetup(page);
  const servers = await page.evaluate(async () => (await window.hocket.query({ type: "servers" })) as { type: string; data: { id: string }[] });
  const serverId = servers.data[0]!.id;
  await page.evaluate((sid) => window.hocket.dispatch({ type: "playTracks", data: { server_id: sid, track_ids: ["tr-1"], start_index: 0, label: "e2e", shuffle: false } }), serverId);
  await expect(page.getByTestId("queue-row-current")).toBeVisible();
  await page.waitForTimeout(2500);
  await scan(page, "home");
  for (const v of ["albums", "artists", "songs", "playlists", "genres"]) {
    await page.getByTestId(`nav-${v}`).click();
    await page.waitForTimeout(800);
    await scan(page, v);
  }
  await page.getByTestId("nav-albums").click();
  await page.getByTestId("grid-tile").first().dblclick();
  await page.waitForTimeout(800);
  await scan(page, "album");
  await page.getByTestId("content").click();
  await page.keyboard.press("f");
  await page.waitForTimeout(1500);
  await scan(page, "fullscreen");
});
