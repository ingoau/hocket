// Full build: typeshare types (if the addon isn't built we still need api.ts),
// main + preload bundles, then the renderer.
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";
import { postprocessApi } from "./postprocess-api.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const run = (cmd, args, opts = {}) => {
  const r = spawnSync(cmd, args, { stdio: "inherit", cwd: root, shell: process.platform === "win32", ...opts });
  if (r.status !== 0) process.exit(r.status ?? 1);
};

const api = resolve(root, "src/core/api.ts");
if (!existsSync(api)) run("node", ["scripts/gen.mjs", "--types-only"]);
// A bare `typeshare` run (without scripts/gen.mjs) leaves `export enum`s the
// renderer can't use; the rewrite is idempotent, so always apply it.
else postprocessApi(api);
run("node", ["scripts/build-main.mjs"]);
run("pnpm", ["exec", "vite", "build"]);
