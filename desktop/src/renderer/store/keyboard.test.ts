import { describe, expect, it } from "vitest";
import { firesInTextField, widgetOwnsKey } from "./keyboard";

describe("global shortcuts inside editable fields", () => {
  it("only the palette and Escape fire; every other chord, modifier or not, belongs to the field", () => {
    expect(firesInTextField("openCommandPalette")).toBe(true);
    expect(firesInTextField("ui.escape")).toBe(true);
    for (const id of ["ui.back", "ui.forward", "volumeUp", "volumeDown", "love", "undo", "redo", "redoAlt", "selectAll", "remove", "findInList", "togglePlay", "seekBackward", "rate3", "navigateSettings"]) {
      expect(firesInTextField(id), id).toBe(false);
    }
  });
});

describe("keys that belong to the focused control, not the global keymap", () => {
  const none = { ctrlKey: false, metaKey: false, altKey: false };
  it("Space and Enter press buttons, links, checkboxes, tabs, radios and menu items", () => {
    expect(widgetOwnsKey(" ", none, { tag: "button" })).toBe(true);
    expect(widgetOwnsKey("Enter", none, { tag: "a", href: true })).toBe(true);
    expect(widgetOwnsKey(" ", none, { tag: "input", type: "checkbox" })).toBe(true);
    expect(widgetOwnsKey(" ", none, { tag: "div", role: "tab" })).toBe(true);
    expect(widgetOwnsKey(" ", none, { tag: "button", role: "radio" })).toBe(true);
    expect(widgetOwnsKey("Enter", none, { tag: "div", role: "menuitem" })).toBe(true);
  });

  it("Space stays play/pause on rows, tiles, lists and plain content", () => {
    expect(widgetOwnsKey(" ", none, { tag: "div", role: "row" })).toBe(false);
    expect(widgetOwnsKey(" ", none, { tag: "div", role: "gridcell" })).toBe(false);
    expect(widgetOwnsKey(" ", none, { tag: "div", role: "listbox" })).toBe(false);
    expect(widgetOwnsKey(" ", none, { tag: "main" })).toBe(false);
    expect(widgetOwnsKey(" ", none, { tag: "a", href: false })).toBe(false);
    expect(widgetOwnsKey(" ", none, undefined)).toBe(false);
  });

  it("arrows move sliders (native and ARIA), radios, tabs, menus and splitters instead of seeking", () => {
    expect(widgetOwnsKey("ArrowLeft", none, { tag: "input", type: "range" })).toBe(true);
    expect(widgetOwnsKey("ArrowRight", none, { tag: "div", role: "slider" })).toBe(true);
    expect(widgetOwnsKey("Home", none, { tag: "button", role: "tab" })).toBe(true);
    expect(widgetOwnsKey("ArrowDown", none, { tag: "button", role: "radio" })).toBe(true);
    expect(widgetOwnsKey("ArrowRight", none, { tag: "div", role: "separator" })).toBe(true);
    expect(widgetOwnsKey("ArrowDown", none, { tag: "select" })).toBe(true);
    expect(widgetOwnsKey("ArrowLeft", none, { tag: "button" })).toBe(false);
    expect(widgetOwnsKey("ArrowLeft", none, { tag: "main" })).toBe(false);
  });

  it("chords with Ctrl, Cmd or Alt always go to the keymap", () => {
    expect(widgetOwnsKey("ArrowLeft", { ...none, altKey: true }, { tag: "input", type: "range" })).toBe(false);
    expect(widgetOwnsKey(" ", { ...none, ctrlKey: true }, { tag: "button" })).toBe(false);
    expect(widgetOwnsKey("ArrowUp", { ...none, metaKey: true }, { tag: "div", role: "slider" })).toBe(false);
  });
});
