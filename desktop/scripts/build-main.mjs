// Bundles the main process and preload with esbuild (CommonJS, Electron's
// Node). The renderer is built by Vite separately.
import { build } from "esbuild";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const watch = process.argv.includes("--watch");

const common = {
  bundle: true,
  platform: "node",
  format: "cjs",
  target: "node22",
  sourcemap: true,
  logLevel: "info",
  external: ["electron"],
  alias: {
    "@core": resolve(root, "src/core"),
    "@shared": resolve(root, "src/shared"),
  },
  define: { "process.env.NODE_ENV": JSON.stringify(process.env.NODE_ENV ?? "production") },
};

const targets = [
  { entryPoints: [resolve(root, "src/main/index.ts")], outfile: resolve(root, "out/main/index.cjs") },
  { entryPoints: [resolve(root, "src/preload/index.ts")], outfile: resolve(root, "out/preload/index.cjs") },
];

if (watch) {
  const { context } = await import("esbuild");
  for (const t of targets) {
    const ctx = await context({ ...common, ...t });
    await ctx.watch();
  }
  console.log("[build-main] watching…");
} else {
  for (const t of targets) await build({ ...common, ...t });
}
