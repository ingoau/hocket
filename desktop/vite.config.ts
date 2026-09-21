import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("./src/renderer", import.meta.url));

export default defineConfig({
  root,
  base: "./",
  plugins: [react()],
  resolve: {
    alias: {
      "@core": fileURLToPath(new URL("./src/core", import.meta.url)),
      "@renderer": root,
      "@shared": fileURLToPath(new URL("./src/shared", import.meta.url)),
    },
  },
  build: {
    outDir: fileURLToPath(new URL("./out/renderer", import.meta.url)),
    emptyOutDir: true,
    target: "chrome140",
    sourcemap: true,
  },
  server: { port: 5178, strictPort: true },
  test: {
    root: fileURLToPath(new URL(".", import.meta.url)),
    include: ["src/**/*.test.ts", "src/**/*.test.tsx"],
    environment: "node",
  },
});
