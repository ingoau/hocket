import { completeSetup, expect, openPlace, playFirstAlbum, test } from "./fixtures";

test.describe("resilience", () => {
  test("a page that fails to render shows its error in place; the rest of the window keeps working", async ({ hocket }) => {
    const { app, page } = hocket;
    await completeSetup(page);
    await playFirstAlbum(page);
    await openPlace(page, "albums");
    await expect(page.getByTestId("grid-tile").nth(1)).toBeVisible();
    // The core answers with an album the page cannot render (a version skew, a bug).
    await app.evaluate(({ ipcMain }) => {
      const handlers = (ipcMain as unknown as { _invokeHandlers: Map<string, (e: unknown, q: { type: string }) => Promise<unknown>> })._invokeHandlers;
      const real = handlers.get("hocket:query")!;
      (globalThis as unknown as { __realQuery: typeof real }).__realQuery = real;
      ipcMain.removeHandler("hocket:query");
      ipcMain.handle("hocket:query", async (e, q: { type: string }) => {
        const r = (await real(e, q)) as { type: string; data: Record<string, unknown> };
        return q.type === "album" ? { ...r, data: { ...r.data, name: { not: "a name" } } } : r;
      });
    });
    await page.getByTestId("grid-tile").nth(1).dblclick();
    const error = page.getByTestId("view-error");
    await expect(error).toBeVisible();
    await expect(page.getByRole("alert").filter({ has: error })).toHaveCount(1);
    // The player bar and the sidebar still work.
    await expect(page.getByTestId("app")).toBeVisible();
    await page.getByTestId("play-pause").click();
    await expect(page.getByTestId("play-pause")).toHaveAttribute("aria-label", "Play");
    // Fixed (the answer is right again), Try again shows the page.
    await app.evaluate(({ ipcMain }) => {
      const real = (globalThis as unknown as { __realQuery: (e: unknown, q: unknown) => Promise<unknown> }).__realQuery;
      ipcMain.removeHandler("hocket:query");
      ipcMain.handle("hocket:query", real);
    });
    await page.getByTestId("view-error-retry").click();
    await expect(page.getByTestId("view-album")).toBeVisible();
    await expect(page.getByTestId("view-error")).toHaveCount(0);
  });

  test("a crashed renderer is loaded again and reattaches to the core", async ({ hocket }) => {
    const { app, page } = hocket;
    await completeSetup(page);
    await playFirstAlbum(page);
    const title = await page.getByTestId("player-bar").locator(".now .title").textContent();
    expect(title).toBeTruthy();
    await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows().find((w) => w.isVisible())?.webContents.forcefullyCrashRenderer());
    // Same window, a new renderer: the library and what is playing come back. (Playwright's page
    // stays the crashed one, so the window is read through main; a script sent to the renderer
    // as it goes may never answer, hence the race.)
    const shown = () => app.evaluate(async ({ BrowserWindow }) => {
      const wc = BrowserWindow.getAllWindows().find((w) => w.isVisible())?.webContents;
      if (!wc || wc.isCrashed()) return "";
      const read = wc.executeJavaScript(`document.querySelector('[data-testid="app"] [data-testid="player-bar"] .now .title')?.textContent ?? ""`) as Promise<string>;
      return Promise.race([read, new Promise<string>((r) => setTimeout(() => r(""), 500))]);
    }).catch(() => "");
    await expect.poll(shown, { timeout: 20_000 }).toBe(title);
  });
});
