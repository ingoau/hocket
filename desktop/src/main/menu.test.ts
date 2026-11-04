import { describe, expect, it, vi } from "vitest";

vi.mock("electron", () => ({ app: { name: "Hocket" }, Menu: { setApplicationMenu: vi.fn(), buildFromTemplate: vi.fn() } }));

import type { MenuItemConstructorOptions } from "electron";
import { appMenuTemplate } from "./menu";

const noop = { showMain() {}, openSettings() {}, quit() {} };

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
});
