// Pairing and the identity's devices: Add a device shows a pairing code (no
// password in it), a new device pairs from the setup screen, and Settings →
// Connect lists the devices, credential sync and the coordinator.
import { completeSetup, expect, test } from "./fixtures";

test.describe("pairing", () => {
  test("the account card shows a pairing code with no password in it", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await page.getByTestId("nav-settings").click();
    await page.getByTestId("settings-account-setup-device").click();
    const dialog = page.getByTestId("dialog-pairDevice");
    await expect(dialog).toBeVisible();
    await expect(dialog.getByTestId("pair-device-code").locator("svg path")).toHaveCount(1);
    await expect(dialog.getByTestId("pair-device-status")).toContainText("Waiting for the other device");
    await dialog.getByTestId("pair-device-copy").click();
    await expect(page.getByText("Pairing link copied")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(dialog).toHaveCount(0);
  });

  test("a new device pairs from the setup screen, then signs in", async ({ hocket }) => {
    const { page } = hocket;
    await expect(page.getByTestId("setup")).toBeVisible();
    await page.getByTestId("setup-source-device").click();
    await page.getByTestId("setup-pair-link").fill("hocket://pair?d=ZmFrZS1wYWlyaW5nLWludml0ZQ");
    await page.getByTestId("setup-pair-join").click();
    // Paired (the fake's invite named no accounts): the Navidrome form is next.
    await expect(page.getByTestId("setup-url")).toBeVisible();
  });

  test("the same pairing failure twice still ends each attempt", async ({ hocket }) => {
    const { page } = hocket;
    await page.getByTestId("setup-source-device").click();
    await page.getByTestId("setup-pair-link").fill("hocket://pair?d=expired-fake-invite");
    const join = page.getByTestId("setup-pair-join");
    for (let i = 0; i < 2; i++) {
      await join.click();
      await expect(page.getByTestId("setup-error")).toContainText("expired");
      await expect(join).toHaveText("Pair");
      await expect(join).toBeEnabled();
    }
  });

  test("settings list this device, credential sync and the coordinator", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await page.getByTestId("nav-settings").click();
    await page.getByTestId("settings-nav-connect").click();
    await expect(page.locator("[data-testid^='identity-device-']").getByText("This device")).toBeVisible();
    const sync = page.getByTestId("identity-credential-sync");
    await expect(sync).toContainText("Off");
    await sync.getByRole("switch").click();
    await expect(sync).toContainText("Google sign-ins never");
    await page.getByTestId("identity-coordinator-url").fill("wss://coordinator.example/ws");
    await page.getByTestId("identity-coordinator-code").fill("ABCD-EFGH");
    await page.getByTestId("identity-claim").click();
    await expect(page.getByTestId("identity-coordinator")).toContainText("(yours)");
    await expect(sync).toContainText("coordinator holds the key");
    await page.getByTestId("identity-invite-create").click();
    await expect(page.getByTestId("identity-invite-FAKE-0001")).toBeVisible();
    await page.getByTestId("identity-invite-FAKE-0001").getByRole("button", { name: "Revoke" }).click();
    await expect(page.getByTestId("identity-invite-FAKE-0001")).toHaveCount(0);
  });
});
