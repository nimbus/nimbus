// Smoke walk for the desktop UI.
//
// What this covers, in order:
//   1. /ui/developer/        — Developer Overview tile envelopes
//   2. /ui/operator/      — Operator System tile envelopes
//   3. /ui/developer/services       — ScopeChip reads `TENANT <tenant>` and
//                               the services table renders
//   4. /ui/operator/services     — tenant-grouped sub-drawer renders
//   5. /ui/operator/services/<id> — single Placement tab is selected
//   6. /ui/operator/tenants      — diagnostic envelope is reachable (the
//                               page renders; the empty/error states
//                               are owned by the route loader)
//   7. /ui/developer/observability  — disabled `events`/`errors` tab chips
//   8. command palette via ⌘K — listbox + mode list render
//
// Fixture seeding:
//   Before the walk, this spec seeds one tenant (`SMOKE_TENANT_ID`) and
//   one service document (`SMOKE_SERVICE_NAME`) so steps 3, 4, and 5
//   exercise non-empty envelopes (ScopeChip + services-table + a real
//   placement-tab page) instead of branching on whether the fixture
//   happens to be empty.
//
// Console hygiene:
//   - assert zero `console.error` across the walk
//   - allow up to one `console.warn` (TanStack Router's `notFound()` warning
//     is the only acceptable warning if a fixture service id is absent;
//     this spec doesn't hit that path)

import type { ConsoleMessage, Page } from "@playwright/test";
import { expect, test } from "./fixtures/nimbus-server";

const SMOKE_TENANT_ID = "smoke";
const SMOKE_SERVICE_NAME = "smoke-svc";

async function authenticate(
  page: Page,
  baseURL: string,
  token: string,
): Promise<void> {
  // The auth session is cookie-based. Posting via `page.request` shares
  // cookies with the browser context so the subsequent navigations are
  // authenticated.
  const res = await page.request.post(`${baseURL}/ui/auth/session`, {
    data: { token },
    headers: {
      "Content-Type": "application/json",
      Accept: "application/json",
    },
  });
  expect(res.status()).toBe(200);
}

// Seeds one tenant and one service so the walk asserts non-empty envelopes
// unconditionally. The tenant is created via the public tenants API; the
// service is inserted directly into the `_nimbus.services` system table via
// the convex raw-mutation route (the system tenant has no exposed
// "register service" mutation — services are normally written by the
// engine when a sandbox starts).
async function seedSmokeFixture(
  page: Page,
  baseURL: string,
): Promise<void> {
  const tenantRes = await page.request.post(`${baseURL}/api/tenants`, {
    data: { id: SMOKE_TENANT_ID },
    headers: {
      "Content-Type": "application/json",
      Accept: "application/json",
    },
  });
  expect(tenantRes.status(), await tenantRes.text()).toBe(201);

  const serviceRes = await page.request.post(
    `${baseURL}/convex/_nimbus/mutation`,
    {
      data: {
        mutation: {
          type: "insert",
          table: "services",
          fields: {
            tenantId: SMOKE_TENANT_ID,
            name: SMOKE_SERVICE_NAME,
            kind: "sandbox",
            state: "running",
            endpoints: [],
          },
        },
      },
      headers: {
        "Content-Type": "application/json",
        Accept: "application/json",
      },
    },
  );
  expect(serviceRes.status(), await serviceRes.text()).toBe(200);
}

interface ConsoleAccumulator {
  errors: ConsoleMessage[];
  warnings: ConsoleMessage[];
}

function attachConsoleAccumulator(page: Page): ConsoleAccumulator {
  const acc: ConsoleAccumulator = { errors: [], warnings: [] };
  page.on("console", (msg) => {
    if (msg.type() === "error") acc.errors.push(msg);
    else if (msg.type() === "warning") acc.warnings.push(msg);
  });
  return acc;
}

