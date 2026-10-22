import { defineConfig } from "@playwright/test";

// End-to-end tests against the live stack: regtest network, toshi-server and `npm run dev`.
// See e2e/escrow.spec.ts for how to start them.
export default defineConfig({
  testDir: "e2e",
  timeout: 4 * 60_000,
  expect: { timeout: 20_000 },
  workers: 1,
  reporter: "list",
  use: {
    baseURL: process.env.WEB_URL ?? "http://localhost:3001",
    // The system Chrome, so no browser download is needed.
    channel: process.env.PW_CHANNEL ?? "chrome",
    headless: true,
    trace: "retain-on-failure",
  },
});
