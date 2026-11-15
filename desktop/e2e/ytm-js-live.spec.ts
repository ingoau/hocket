// LIVE check of YouTube Music's JS path on desktop (docs/providers.md, Phase 4 follow-up): the real
// core, anonymous YouTube Music with the hidden-window JS host, every client but the web one
// skipped (HOCKET_YTM_FORCE_WEB), and a real track played. Passes when the core logs the stream as
// resolved by the web client, i.e. the player's signature/n functions ran in the hidden window.
// Talks to YouTube, so it only runs when asked:
//   HOCKET_LIVE_YTM=1 xvfb-run -a -s "-screen 0 2000x1600x24" pnpm test:e2e e2e/ytm-js-live.spec.ts
import { _electron as electron, expect, test } from "@playwright/test";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const root = resolve(__dirname, "..");
const addonBuilt = existsSync(join(root, "native", "index.js"));

test.describe("YouTube Music JS host (live)", () => {
  test.skip(!process.env.HOCKET_LIVE_YTM, "live check: set HOCKET_LIVE_YTM=1");
  test.skip(!addonBuilt, "native addon not built (run pnpm gen)");
  test.setTimeout(180_000);

  test("the web client resolves anonymously with the JS host solving its puzzles", async () => {
    const userData = mkdtempSync(join(tmpdir(), "hocket-live-"));
    const env: Record<string, string> = {};
    for (const [k, v] of Object.entries(process.env)) if (v !== undefined && k !== "HOCKET_FAKE_CORE") env[k] = v;
    Object.assign(env, {
      HOCKET_USER_DATA: userData,
      HOCKET_DEVICE_NAME: "e2e-live",
      HOCKET_LOG: "warn,hocket::ytm=debug,hocket::stream=info",
      HOCKET_YTM_FORCE_WEB: "1",
      HOCKET_INSECURE_CREDENTIAL_STORE: "1",
    });
    const app = await electron.launch({ args: [root, "--no-sandbox", "--disable-gpu", "--enable-unsafe-swiftshader"], env });
    let log = "";
    app.process().stdout?.on("data", (d: Buffer) => (log += d.toString()));
    app.process().stderr?.on("data", (d: Buffer) => (log += d.toString()));
    try {
      const page = await app.firstWindow();
      await page.waitForLoadState("domcontentloaded");
      await page.evaluate(() =>
        window.hocket.dispatch({ type: "addAccount", data: { setup: { type: "youtubeMusic", data: { cookies: undefined, js_host: true } } } }),
      );
      await expect
        .poll(async () => JSON.stringify(await page.evaluate(() => window.hocket.query({ type: "accounts" }))), { timeout: 30_000 })
        .toContain("youtubeMusic");
      // Daft Punk, "Get Lucky" (the spike's test track): into the catalogue by a live search, then play it.
      await page.evaluate(() =>
        window.hocket.query({ type: "search", data: { server_id: "", query: "daft punk get lucky", limit: 5, include_server: true, request_id: "live" } }),
      );
      await page.waitForTimeout(5_000);
      await page.evaluate(() =>
        window.hocket.dispatch({
          type: "playTracks",
          data: { tracks: [{ source: "ytm", id: "4D7u5KF7SP8" }], start_index: 0, label: "live", shuffle: false, start_position_ms: 0 },
        }),
      );
      await expect.poll(() => log, { timeout: 90_000, intervals: [1_000] }).toMatch(/stream resolved.*WebRemix|client=WebRemix.*stream resolved/);
      expect(log).not.toMatch(/web: the JS host signed nothing/);
      // Let the stream reader fetch with the signed URL, then show what happened.
      await page.waitForTimeout(10_000);
      console.log(log.split("\n").filter((l) => /ytm-js|hocket::ytm|hocket::stream|refused/.test(l)).join("\n"));
      expect(log).toMatch(/\[ytm-js\] prepared: signatureTimestamp \d+, PO token minted/);
      // Past the first MiB without a refusal: the PO token passed googlevideo's wall.
      expect(log).not.toMatch(/stream URL refused/);
    } finally {
      await app.close();
      rmSync(userData, { recursive: true, force: true });
    }
  });
});
