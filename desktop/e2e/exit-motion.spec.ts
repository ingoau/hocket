// Closed surfaces animate out: a dialog, menu, popover, toast or the fullscreen
// player stays mounted for one exit animation, inert and untouchable, then
// unmounts (lib/presence.ts). Under reduced motion it goes at once. The exits
// are slowed here so the closing state is observable.
import type { Locator, Page } from "@playwright/test";
import { completeSetup, expect, playFirstAlbum, test } from "./fixtures";

/** Stretch every exit so the test can look at it mid-flight. */
async function slowExits(page: Page): Promise<void> {
  await page.evaluate(() => {
    document.documentElement.style.setProperty("--dur-exit", "1500ms");
    document.documentElement.style.setProperty("--dur-short", "1500ms");
  });
}

/** The element's closing state: the class, inertness, and an animation of its own running. */
async function exitState(el: Locator): Promise<{ closing: boolean; inert: boolean; animating: boolean }> {
  return el.evaluate((n) => ({
    closing: n.classList.contains("closing"),
    inert: n.hasAttribute("inert"),
    animating: n.getAnimations().some((a) => a.playState === "running"),
  }));
}

async function expectExiting(el: Locator): Promise<void> {
  await expect.poll(() => exitState(el)).toEqual({ closing: true, inert: true, animating: true });
  await expect(el).toHaveCount(0);
}

test.describe("exit animations", () => {
  test("menus, dialogs, the palette, popovers, toasts and the fullscreen player leave through an exit animation", async ({ hocket }) => {
    const { page } = hocket;
    await completeSetup(page);
    await playFirstAlbum(page);
    await slowExits(page);

    // A toast, first while the one from playing is fresh (it auto-dismisses
    // after 5 s; hovering holds it): dismissed by its action, it sinks away
    // rather than vanishing.
    const toast = page.getByTestId("toast").filter({ hasText: "Play" }).first();
    await expect(toast).toBeVisible();
    await toast.hover();
    await toast.getByTestId("toast-action").click();
    await expect.poll(() => exitState(toast)).toEqual({ closing: true, inert: true, animating: true });
    await expect(page.locator('[data-testid="toast"].closing')).toHaveCount(0);

    // Context menu: Escape starts the exit; it is inert at once and then gone.
    await page.getByTestId("track-row").first().click({ button: "right" });
    const menu = page.getByTestId("context-menu");
    await expect(menu).toBeVisible();
    await page.keyboard.press("Escape");
    await expectExiting(menu);

    // Dialog: the sheet and its scrim leave together; the shell is live again from the first frame.
    await page.getByTestId("track-row").first().click({ button: "right" });
    await menu.locator('[data-action="addToPlaylist"]').click();
    const dialog = page.getByTestId("dialog-addToPlaylist");
    await expect(dialog).toBeVisible();
    expect(await page.getByTestId("app-body").getAttribute("inert")).not.toBeNull();
    await page.keyboard.press("Escape");
    await expect.poll(() => exitState(dialog)).toEqual({ closing: true, inert: true, animating: true });
    expect(await page.locator(".overlay").evaluate((n) => n.classList.contains("closing") && n.hasAttribute("inert"))).toBe(true);
    expect(await page.getByTestId("app-body").getAttribute("inert")).toBeNull();
    await expect(dialog).toHaveCount(0);

    // Command palette.
    await page.getByTestId("content").click();
    await page.keyboard.press("Control+k");
    const palette = page.getByTestId("palette");
    await expect(palette).toBeVisible();
    await page.keyboard.press("Escape");
    await expectExiting(palette);

    // Jobs popover.
    await page.getByTestId("jobs-button").click();
    const popover = page.getByTestId("jobs-popover");
    await expect(popover).toBeVisible();
    await page.getByTestId("jobs-button").click();
    await expectExiting(popover);

    // The fullscreen player: fades out, focus already back in the shell.
    await page.getByTestId("content").click();
    await page.keyboard.press("f");
    const fs = page.getByTestId("fullscreen-player");
    await expect(fs).toBeVisible();
    await page.keyboard.press("Escape");
    await expect.poll(() => exitState(fs)).toEqual({ closing: true, inert: true, animating: true });
    expect(await page.evaluate(() => !document.activeElement?.closest('[data-testid="fullscreen-player"]'))).toBe(true);
    await expect(fs).toHaveCount(0);
  });

  test("under reduced motion a closed surface goes at once", async ({ hocket }) => {
    const { page } = hocket;
    await page.emulateMedia({ reducedMotion: "reduce" });
    await completeSetup(page);
    await slowExits(page);
    await page.getByTestId("content").click();
    await page.keyboard.press("Control+k");
    const palette = page.getByTestId("palette");
    await expect(palette).toBeVisible();
    await page.keyboard.press("Escape");
    // Well inside the stretched exit: nothing waited for an animation that never ran.
    await expect(palette).toHaveCount(0, { timeout: 500 });
    expect(await page.evaluate(() => document.getAnimations().filter((a) => a.playState === "running").length)).toBe(0);
  });
});
