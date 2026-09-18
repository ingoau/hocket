// Development: Vite dev server for the renderer, esbuild --watch for main and
// preload, then Electron pointed at the dev server.
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const env = { ...process.env, HOCKET_DEV: "1", NODE_ENV: "development" };
const children = [];
const start = (cmd, args) => {
  const c = spawn(cmd, args, { cwd: root, stdio: "inherit", env, shell: process.platform === "win32" });
  children.push(c);
  return c;
};

start("node", ["scripts/build-main.mjs", "--watch"]);
start("pnpm", ["exec", "vite"]);
await new Promise((r) => setTimeout(r, 2500));
const electron = start("pnpm", ["exec", "electron", "."]);
electron.on("exit", () => {
  for (const c of children) c.kill();
  process.exit(0);
});
process.on("SIGINT", () => {
  for (const c of children) c.kill();
  process.exit(0);
});
