// Regenerates desktop/src/core/api.ts from crates/hocket-core/src/api.rs with
// typeshare, and (unless --types-only) builds the napi addon into ./native.
import { spawnSync } from "node:child_process";
import { mkdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repo = resolve(root, "..");
const typesOnly = process.argv.includes("--types-only");
const release = !process.argv.includes("--debug");

const run = (cmd, args, extraEnv = {}) => {
  const r = spawnSync(cmd, args, { stdio: "inherit", cwd: root, env: { ...process.env, ...extraEnv }, shell: process.platform === "win32" });
  if (r.status !== 0) {
    console.error(`[gen] ${cmd} ${args.join(" ")} failed (${r.status})`);
    process.exit(r.status ?? 1);
  }
};

mkdirSync(resolve(root, "src/core"), { recursive: true });
run("typeshare", [resolve(repo, "crates/hocket-core/src/api.rs"), "--lang=typescript", `--output-file=${resolve(root, "src/core/api.ts")}`]);
console.log("[gen] src/core/api.ts written");

if (!typesOnly) {
  mkdirSync(resolve(root, "native"), { recursive: true });
  const targetDir = process.env.CARGO_TARGET_DIR ?? resolve(repo, "target");
  run("pnpm", [
    "exec", "napi", "build",
    "--manifest-path", resolve(repo, "crates/hocket-node/Cargo.toml"),
    "--target-dir", targetDir,
    "--output-dir", resolve(root, "native"),
    "--platform",
    "--js", "index.js",
    "--dts", "index.d.ts",
    ...(release ? ["--release"] : []),
  ]);
  console.log("[gen] native addon built into desktop/native");
}
