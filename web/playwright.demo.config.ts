import { defineConfig } from "@playwright/test";

// The scripted demo rehearsal. Needs the demo environment: ./scripts/demo-env.sh up
// Run it with: npm run rehearse   (PW_CHANNEL=chromium if system Chrome is not installed)
export default defineConfig({
  testDir: "e2e-demo",
  timeout: 6 * 60_000,
  expect: { timeout: 20_000 },
  workers: 1,
  reporter: "list",
  use: {
    baseURL: process.env.WEB_URL ?? "http://localhost:3001",
    channel: process.env.PW_CHANNEL ?? "chrome",
    headless: true,
    trace: "retain-on-failure",
  },
});
