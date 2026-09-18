// Application menu. Deliberately minimal: the app's own shortcuts live in the
// renderer (rebindable, generated from the action registry), and menu
// accelerators would intercept keys before the renderer sees them. macOS
// needs the Edit roles for copy/paste in text fields; everywhere else the
// menu bar is hidden.
import { Menu, app } from "electron";

export function installAppMenu(handlers: { showMain(): void; openSettings(): void; quit(): void }): void {
  if (process.platform !== "darwin") {
    Menu.setApplicationMenu(null);
    return;
  }
  Menu.setApplicationMenu(
    Menu.buildFromTemplate([
      {
        label: app.name,
        submenu: [
          { role: "about" },
          { type: "separator" },
          { label: "Settings…", accelerator: "Cmd+,", click: () => handlers.openSettings() },
          { type: "separator" },
          { role: "hide" },
          { role: "hideOthers" },
          { role: "unhide" },
          { type: "separator" },
          { label: "Quit Hocket", accelerator: "Cmd+Q", click: () => handlers.quit() },
        ],
      },
      {
        label: "Edit",
        submenu: [{ role: "cut" }, { role: "copy" }, { role: "paste" }, { role: "pasteAndMatchStyle" }],
      },
      {
        label: "Window",
        submenu: [{ role: "minimize" }, { role: "zoom" }, { type: "separator" }, { label: "Show Hocket", click: () => handlers.showMain() }],
      },
    ]),
  );
}
