// Application menu. Deliberately minimal: the app's own shortcuts live in the
// renderer (rebindable, generated from the action registry). macOS needs the
// Edit roles for text-field editing: Cocoa routes Cmd+A/Z/Shift+Z/X/C/V
// through the menu's key equivalents, so a key with no Edit item does nothing
// in an input. The renderer sees each key first and preventDefaults the ones
// it handles itself (Cmd+A selecting tracks, Cmd+Z undoing a library edit),
// so these items only act when focus is in a text field. Everywhere else the
// menu bar is hidden.
import { Menu, app, type MenuItemConstructorOptions } from "electron";

export interface AppMenuHandlers {
  showMain(): void;
  openSettings(): void;
  /** Turn offline mode (network.offlineMode) on or off. */
  setOfflineMode(on: boolean): void;
  quit(): void;
}

/** What the menu reflects from the core: offline mode's checkbox. */
export interface AppMenuState {
  offlineMode: boolean;
}

export function appMenuTemplate(handlers: AppMenuHandlers, state: AppMenuState = { offlineMode: false }): MenuItemConstructorOptions[] {
  return [
    {
      label: app.name,
      submenu: [
        { role: "about" },
        { type: "separator" },
        { label: "Settings…", accelerator: "Cmd+,", click: () => handlers.openSettings() },
        { label: "Offline Mode", type: "checkbox", checked: state.offlineMode, click: (item) => handlers.setOfflineMode(item.checked) },
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
      submenu: [
        { role: "undo" },
        { role: "redo" },
        { type: "separator" },
        { role: "cut" },
        { role: "copy" },
        { role: "paste" },
        { role: "pasteAndMatchStyle" },
        { role: "delete" },
        { role: "selectAll" },
      ],
    },
    {
      label: "Window",
      submenu: [{ role: "minimize" }, { role: "zoom" }, { type: "separator" }, { label: "Show Hocket", click: () => handlers.showMain() }],
    },
  ];
}

/** Installs the menu (macOS; elsewhere none) and returns how to update what it reflects. */
export function installAppMenu(handlers: AppMenuHandlers): (state: AppMenuState) => void {
  if (process.platform !== "darwin") {
    Menu.setApplicationMenu(null);
    return () => {};
  }
  let current: AppMenuState = { offlineMode: false };
  const build = () => Menu.setApplicationMenu(Menu.buildFromTemplate(appMenuTemplate(handlers, current)));
  build();
  return (state) => {
    if (state.offlineMode === current.offlineMode) return;
    current = state;
    build();
  };
}
