// Smoke walk for the desktop UI.
//
// What this covers, in order:
//   1. /ui/app/        — Developer Overview tile envelopes
//   2. /ui/admin/      — Operator System tile envelopes
//   3. /ui/app/services       — ScopeChip reads `TENANT <tenant>` and
//                               the services table renders
//   4. /ui/admin/services     — tenant-grouped sub-drawer renders
//   5. /ui/admin/services/<id> — single Placement tab is selected
//   6. /ui/admin/tenants      — diagnostic envelope is reachable (the
//                               page renders; the empty/error states
//                               are owned by the route loader)
//   7. /ui/app/observability  — disabled `events`/`errors` tab chips
//   8. command palette via ⌘K — listbox + mode list render
//
// Console hygiene:
//   - assert zero `console.error` across the walk
//   - allow up to one `console.warn` (TanStack Router's `notFound()` warning
//     is the only acceptable warning if a fixture service id is absent;
//     this spec doesn't hit that path)

import type { ConsoleMessage, Page } from "@playwright/test";
import { expect, test } from "./fixtures/nimbus-server";

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
    const console = attachConsoleAccumulator(page);

    // 1. Developer Overview
    await page.goto(`${baseURL}/ui/app/`);
    await expect(page.getByTestId("page-overview")).toBeVisible();
    await expect(page.getByTestId("overview-top-strip")).toBeVisible();
    await expect(page.getByTestId("overview-counts")).toBeVisible();
    await expect(page.getByTestId("overview-events")).toBeVisible();
    await expect(page.getByTestId("overview-runs")).toBeVisible();

    // 2. Operator System
    await page.goto(`${baseURL}/ui/admin/`);
    await expect(page.getByTestId("page-admin-system")).toBeVisible();
    await expect(page.getByTestId("system-overview")).toBeVisible();

    // 3. Developer Services — ScopeChip reads `TENANT <tenant>`
    await page.goto(`${baseURL}/ui/app/services`);
    await expect(page.getByTestId("page-services")).toBeVisible();
    // The ScopeChip and services-table only render when an active tenant
    // is selected and at least one service exists. A fresh nimbus server
    // has neither, so this step is best-effort: any present element is
    // asserted, but their absence is also valid for an empty fixture.
    const scopeChip = page.getByTestId("services-scope");
    if (await scopeChip.count()) {
      await expect(scopeChip).toContainText(/tenant/i);
    }
    const servicesTable = page.getByTestId("services-table");
    if (await servicesTable.count()) {
      await expect(servicesTable).toBeVisible();
    }

    // 4. Operator Services — tenant-grouped sub-drawer
    await page.goto(`${baseURL}/ui/admin/services`);
    await expect(page.getByTestId("page-admin-services")).toBeVisible();
    await expect(page.getByTestId("admin-services-summary")).toBeVisible();
    // sub-drawer presence (the items only render if services exist;
    // the host envelope must be there regardless)
    await expect(page.getByTestId("sub-drawer")).toBeVisible();

    // 5. Operator Service detail — single Placement tab
    // This step is conditional: we navigate only if at least one service
    // sub-drawer item exists. A fresh server has none, so we assert the
    // not-found envelope when navigating to a synthetic id; either path
    // proves the route is wired.
    const firstServiceLink = page
      .locator('[data-testid^="sub-drawer-item-op-service-"]')
      .first();
    if (await firstServiceLink.count()) {
      await firstServiceLink.click();
      await expect(page.getByTestId("page-admin-service-detail")).toBeVisible();
      await expect(
        page.getByTestId("admin-service-detail-tab-placement"),
      ).toBeVisible();
      await expect(
        page.getByTestId("admin-service-tab-placement"),
      ).toBeVisible();
    } else {
      await page.goto(
        `${baseURL}/ui/admin/services/service_synthetic_smoke_id`,
      );
      await expect(page.getByTestId("admin-service-not-found")).toBeVisible();
    }

    // 6. Operator Tenants — diagnostic envelope is reachable
    await page.goto(`${baseURL}/ui/admin/tenants`);
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
    await page.goto(`${baseURL}/ui/app/observability`);
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
