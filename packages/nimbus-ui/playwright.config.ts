import { defineConfig, devices } from "@playwright/test";

const PORT = Number(process.env.NIMBUS_E2E_PORT ?? 8788);
const BASE_URL = process.env.NIMBUS_E2E_BASE_URL ?? `http://127.0.0.1:${PORT}`;
const NIMBUS_BIN =
  process.env.NIMBUS_E2E_BIN ?? "../../target/debug/nimbus";

export default defineConfig({
  testDir: "./tests/e2e",
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: 1,
  reporter: process.env.CI ? "github" : "list",
  use: {
    baseURL: BASE_URL,
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
    ignoreHTTPSErrors: true,
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
  webServer: process.env.NIMBUS_E2E_NO_SERVER
    ? undefined
    : {
        command: `${NIMBUS_BIN} start --host 127.0.0.1 --port ${PORT}`,
        url: `${BASE_URL}/ui/auth`,
        reuseExistingServer: !process.env.CI,
        timeout: 60_000,
        env: {
          NIMBUS_E2E: "1",
        },
      },
});
