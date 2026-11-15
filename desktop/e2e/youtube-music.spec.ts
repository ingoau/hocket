// YouTube Music in setup (docs/providers.md, Phase 4): the "Where's your
// music?" card offers signing in with Google or carrying on without an
// account; either way the account is added and survives a restart (the
// sign-in's cookies stay in the OS-encrypted store on this device).
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { completeSetup, expect, launchFake, test } from "./fixtures";

test.describe("YouTube Music setup", () => {
  test("continue without an account adds anonymous YouTube Music", async ({ hocket }) => {
    const { page } = hocket;
    await expect(page.getByTestId("setup")).toBeVisible();
    await page.getByTestId("setup-source-ytm").click();
    await expect(page.getByTestId("setup-ytm")).toBeVisible();
    await page.getByTestId("setup-ytm-anon").click();
    await expect(page.getByTestId("setup-summary")).toBeVisible({ timeout: 15_000 });
    await expect(page.getByTestId("setup-ytm-who")).toContainText("Not signed in");
    await page.getByTestId("setup-continue").click();
    await expect(page.getByTestId("setup-another")).toBeVisible();
    await page.getByTestId("setup-done").click();
    await expect(page.getByTestId("app")).toBeVisible({ timeout: 15_000 });
  });

  test("signing in adds the account and the sign-in is replayed after a restart", async () => {
    const userData = mkdtempSync(join(tmpdir(), "hocket-e2e-"));
    const env = { HOCKET_FAKE_YTM_SIGNIN: "1", HOCKET_INSECURE_CREDENTIAL_STORE: "1" };
    try {
      let { app, page } = await launchFake(userData, env);
      await completeSetup(page);
      // Settings → Accounts → Add uses the same cards; "Add another source?" is the same list.
      await page.evaluate(() => window.hocket.dispatch({ type: "requestSnapshot" }));
      const signIn = await page.evaluate(() => window.hocket.ytm.signIn());
      expect(signIn).toBe("signedIn");
      await expect
        .poll(async () => (await page.evaluate(() => window.hocket.query({ type: "accounts" }))) as unknown as { data: { kind: string; username: string }[] }, { timeout: 15_000 })
        .toMatchObject({ data: expect.arrayContaining([expect.objectContaining({ kind: "youtubeMusic", username: "Test User" })]) });
      await app.close();
      ({ app, page } = await launchFake(userData, env));
      await expect(page.getByTestId("app")).toBeVisible({ timeout: 15_000 });
      await expect
        .poll(async () => (await page.evaluate(() => window.hocket.query({ type: "accounts" }))) as unknown as { data: { kind: string }[] }, { timeout: 15_000 })
        .toMatchObject({ data: expect.arrayContaining([expect.objectContaining({ kind: "youtubeMusic" })]) });
      await app.close();
    } finally {
      rmSync(userData, { recursive: true, force: true });
    }
  });

  test("the YouTube Music card is offered when adding another source", async ({ hocket }) => {
    const { page } = hocket;
    await page.getByTestId("setup-url").fill("https://music.example.org");
    await page.getByTestId("setup-username").fill("alice");
    await page.getByTestId("setup-password").fill("secret");
    await page.getByTestId("setup-connect").click();
    await page.getByTestId("setup-continue").click({ timeout: 15_000 });
    await page.getByTestId("setup-another-ytm").click();
    await expect(page.getByTestId("setup-ytm")).toBeVisible();
    await expect(page.getByTestId("setup-ytm-signin")).toBeEnabled();
    await page.getByTestId("setup-ytm-anon").click();
    await expect(page.getByTestId("setup-ytm-who")).toBeVisible({ timeout: 15_000 });
  });
});
