import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "./tests/e2e",
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: 1,
  reporter: process.env.CI ? "github" : "list",
  use: {
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
    ignoreHTTPSErrors: true,
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
    // The mobile project runs the specs whose behaviour differs below 640px:
    // the smoke walk drives every navigation through the sidebar, which is a
    // sheet at this width, and the sheet spec owns the sheet itself.
    {
      name: "mobile",
      use: { ...devices["Pixel 7"] },
      testMatch: ["**/smoke.spec.ts", "**/sidebar-sheet.spec.ts"],
    },
  ],
});
