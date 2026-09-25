import { describe, expect, it } from "vitest";
import { Keymap, chordFromEvent, chordToString, formatChord, isTextInput, parseChord } from "./shortcuts";

describe("shortcut parser", () => {
  it("parses and normalises chords", () => {
    expect(chordToString(parseChord("ctrl+shift+z")!)).toBe("Ctrl+Shift+Z");
    expect(chordToString(parseChord("Mod+K")!)).toBe("Mod+K");
    expect(chordToString(parseChord("CmdOrCtrl+,")!)).toBe("Mod+,");
    expect(chordToString(parseChord("space")!)).toBe("Space");
    expect(chordToString(parseChord("Shift+left")!)).toBe("Shift+ArrowLeft");
    expect(parseChord("Ctrl+")).toBeUndefined();
    expect(chordToString(parseChord("Ctrl++")!)).toBe("Ctrl++");
    expect(parseChord("A+B")).toBeUndefined();
    expect(parseChord("")).toBeUndefined();
  });

  it("formats per platform", () => {
    const c = parseChord("Mod+Alt+K")!;
    expect(formatChord(c, "macOs")).toBe("Cmd+Option+K");
    expect(formatChord(c, "linux")).toBe("Ctrl+Alt+K");
  });

  it("builds chords from key events", () => {
    const mac = chordFromEvent({ key: "k", code: "KeyK", ctrlKey: false, altKey: false, shiftKey: false, metaKey: true }, "macOs")!;
    expect(chordToString(mac)).toBe("Mod+K");
    const linux = chordFromEvent({ key: "K", code: "KeyK", ctrlKey: true, altKey: false, shiftKey: true, metaKey: false }, "linux")!;
    expect(chordToString(linux)).toBe("Mod+Shift+K");
    expect(chordFromEvent({ key: "Shift", ctrlKey: false, altKey: false, shiftKey: true, metaKey: false }, "linux")).toBeUndefined();
    const space = chordFromEvent({ key: " ", code: "Space", ctrlKey: false, altKey: false, shiftKey: false, metaKey: false }, "windows")!;
    expect(chordToString(space)).toBe("Space");
  });

  it("looks up bindings and reports conflicts", () => {
    const km = new Keymap([
      { actionId: "a", shortcut: "Mod+K" },
      { actionId: "b", shortcut: "mod+k" },
      { actionId: "c", shortcut: "F" },
      { actionId: "d", shortcut: undefined },
    ]);
    expect(km.lookup(parseChord("Mod+K")!)).toEqual(["a", "b"]);
    expect(km.lookup(parseChord("f")!)).toEqual(["c"]);
    expect(km.conflicts()).toEqual([{ chord: "mod+k", actionIds: ["a", "b"] }]);
  });

  it("treats a Ctrl binding as the platform modifier off macOS (the core spells its defaults that way)", () => {
    const bindings = [{ actionId: "undo", shortcut: "Ctrl+Z" }, { actionId: "palette", shortcut: "Mod+K" }];
    const ev = (platform: "macOs" | "linux" | "windows", e: { key: string; code: string; ctrlKey?: boolean; metaKey?: boolean }) =>
      chordFromEvent({ altKey: false, shiftKey: false, ctrlKey: false, metaKey: false, ...e }, platform)!;
    for (const platform of ["linux", "windows"] as const) {
      const km = new Keymap(bindings, platform);
      expect(km.lookup(ev(platform, { key: "z", code: "KeyZ", ctrlKey: true }))).toEqual(["undo"]);
      expect(km.lookup(ev(platform, { key: "k", code: "KeyK", ctrlKey: true }))).toEqual(["palette"]);
    }
    // On macOS Ctrl stays Control; Cmd is the modifier.
    const mac = new Keymap(bindings, "macOs");
    expect(mac.lookup(ev("macOs", { key: "z", code: "KeyZ", ctrlKey: true }))).toEqual(["undo"]);
    expect(mac.lookup(ev("macOs", { key: "z", code: "KeyZ", metaKey: true }))).toEqual([]);
    expect(mac.lookup(ev("macOs", { key: "k", code: "KeyK", metaKey: true }))).toEqual(["palette"]);
  });

  it("recognises text inputs", () => {
    expect(isTextInput(null)).toBe(false);
    expect(isTextInput({ tagName: "INPUT", type: "text" } as unknown as HTMLElement)).toBe(true);
    expect(isTextInput({ tagName: "INPUT", type: "range" } as unknown as HTMLElement)).toBe(false);
    expect(isTextInput({ tagName: "DIV", isContentEditable: true } as unknown as HTMLElement)).toBe(true);
    expect(isTextInput({ tagName: "BUTTON" } as unknown as HTMLElement)).toBe(false);
  });
});