test.describe("desktop UI smoke walk", () => {
  test("10-step deterministic walk asserts envelopes and console hygiene", async ({
    page,
    nimbusServer,
  }) => {
    const baseURL = nimbusServer.baseURL;
    await authenticate(page, baseURL, nimbusServer.readToken());
    await seedSmokeFixture(page, baseURL);
    const console = attachConsoleAccumulator(page);

    // 1. Developer Overview
    await page.goto(`${baseURL}/ui/developer/`);
    await expect(page.getByTestId("page-overview")).toBeVisible();
    await expect(page.getByTestId("overview-top-strip")).toBeVisible();
    await expect(page.getByTestId("overview-counts")).toBeVisible();
    await expect(page.getByTestId("overview-events")).toBeVisible();
    await expect(page.getByTestId("overview-runs")).toBeVisible();

    // 2. Operator System
    await page.goto(`${baseURL}/ui/operator/`);
    await expect(page.getByTestId("page-admin-system")).toBeVisible();
    await expect(page.getByTestId("system-overview")).toBeVisible();

    // 3. Developer Services — ScopeChip reads `TENANT <tenant>` and the
    // services table renders. The tenant bootstrap auto-selects the only
    // seeded tenant on the first developer-view navigation, so both
    // envelopes are deterministic.
    await page.goto(`${baseURL}/ui/developer/services`);
    await expect(page.getByTestId("page-services")).toBeVisible();
    await expect(page.getByTestId("services-scope")).toContainText(
      new RegExp(SMOKE_TENANT_ID, "i"),
    );
    await expect(page.getByTestId("services-table")).toBeVisible();
    await expect(
      page.getByTestId(`services-row-${SMOKE_SERVICE_NAME}`),
    ).toBeVisible();

    // 4. Operator Services — tenant-grouped sub-drawer
    await page.goto(`${baseURL}/ui/operator/services`);
    await expect(page.getByTestId("page-admin-services")).toBeVisible();
    await expect(page.getByTestId("admin-services-summary")).toBeVisible();
    // sub-drawer presence (the items only render if services exist;
    // the host envelope must be there regardless)
    await expect(page.getByTestId("sub-drawer")).toBeVisible();

    // 5. Operator Service detail — single Placement tab. The seeded
    // service surfaces in the sub-drawer regardless of which tenant is
    // active in the operator view.
    const firstServiceLink = page
      .locator('[data-testid^="sub-drawer-item-op-service-"]')
      .first();
    await expect(firstServiceLink).toBeVisible();
    await firstServiceLink.click();
    await expect(page.getByTestId("page-admin-service-detail")).toBeVisible();
    await expect(
      page.getByTestId("admin-service-detail-tab-placement"),
    ).toBeVisible();
    await expect(
      page.getByTestId("admin-service-tab-placement"),
    ).toBeVisible();

    // 6. Operator Tenants — diagnostic envelope is reachable
    await page.goto(`${baseURL}/ui/operator/tenants`);
    await expect(page.getByTestId("page-storage")).toBeVisible();
    // Either the table or the empty/server-error envelope renders; the
    // route is wired if any of these are visible.
    await expect(
      page.getByTestId("storage-tenants-table").or(
        page.getByTestId("storage-empty").or(
          page.getByTestId("storage-server-error-envelope"),
        ),
      ),
    ).toBeVisible();

    // 7. Developer Observability — disabled events/errors tab chips
    await page.goto(`${baseURL}/ui/developer/observability`);
    await expect(page.getByTestId("page-observability")).toBeVisible();
    await expect(page.getByTestId("observability-tabs")).toBeVisible();
    await expect(
      page.getByTestId("observability-tab-events-coming-soon"),
    ).toBeVisible();
    await expect(
      page.getByTestId("observability-tab-errors-coming-soon"),
    ).toBeVisible();

    // 8. Command palette via ⌘K
    await page.keyboard.press("Meta+k");
    const palette = page.getByTestId("command-palette");
    if (!(await palette.isVisible().catch(() => false))) {
      // On Linux CI runners Meta isn't always remapped to the host's
      // "command" key; Control+K is the documented fallback.
      await page.keyboard.press("Control+k");
    }
    await expect(page.getByTestId("command-palette")).toBeVisible();
    await expect(page.getByTestId("command-palette-input")).toBeVisible();
    await expect(page.getByTestId("command-palette-list")).toBeVisible();
    // Mode chips render in the footer.
    await expect(page.locator('[data-testid^="palette-mode-"]')).not.toHaveCount(
      0,
    );
    await page.keyboard.press("Escape");

    // 9. Console hygiene gate.
    //
    // We allow at most one warning (a known runtime warning has not been
    // observed in this walk; this leaves headroom for an environment
    // hiccup without making the lane flaky). We allow zero errors.
    const errorText = console.errors.map((m) => m.text()).join("\n  ");
    const warnText = console.warnings.map((m) => m.text()).join("\n  ");
    expect(
      console.errors,
      `console.error during walk:\n  ${errorText}`,
    ).toHaveLength(0);
    expect(
      console.warnings.length,
      `console.warn during walk:\n  ${warnText}`,
    ).toBeLessThanOrEqual(1);
  });
});
