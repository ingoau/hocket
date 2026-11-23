import { describe, expect, it, vi } from "vitest";

vi.mock("electron", () => ({ app: { name: "Hocket" }, Menu: { setApplicationMenu: vi.fn(), buildFromTemplate: vi.fn() } }));

import type { MenuItemConstructorOptions } from "electron";
import { appMenuTemplate } from "./menu";

const noop = { showMain() {}, openSettings() {}, setOfflineMode() {}, quit() {} };

function roles(label: string): string[] {
  const menu = appMenuTemplate(noop).find((m) => m.label === label);
  return ((menu?.submenu ?? []) as MenuItemConstructorOptions[]).flatMap((i) => (i.role ? [i.role] : []));
}

describe("macOS app menu", () => {
  // R-20261104-9ay9: Cmd+A did nothing in text fields because macOS only
  // performs editing keys that an Edit menu item claims.
  it("gives text fields every standard editing key", () => {
    expect(roles("Edit")).toEqual(expect.arrayContaining(["undo", "redo", "cut", "copy", "paste", "delete", "selectAll"]));
  });

  it("adds no accelerators beyond Settings and Quit", () => {
    const accels = appMenuTemplate(noop)
      .flatMap((m) => (m.submenu ?? []) as MenuItemConstructorOptions[])
      .flatMap((i) => (i.accelerator ? [String(i.accelerator)] : []));
    expect(accels.sort()).toEqual(["Cmd+,", "Cmd+Q"]);
  });

  it("has an Offline Mode checkbox that shows the setting and flips it", () => {
    const calls: boolean[] = [];
    const item = (on: boolean) => ((appMenuTemplate({ ...noop, setOfflineMode: (v) => calls.push(v) }, { offlineMode: on })[0]?.submenu ?? []) as MenuItemConstructorOptions[]).find((i) => i.label === "Offline Mode");
    expect(item(false)).toMatchObject({ type: "checkbox", checked: false });
    expect(item(true)?.checked).toBe(true);
    // Electron flips the item before the click handler runs: it reports the new state.
    item(false)?.click?.({ checked: true } as never, undefined, {} as never);
    expect(calls).toEqual([true]);
  });
});
