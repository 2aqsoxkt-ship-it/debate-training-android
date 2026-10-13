import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "tests/ui",
  testMatch: "**/*.spec.ts",
  workers: 2,
  retries: 0,
  reporter: "list",
  use: {
    channel: "msedge",
    baseURL: "http://127.0.0.1:1422",
    screenshot: "only-on-failure",
    trace: "retain-on-failure",
  },
  webServer: {
    command: "npm run dev -- --port 1422",
    url: "http://127.0.0.1:1422",
    reuseExistingServer: false,
    timeout: 30_000,
  },
});
