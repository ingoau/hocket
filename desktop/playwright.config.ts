import { defineConfig } from "@playwright/test";

// Electron e2e against the FakeCore. Run with `pnpm test:e2e` (needs a display:
// `xvfb-run -a pnpm test:e2e` on a headless Linux box). Requires `pnpm build` first.
export default defineConfig({
  testDir: "./e2e",
  timeout: 60_000,
  expect: { timeout: 10_000 },
  fullyParallel: false,
  workers: 1,
  retries: process.env.CI ? 1 : 0,
  reporter: [["list"]],
  use: { trace: "retain-on-failure" },
});
