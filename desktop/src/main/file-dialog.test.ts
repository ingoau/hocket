import { beforeEach, describe, expect, it, vi } from "vitest";

const dialogs = { showOpenDialog: vi.fn(), showSaveDialog: vi.fn() };
vi.mock("electron", () => ({ dialog: dialogs }));
const { forgetLastFolder, showOpenDialog, showSaveDialog } = await import("./file-dialog");

const win = {} as Electron.BrowserWindow;
const defaultPathOf = (fn: typeof dialogs.showOpenDialog) => (fn.mock.lastCall![1] as { defaultPath?: string }).defaultPath;

describe("file dialogs", () => {
  beforeEach(() => {
    forgetLastFolder();
    dialogs.showOpenDialog.mockReset();
    dialogs.showSaveDialog.mockReset();
  });

  it("start where the user last picked from, keeping the suggested name", async () => {
    dialogs.showSaveDialog.mockResolvedValue({ canceled: false, filePath: "/home/u/Music/out.json" });
    await showSaveDialog(win, { defaultPath: "hocket-config.json" });
    expect(defaultPathOf(dialogs.showSaveDialog)).toBe("hocket-config.json");
    await showSaveDialog(win, { defaultPath: "../log.txt" });
    expect(defaultPathOf(dialogs.showSaveDialog)).toBe("/home/u/Music/log.txt");
    dialogs.showOpenDialog.mockResolvedValue({ canceled: false, filePaths: ["/tmp/shots/a.png"] });
    await showOpenDialog(win, {});
    expect(defaultPathOf(dialogs.showOpenDialog)).toBe("/home/u/Music");
    await showOpenDialog(win, {});
    expect(defaultPathOf(dialogs.showOpenDialog)).toBe("/tmp/shots");
  });

  it("a cancelled dialog changes nothing, and an absolute path is left alone", async () => {
    dialogs.showOpenDialog.mockResolvedValueOnce({ canceled: false, filePaths: ["/a/b.json"] }).mockResolvedValue({ canceled: true, filePaths: [] });
    await showOpenDialog(win, {});
    await showOpenDialog(win, {});
    await showOpenDialog(win, { defaultPath: "/elsewhere/x.json" });
    expect(defaultPathOf(dialogs.showOpenDialog)).toBe("/elsewhere/x.json");
    await showOpenDialog(win, {});
    expect(defaultPathOf(dialogs.showOpenDialog)).toBe("/a");
  });
});
