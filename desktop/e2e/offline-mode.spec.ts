// Offline mode on the desktop (FakeCore): the switch in the activity popover and in Settings, the
// banner that says which kind of offline it is (and the way back), rows dimmed when they can't
// play, and downloads that wait for a connection instead of running.
import type { Page } from "@playwright/test";
import { chooseOption, completeSetup, expect, test, openPlace } from "./fixtures";
import { expectNoViolations, serverId } from "./a11y-helpers";

/** An album with nothing downloaded or cached. */
async function albumWithNothingOffline(page: Page): Promise<string> {
  const sid = await serverId(page);
  return page.evaluate(async (sid) => {
    const albums = (await window.hocket.query({ type: "albums", data: { server_id: sid, sort: "default", descending: false, page: { offset: 0, limit: 300 } } })) as { data: { items: { id: string }[] } };
    for (const a of albums.data.items) {
      const tr = (await window.hocket.query({ type: "albumTracks", data: { id: a.id } })) as { data: { offline: string }[] };
      if (tr.data.every((x) => x.offline === "none")) return a.id;
    }
    return "";
  }, sid);
}

test.describe("offline mode", () => {
  test("the switch takes the app offline, says so, dims what can't play and holds downloads", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await expect(page.getByTestId("offline-banner")).toHaveCount(0);

    // The quick switch, in the activity popover.
    await page.getByTestId("jobs-button").click();
    const quick = page.getByTestId("jobs-offline-mode");
    await expect(quick).not.toBeChecked();
    await quick.check();
    await expect(quick).toBeChecked();
    await expect(page.getByTestId("offline-banner")).toHaveAttribute("data-reason", "offlineMode");
    await expect(page.getByTestId("offline-banner-text")).toHaveText("Offline mode");
    await expect.poll(() => page.evaluate(async () => ((await window.hocket.query({ type: "setting", data: { key: "network.offlineMode" } })) as { data?: { value: string } }).data?.value)).toBe("true");
    await page.keyboard.press("Escape");

    // Songs that aren't on this device are dimmed; downloaded and cached ones aren't.
    await openPlace(page, "songs");
    const table = page.getByTestId("songs-table");
    await expect(table.locator(".tr.offline-unavailable").first()).toBeVisible();
    await expect(table.locator(".tr.offline-unavailable").first()).toHaveAttribute("title", "Not available offline");
    await expect(table.locator(".tr.offline-unavailable").getByTestId("offline-badge")).toHaveCount(0);
    await expectNoViolations(page, "offline mode songs");

    // A download waits for a connection instead of running.
    const album = await albumWithNothingOffline(page);
    expect(album).not.toBe("");
    await page.evaluate((id) => window.hocket.dispatch({ type: "pin", data: { target: { type: "album", data: { id } }, transcode: false } }), album);
    await page.getByTestId("jobs-button").click();
    const state = page.getByTestId("job-row").first().getByTestId("job-state");
    await expect(state).toHaveText("Waiting for a connection");
    await page.keyboard.press("Escape");
    // The Downloads view says it once, over the list (a pin doesn't say which job is its own).
    await openPlace(page, "downloads");
    await expect(page.getByTestId("downloads-waiting")).toHaveText("Waiting for a connection");
    await openPlace(page, "songs");
    await expect(table.locator(".tr.offline-unavailable").first()).toBeVisible();

    // The banner's way back: online again, the banner goes (without a "No connection" on the way,
    // while the core's NetworkChanged follows the setting) and the download goes ahead.
    await page.evaluate(() => {
      const seen: string[] = [];
      (window as unknown as { seenReasons: string[] }).seenReasons = seen;
      new MutationObserver(() => {
        const reason = document.querySelector('[data-testid="offline-banner"]')?.getAttribute("data-reason");
        if (reason && seen[seen.length - 1] !== reason) seen.push(reason);
      }).observe(document.body, { subtree: true, childList: true, attributes: true, attributeFilter: ["data-reason"] });
    });
    await page.getByTestId("offline-mode-off").click();
    await expect(page.getByTestId("offline-banner")).toHaveCount(0);
    expect(await page.evaluate(() => (window as unknown as { seenReasons: string[] }).seenReasons)).not.toContain("noConnection");
    await expect(table.locator(".tr.offline-unavailable")).toHaveCount(0);
    await page.getByTestId("jobs-button").click();
    await expect(state).toHaveText(/Running|Done/);
    await page.keyboard.press("Escape");

    // The same setting in Settings.
    await page.getByTestId("nav-settings").click();
    const setting = page.getByTestId("setting-network.offlineMode");
    await expect(setting).not.toBeChecked();
    await setting.check();
    await expect(page.getByTestId("offline-banner-text")).toHaveText("Offline mode");
    await setting.uncheck();
    await expect(page.getByTestId("offline-banner")).toHaveCount(0);
  });

  test("library refresh, full reconcile and server search are in Settings", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await page.getByTestId("nav-settings").click();
    const interval = page.getByTestId("setting-syncInterval");
    await expect(interval).toHaveAttribute("data-value", "60");
    await chooseOption(interval, "360");
    await expect.poll(() => page.evaluate(async () => ((await window.hocket.query({ type: "setting", data: { key: "library.syncIntervalMinutes" } })) as { data?: { value: string } }).data?.value)).toBe("360");
    const reconcile = page.getByTestId("setting-fullReconcile");
    await expect(reconcile).toHaveAttribute("data-value", "7");
    await chooseOption(reconcile, "14");
    await expect.poll(() => page.evaluate(async () => ((await window.hocket.query({ type: "setting", data: { key: "library.fullReconcileDays" } })) as { data?: { value: string } }).data?.value)).toBe("14");
    const search = page.getByTestId("setting-search.includeServer");
    await expect(search).toBeChecked();
    await search.uncheck();
    await expect(search).not.toBeChecked();
  });
});
