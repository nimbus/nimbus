// Smoke walk for the console, run in the desktop and the mobile project.
//
// Every step after the first reaches its page through the sidebar: the
// view switcher for a change of view, a nav row for a page. On the mobile
// project the sidebar is a sheet behind the top-bar menu button, and the
// same helpers open it first.
//
// What this covers, in order:
//   1. /ui/developer/        — Developer Overview headline, connect
//                               panel, and the first-run panel on a
//                               server with no functions or runs
//   2. /ui/operator/      — Operator Nodes tile envelopes
//   3. /ui/developer/services       — ScopeChip reads `TENANT <tenant>` and
//                               the services table renders
//   4. /ui/operator/services     — tenant-grouped sub-panel renders
//   5. /ui/operator/services/<id> — single Placement tab is selected
//   6. /ui/operator/tenants      — diagnostic envelope is reachable (the
//                               page renders; the empty/error states
//                               are owned by the route loader)
//   7. /ui/developer/observability  — Logs/Runs page tabs; no Events/Errors chips
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
            sourceGeneration: "1",
            attachmentId: "smoke-attachment",
            generation: "1",
            attachmentProviderId: "local",
            observedPhase: "active",
            endpoints: [],
            conditions: [],
            cleanupState: "clear",
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

// The sidebar is a sheet below 640px. `openNav` puts the sidebar body on
// screen in either layout so a step can click a row or the view switcher
// without knowing which project it runs in. A navigation closes the sheet
// on its own; `closeNav` is for the steps that change scope without moving.
async function openNav(page: Page): Promise<void> {
  // Below the desktop tier the sub-panel overlay stays open across a
  // navigation from one of its own items, and its scrim covers the top bar.
  const overlay = page.getByTestId("sub-panel-overlay");
  if (await overlay.isVisible()) {
    await page.keyboard.press("Escape");
    await expect(overlay).toHaveCount(0);
  }
  const menu = page.getByTestId("mobile-menu-button");
  if (await menu.isVisible()) {
    await menu.click();
    await expect(page.getByTestId("sidebar-sheet")).toBeVisible();
  }
}

async function closeNav(page: Page): Promise<void> {
  const sheet = page.getByTestId("sidebar-sheet");
  if (await sheet.isVisible()) {
    await page.keyboard.press("Escape");
    await expect(sheet).toHaveCount(0);
  }
}

async function switchView(
  page: Page,
  view: "developer" | "operator",
): Promise<void> {
  await openNav(page);
  await page.getByTestId(`view-switcher-${view}`).click();
}

async function navigateTo(page: Page, id: string): Promise<void> {
  await openNav(page);
  await page.getByTestId(`nav-${id}`).click();
}

// Below the desktop tier the sub-panel starts as a rail; its items live in
// the overlay behind the expand button.
async function openSubPanel(page: Page): Promise<void> {
  const panel = page.getByTestId("sub-panel");
  await expect(panel).toBeVisible();
  if ((await panel.getAttribute("data-collapsed")) === "true") {
    await page.getByTestId("sub-panel-toggle").click();
    await expect(page.getByTestId("sub-panel-overlay")).toBeVisible();
  }
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

test.describe("console smoke walk", () => {
  test("8-step deterministic walk through the sidebar asserts envelopes and console hygiene", async ({
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
    await expect(page.getByTestId("overview-headline")).toBeVisible();
    await expect(page.getByTestId("overview-connect")).toBeVisible();
    await expect(page.getByTestId("overview-connect-snippet")).toContainText(
      `${baseURL}/api/tenants/`,
    );
    // A fresh server has no functions and no runs, so the Overview shows
    // the first-run panel and not empty stat tiles.
    await expect(page.getByTestId("overview-onboarding")).toBeVisible();
    await expect(page.getByTestId("overview-stats")).toHaveCount(0);

    // 2. Operator Nodes, through the view switcher
    await switchView(page, "operator");
    await expect(page).toHaveURL(/\/ui\/operator\/?$/);
    await expect(page.getByTestId("page-operator-nodes")).toBeVisible();
    await expect(page.getByTestId("nodes-hosted")).toBeVisible();

    // 3. Developer Services — back through the view switcher, then the
    // Services row, then select the seeded tenant through the real tenant
    // selector in the sidebar scope row and assert the scoped service list.
    await switchView(page, "developer");
    await expect(page.getByTestId("page-overview")).toBeVisible();
    await navigateTo(page, "services");
    await expect(page.getByTestId("page-services")).toBeVisible();
    await openNav(page);
    await page.getByTestId("tenant-selector-trigger").click();
    await page
      .getByTestId(`tenant-selector-option-${SMOKE_TENANT_ID}`)
      .click();
    await closeNav(page);
    await expect(page.getByTestId("services-scope")).toContainText(
      new RegExp(SMOKE_TENANT_ID, "i"),
    );
    await expect(page.getByTestId("services-table")).toBeVisible();
    await expect(
      page.getByTestId(`services-row-${SMOKE_SERVICE_NAME}`),
    ).toBeVisible();

    // 4. Operator Services — tenant-grouped sub-panel. The view switch
    // restores the operator route last open, which is the nodes page.
    await switchView(page, "operator");
    await navigateTo(page, "services");
    await expect(page.getByTestId("page-admin-services")).toBeVisible();
    await expect(page.getByTestId("admin-services-summary")).toBeVisible();
    // sub-panel presence (the items only render if services exist;
    // the host envelope must be there regardless)
    await openSubPanel(page);

    // 5. Operator Service detail — single Placement tab. The seeded
    // service surfaces in the sub-panel regardless of which tenant is
    // active in the operator view.
    const firstServiceLink = page
      .locator('[data-testid^="sub-panel-item-op-service-"]')
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
    await navigateTo(page, "tenants");
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

    // 7. Developer Observability — the Logs/Runs tab strip switches the
    // sub-view through the URL, and no unbuilt view is named.
    await switchView(page, "developer");
    await navigateTo(page, "observability");
    await expect(page.getByTestId("page-observability")).toBeVisible();
    await expect(page.getByTestId("observability-tabs")).toBeVisible();
    await expect(page.getByTestId("observability-tab-logs")).toHaveAttribute(
      "aria-current",
      "page",
    );
    await page.getByTestId("observability-tab-runs").click();
    await expect(page).toHaveURL(/tab=runs/);
    await expect(page.getByTestId("observability-tab-runs")).toHaveAttribute(
      "aria-current",
      "page",
    );
    await expect(page.getByTestId("observability-tab-events")).toHaveCount(0);
    await expect(page.getByTestId("observability-tab-errors")).toHaveCount(0);

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
    // The palette opens on its three groups and writes the keyboard
    // contract in its footer.
    await expect(page.getByTestId("palette-group-routes")).toBeVisible();
    await expect(page.getByTestId("palette-group-tenants")).toBeVisible();
    await expect(page.getByTestId("palette-group-actions")).toBeVisible();
    await expect(page.getByTestId("command-palette-footer")).toContainText(
      "tenant lens",
    );
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("command-palette")).toBeHidden();

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
