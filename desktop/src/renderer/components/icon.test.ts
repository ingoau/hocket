// Every icon the action registry can emit (Material Symbols names in
// crates/hocket-core/src/actions/defs.rs) must map to a lucide glyph, and the
// renderer's own names must all resolve; nothing falls through to the
// placeholder silently.
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { ICON_NAMES, MATERIAL, hasIcon, iconName } from "./Icon";

const DEFS = resolve(__dirname, "../../../../crates/hocket-core/src/actions/defs.rs");

describe("icon map", () => {
  it("covers every Material Symbols icon name in the action registry", () => {
    const src = readFileSync(DEFS, "utf8");
    const names = new Set<string>();
    for (const m of src.matchAll(/action!\(\s*"[^"]*",\s*"[^"]*",\s*"([^"]*)"/g)) names.add(m[1]!);
    expect(names.size).toBeGreaterThan(40);
    const missing = [...names].filter((n) => !hasIcon(n) || iconName(n) === "music" && MATERIAL[n] !== "song");
    expect(missing).toEqual([]);
  });

  it("maps every Material name onto a renderer glyph and keeps the renderer's own names", () => {
    for (const [material, local] of Object.entries(MATERIAL)) {
      expect(ICON_NAMES, `${material} → ${local}`).toContain(local);
      expect(iconName(material)).toBe(local);
    }
    for (const n of ICON_NAMES) expect(iconName(n)).toBe(n);
    expect(hasIcon("no_such_icon")).toBe(false);
    expect(iconName("no_such_icon")).toBe("music");
  });
});
