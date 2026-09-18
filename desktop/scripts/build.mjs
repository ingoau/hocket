// Full build: typeshare types (if the addon isn't built we still need api.ts),
// main + preload bundles, then the renderer.
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const run = (cmd, args, opts = {}) => {
  const r = spawnSync(cmd, args, { stdio: "inherit", cwd: root, shell: process.platform === "win32", ...opts });
  if (r.status !== 0) process.exit(r.status ?? 1);
};

if (!existsSync(resolve(root, "src/core/api.ts"))) run("node", ["scripts/gen.mjs", "--types-only"]);
run("node", ["scripts/build-main.mjs"]);
run("pnpm", ["exec", "vite", "build"]);
