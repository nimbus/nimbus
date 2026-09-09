/**
 * UIR13 Settings e2e: the System sub-page reports the data directory the
 * engine opened, the Shutdown sub-page routes both writes through
 * ConfirmDialog with a gate the operator has to satisfy, and the tenant
 * settings page says once that nothing tenant-scoped is built.
 *
 * The rotation dialog is exercised with a bogus bearer and the shutdown
 * dialog is cancelled: the API specs (`rotate-token`, `shutdown`) prove the
 * writes themselves, and this walk must not sign the fixture out or stop
 * the server it is testing.
 */
import type { Page } from "@playwright/test";
import { expect, test } from "./fixtures/nimbus-server";

const TENANT_ID = "settings-e2e";
const PROOF_DIR = "../../docs/private/plans/proof/nimbus-ui-rebuild";

async function authenticate(
  page: Page,
  baseURL: string,
  token: string,
): Promise<void> {
  const res = await page.request.post(`${baseURL}/ui/auth/session`, {
    data: { token },
    headers: {
      "Content-Type": "application/json",
      Accept: "application/json",
    },
  });
  expect(res.status()).toBe(200);
}

async function hideToasts(page: Page): Promise<void> {
  await page.addStyleTag({
    content: "[data-sonner-toaster] { display: none !important; }",
  });
}

test.describe("settings", () => {
  test.skip(
    ({ isMobile }) => isMobile,
    "The settings sub-panel is a desktop surface; the smoke walk covers the sheet on mobile.",
  );

  test("reports the data directory on System and gates both danger-zone writes", async ({
    page,
    nimbusServer,
  }) => {
    const { baseURL, readToken } = nimbusServer;
    await authenticate(page, baseURL, readToken());
    await page.setViewportSize({ width: 1280, height: 720 });

    // System: the row is the directory the engine opened, not a config
    // value, so it is non-empty on any started server.
    await page.goto(`${baseURL}/ui/operator/settings?section=system`);
    await hideToasts(page);
    await expect(page.getByTestId("page-settings")).toHaveAttribute(
      "data-section",
      "system",
    );
    await expect(page.getByTestId("settings-server-info")).toBeVisible();
    const dataDir = page.getByTestId("settings-server-data-dir");
    await expect(dataDir).toBeVisible();
    expect((await dataDir.textContent())?.trim().length).toBeGreaterThan(0);
    await expect(
      page.getByTestId("settings-server-data-dir-missing"),
    ).toHaveCount(0);
    await page.screenshot({ path: `${PROOF_DIR}/UIR13-system.png` });

    // The menu lists only built sub-pages.
    const menu = page.getByTestId("sub-panel");
    await expect(menu).toBeVisible();
    for (const label of ["General", "System", "Integrations", "Shutdown"]) {
      await expect(menu.getByRole("link", { name: label })).toBeVisible();
    }
    // Deploys moved to its own Developer page (UIR23).
    for (const label of ["Deploys", "Endpoints", "Token", "Environment"]) {
      await expect(menu.getByText(label, { exact: true })).toHaveCount(0);
    }

    // General: appearance, license and usage, configuration; no server
    // identity, which moved to System.
    await menu.getByRole("link", { name: "General" }).click();
    await expect(page).toHaveURL(/section=general/);
    await expect(page.getByTestId("settings-tenant-header")).toBeVisible();
    await expect(page.getByTestId("settings-server-info")).toHaveCount(0);
    await page.screenshot({ path: `${PROOF_DIR}/UIR13-general.png` });

    // Shutdown: rotation stays inert until a bearer is typed, and a bogus
    // bearer is refused by the server inside the dialog.
    await page.goto(`${baseURL}/ui/operator/settings?section=shutdown`);
    await hideToasts(page);
    await expect(page.getByTestId("settings-danger-zone")).toBeVisible();
    await page.getByTestId("settings-rotate-open").click();
    const rotateConfirm = page.getByTestId("settings-rotate-dialog-confirm");
    await expect(rotateConfirm).toHaveAttribute("aria-disabled", "true");
    await page.getByTestId("settings-rotate-token").fill("not-a-token");
    await expect(rotateConfirm).toHaveAttribute("aria-disabled", "false");
    await rotateConfirm.click();
    const rotateError = page
      .getByTestId("settings-rotate-dialog")
      .locator('[data-slot="dialog-error"]');
    await expect(rotateError).toBeVisible();
    await expect(page.getByTestId("settings-rotate-result")).toHaveCount(0);
    await page.getByTestId("settings-rotate-dialog-cancel").click();
    await expect(page.getByTestId("settings-rotate-dialog")).toHaveCount(0);

    // Shutdown asks for the typed phrase; the walk stops before Confirm.
    await page.getByTestId("settings-shutdown-open").click();
    const shutdownConfirm = page.getByTestId("settings-shutdown-dialog-confirm");
    await expect(shutdownConfirm).toHaveAttribute("aria-disabled", "true");
    await page.getByTestId("settings-shutdown-dialog-typed").fill("shut");
    await expect(shutdownConfirm).toHaveAttribute("aria-disabled", "true");
    await page.getByTestId("settings-shutdown-dialog-typed").fill("shutdown");
    await expect(shutdownConfirm).toHaveAttribute("aria-disabled", "false");
    await page.screenshot({
      path: `${PROOF_DIR}/UIR13-shutdown-dialog.png`,
      animations: "disabled",
    });
    await page.getByTestId("settings-shutdown-dialog-cancel").click();
    await expect(page.getByTestId("settings-shutdown-dialog")).toHaveCount(0);
    await expect(page.getByTestId("settings-shutdown-accepted")).toHaveCount(0);
  });

  test("shows one empty state and no sub-panel on the tenant settings page", async ({
    page,
    nimbusServer,
  }) => {
    const { baseURL, readToken } = nimbusServer;
    await authenticate(page, baseURL, readToken());
    await page.setViewportSize({ width: 1280, height: 720 });
    const tenantRes = await page.request.post(`${baseURL}/api/tenants`, {
      data: { id: TENANT_ID },
      headers: {
        "Content-Type": "application/json",
        Accept: "application/json",
      },
    });
    expect(tenantRes.status(), await tenantRes.text()).toBe(201);
    await page.addInitScript(
      ([key, tenant]) => localStorage.setItem(key, tenant),
      ["nimbus-ui:active-tenant", TENANT_ID],
    );

    await page.goto(`${baseURL}/ui/developer/settings`);
    await hideToasts(page);
    await expect(page.getByTestId("page-settings")).toBeVisible();
    await expect(page.getByTestId("settings-empty")).toBeVisible();
    await expect(page.getByTestId("settings-empty-body")).toContainText(
      "Adapter binding",
    );
    await expect(page.getByTestId("sub-panel")).toHaveCount(0);
    await page.screenshot({ path: `${PROOF_DIR}/UIR13-tenant.png` });

    await page.getByTestId("settings-empty-cta").click();
    await expect(page).toHaveURL(/\/ui\/operator\/settings\?section=general/);
  });
});
