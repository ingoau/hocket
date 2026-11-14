// Set up another device: the account card's QR code of this device's setup
// link, and a setup link pasted into a new device's server address field.
import { deflateSync } from "node:zlib";
import { completeSetup, expect, test } from "./fixtures";

test.describe("set up another device", () => {
  test("the account card shows a setup code with the server and coordinator", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    // One account setting changed (it rides in the code, with no coordinator), one device setting (it doesn't).
    await page.evaluate(() => {
      window.hocket.dispatch({ type: "setSetting", data: { key: "queue.savedCap", value: "5" } });
      window.hocket.dispatch({ type: "setSetting", data: { key: "display.theme", value: JSON.stringify("dark") } });
    });
    await page.getByTestId("nav-settings").click();
    await page.getByTestId("settings-account-setup-device").click();
    const dialog = page.getByTestId("dialog-setupDevice");
    await expect(dialog).toBeVisible();
    await expect(dialog.getByTestId("setup-device-code").locator("svg path")).toHaveCount(1);
    await expect(dialog).toContainText("alice on music.example.org");
    await expect(dialog.getByTestId("setup-device-coordinator")).toContainText("None");
    await expect(dialog.getByTestId("setup-device-settings")).toHaveText("1 changed setting comes along");
    // The password stays out until switched on.
    await expect(dialog.getByTestId("setup-device-password")).not.toBeChecked();
    await dialog.getByTestId("setup-device-copy").click();
    await expect(page.getByText("Setup link copied")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(dialog).toHaveCount(0);
  });

  test("a pasted setup link with the password connects without typing", async ({ hocket }) => {
    const { page } = hocket;
    await expect(page.getByTestId("setup")).toBeVisible();
    await page.getByTestId("setup-url").fill("hocket://setup?server=https%3A%2F%2Fmusic.example.org&user=alice&password=secret&coordinator=wss%3A%2F%2Fcoord.example.org");
    await expect(page.getByTestId("setup-summary")).toBeVisible({ timeout: 15_000 });
    await page.getByTestId("setup-continue").click({ timeout: 15_000 });
    // "Add another source?": not now.
    await page.getByTestId("setup-done").click({ timeout: 15_000 });
    await expect(page.getByTestId("app")).toBeVisible({ timeout: 15_000 });
  });

  test("a setup link's settings are applied as it connects", async ({ hocket }) => {
    const { page } = hocket;
    // Packed as SetupLink.kt and shared/setup-settings do: zlib, then unpadded base64url.
    const settings = deflateSync(JSON.stringify({ "queue.savedCap": 7, "display.theme": "dark" })).toString("base64url");
    await page.getByTestId("setup-url").fill(`hocket://setup?server=https%3A%2F%2Fmusic.example.org&user=alice&password=secret&settings=${settings}`);
    await page.getByTestId("setup-continue").click({ timeout: 15_000 });
    // "Add another source?": not now.
    await page.getByTestId("setup-done").click({ timeout: 15_000 });
    await expect(page.getByTestId("app")).toBeVisible({ timeout: 15_000 });
    const value = (key: string) => page.evaluate(async (k) => {
      const r = await window.hocket.query({ type: "setting", data: { key: k } });
      return r.type === "settingDetail" ? r.data?.value : undefined;
    }, key);
    await expect.poll(() => value("queue.savedCap")).toBe("7");
    // Device-local settings stay this device's own.
    expect(await value("display.theme")).toBe(JSON.stringify("system"));
  });

  test("an opened setup link fills the form and waits for Connect; a signed-in device turns it away", async ({ hocket }) => {
    const { app, page } = hocket;
    const open = (url: string) => app.evaluate(({ BrowserWindow }, u) => { for (const w of BrowserWindow.getAllWindows()) w.webContents.send("hocket:deep-link", u); }, url);
    await expect(page.getByTestId("setup")).toBeVisible();
    // Its settings (like a coordinator) don't show in the form, so a link from outside doesn't bring them.
    const settings = deflateSync(JSON.stringify({ "queue.savedCap": 7 })).toString("base64url");
    await open(`hocket://setup?server=https%3A%2F%2Fmusic.example.org&user=alice&password=secret&settings=${settings}`);
    await expect(page.getByTestId("setup-url")).toHaveValue("https://music.example.org");
    await expect(page.getByTestId("setup-password")).toHaveValue("secret");
    // Any web page can open a hocket:// link: nothing connects until the person presses Connect.
    await page.waitForTimeout(500);
    await expect(page.getByTestId("setup-summary")).toHaveCount(0);
    await page.getByTestId("setup-connect").click();
    await page.getByTestId("setup-continue").click({ timeout: 15_000 });
    // "Add another source?": not now.
    await page.getByTestId("setup-done").click({ timeout: 15_000 });
    await expect(page.getByTestId("app")).toBeVisible({ timeout: 15_000 });
    const cap = await page.evaluate(async () => {
      const r = await window.hocket.query({ type: "setting", data: { key: "queue.savedCap" } });
      return r.type === "settingDetail" ? r.data?.value : undefined;
    });
    expect(cap).toBe("10");
    await open("hocket://setup?server=https%3A%2F%2Fother.example.org&user=bob");
    await expect(page.getByText("This device is already set up", { exact: false })).toBeVisible();
  });

  test("a pasted setup link without the password fills the form", async ({ hocket }) => {
    const { page } = hocket;
    await page.getByTestId("setup-url").fill("hocket://setup?server=https%3A%2F%2Fmusic.example.org&user=alice");
    await expect(page.getByTestId("setup-url")).toHaveValue("https://music.example.org");
    await expect(page.getByTestId("setup-username")).toHaveValue("alice");
    await expect(page.getByTestId("setup-password")).toHaveValue("");
    await expect(page.getByTestId("setup")).toBeVisible();
  });
});
