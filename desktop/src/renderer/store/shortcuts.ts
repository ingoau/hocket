// Chord parsing and matching. Normalised form: "Mod+Shift+K", "Space",
// "ArrowLeft", "Delete". `Mod` is Cmd on macOS and Ctrl elsewhere; explicit
// "Ctrl"/"Cmd" are kept as written. Modifiers are ordered Mod, Ctrl, Alt,
// Shift, Meta. Keys are compared case-insensitively.
export interface Chord {
  mod: boolean;
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
  meta: boolean;
  key: string;
}

const MOD_NAMES: Record<string, keyof Omit<Chord, "key">> = {
  mod: "mod",
  cmdorctrl: "mod",
  commandorcontrol: "mod",
  ctrl: "ctrl",
  control: "ctrl",
  alt: "alt",
  option: "alt",
  shift: "shift",
  meta: "meta",
  cmd: "meta",
  command: "meta",
  super: "meta",
  win: "meta",
};

const KEY_ALIASES: Record<string, string> = {
  " ": "Space",
  spacebar: "Space",
  esc: "Escape",
  del: "Delete",
  return: "Enter",
  left: "ArrowLeft",
  right: "ArrowRight",
  up: "ArrowUp",
  down: "ArrowDown",
  plus: "+",
  comma: ",",
};

export function parseChord(text: string): Chord | undefined {
  const parts = text.split("+").map((p) => p.trim()).filter(Boolean);
  // "Mod++" (the plus key) edge: the split ate it.
  if (text.trim().endsWith("++")) parts.push("+");
  if (!parts.length) return undefined;
  const chord: Chord = { mod: false, ctrl: false, alt: false, shift: false, meta: false, key: "" };
  for (const p of parts) {
    const m = MOD_NAMES[p.toLowerCase()];
    if (m) {
      chord[m] = true;
      continue;
    }
    if (chord.key) return undefined; // two non-modifier keys
    chord.key = normaliseKey(p);
  }
  if (!chord.key) return undefined;
  return chord;
}

export function normaliseKey(k: string): string {
  const alias = KEY_ALIASES[k.toLowerCase()] ?? KEY_ALIASES[k];
  if (alias) return alias;
  if (k.length === 1) return k.toUpperCase();
  if (/^arrow/i.test(k)) return "Arrow" + k.slice(5, 6).toUpperCase() + k.slice(6).toLowerCase();
  if (/^f\d{1,2}$/i.test(k)) return k.toUpperCase();
  return k.charAt(0).toUpperCase() + k.slice(1);
}

export function formatChord(c: Chord, platform: "macOs" | "linux" | "windows" = "linux"): string {
  const parts: string[] = [];
  if (c.mod) parts.push(platform === "macOs" ? "Cmd" : "Ctrl");
  if (c.ctrl) parts.push("Ctrl");
  if (c.alt) parts.push(platform === "macOs" ? "Option" : "Alt");
  if (c.shift) parts.push("Shift");
  if (c.meta) parts.push(platform === "macOs" ? "Cmd" : "Meta");
  parts.push(c.key);
  return parts.join("+");
}

/** Canonical string form for storage/comparison (platform-independent). */
export function chordToString(c: Chord): string {
  const parts: string[] = [];
  if (c.mod) parts.push("Mod");
  if (c.ctrl) parts.push("Ctrl");
  if (c.alt) parts.push("Alt");
  if (c.shift) parts.push("Shift");
  if (c.meta) parts.push("Meta");
  parts.push(c.key);
  return parts.join("+");
}

export interface KeyLike {
  key: string;
  code?: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
}

/** Chord from a keyboard event. Returns undefined for bare modifier presses. */
export function chordFromEvent(e: KeyLike, platform: "macOs" | "linux" | "windows"): Chord | undefined {
  if (["Control", "Shift", "Alt", "Meta", "OS"].includes(e.key)) return undefined;
  const isMac = platform === "macOs";
  const modDown = isMac ? e.metaKey : e.ctrlKey;
  let key = e.key;
  // With Shift held, e.key is the shifted glyph; prefer the physical key for letters/digits.
  if (e.code && /^(Key[A-Z]|Digit[0-9])$/.test(e.code)) key = e.code.slice(-1);
  if (e.code === "Space") key = "Space";
  return {
    mod: modDown,
    ctrl: isMac ? e.ctrlKey : false,
    alt: e.altKey,
    shift: e.shiftKey,
    meta: isMac ? false : e.metaKey,
    key: normaliseKey(key),
  };
}

export function chordsEqual(a: Chord, b: Chord): boolean {
  return a.mod === b.mod && a.ctrl === b.ctrl && a.alt === b.alt && a.shift === b.shift && a.meta === b.meta && a.key.toLowerCase() === b.key.toLowerCase();
}

export interface Binding {
  actionId: string;
  chord: Chord;
}

export class Keymap {
  private byChord = new Map<string, string[]>();
  constructor(bindings: { actionId: string; shortcut: string | undefined }[]) {
    for (const b of bindings) {
      if (!b.shortcut) continue;
      const c = parseChord(b.shortcut);
      if (!c) continue;
      const k = chordToString(c).toLowerCase();
      this.byChord.set(k, [...(this.byChord.get(k) ?? []), b.actionId]);
    }
  }

  /** Action ids bound to the chord, first wins. */
  lookup(chord: Chord): string[] {
    return this.byChord.get(chordToString(chord).toLowerCase()) ?? [];
  }

  /** Every chord bound to more than one action. */
  conflicts(): { chord: string; actionIds: string[] }[] {
    return [...this.byChord.entries()].filter(([, ids]) => ids.length > 1).map(([chord, actionIds]) => ({ chord, actionIds }));
  }
}

/** Whether the event target is a text-entry element (where Ctrl+Z belongs to the field). */
export function isTextInput(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  if (!el || typeof el.tagName !== "string") return false;
  const tag = el.tagName.toLowerCase();
  if (tag === "textarea" || tag === "select") return true;
  if (tag === "input") {
    const type = ((el as HTMLInputElement).type || "text").toLowerCase();
    return !["checkbox", "radio", "range", "button", "submit", "color", "file"].includes(type);
  }
  return el.isContentEditable === true;
}
