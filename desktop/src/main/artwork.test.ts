import { mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { realpathSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { ART_TOKEN_RE } from "@shared/constants";
import { ArtworkRegistry } from "./artwork";

describe("ArtworkRegistry", () => {
  let root: string;
  let cache: string;
  beforeEach(() => {
    root = realpathSync(mkdtempSync(join(tmpdir(), "hocket-art-")));
    cache = join(root, "stream-cache");
    mkdirSync(join(cache, "images", "srv"), { recursive: true });
    writeFileSync(join(cache, "images", "srv", "a-300.png"), "png");
    writeFileSync(join(root, "credentials.json"), "[]");
  });
  afterEach(() => rmSync(root, { recursive: true, force: true }));

  it("hands out an opaque token for a file inside the cache and resolves it back", async () => {
    const reg = new ArtworkRegistry(cache);
    const file = join(cache, "images", "srv", "a-300.png");
    const token = await reg.register(file);
    expect(token).toMatch(ART_TOKEN_RE);
    expect(await reg.register(file)).toBe(token);
    expect(await reg.register(`file://${file}`)).toBe(token);
    expect(await reg.resolve(token!)).toBe(file);
  });

  it("refuses anything outside the cache: userData files, traversal, symlinks out, unknown or malformed tokens", async () => {
    const reg = new ArtworkRegistry(cache);
    expect(await reg.register(join(root, "credentials.json"))).toBeUndefined();
    expect(await reg.register(join(cache, "images", "..", "..", "credentials.json"))).toBeUndefined();
    symlinkSync(join(root, "credentials.json"), join(cache, "images", "srv", "evil-300.png"));
    expect(await reg.register(join(cache, "images", "srv", "evil-300.png"))).toBeUndefined();
    expect(await reg.register(join(cache, "images", "srv", "missing.png"))).toBeUndefined();
    expect(await reg.resolve("0".repeat(32))).toBeUndefined();
    expect(await reg.resolve(join(cache, "images", "srv", "a-300.png"))).toBeUndefined();
    expect(await reg.resolve("../credentials.json")).toBeUndefined();
  });

  it("re-checks the file when serving: a token stops resolving once its file is swapped for a symlink out", async () => {
    const reg = new ArtworkRegistry(cache);
    const file = join(cache, "images", "srv", "a-300.png");
    const token = (await reg.register(file))!;
    rmSync(file);
    symlinkSync(join(root, "credentials.json"), file);
    expect(await reg.resolve(token)).toBeUndefined();
  });

  it("bounds the map", async () => {
    const reg = new ArtworkRegistry(cache, 2);
    const files = ["x", "y", "z"].map((n) => join(cache, "images", "srv", `${n}-64.png`));
    for (const f of files) writeFileSync(f, "png");
    const tokens: string[] = [];
    for (const f of files) tokens.push((await reg.register(f))!);
    expect(await reg.resolve(tokens[0]!)).toBeUndefined();
    expect(await reg.resolve(tokens[2]!)).toBe(files[2]);
  });
});
