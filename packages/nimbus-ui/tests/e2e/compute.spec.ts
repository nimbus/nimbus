// Compute in a real server: the function tree in the sub-panel, the
// Functions tab as a DataTable, a function page with its Overview and Source
// tabs, and the runner drawer prefilled from the function's validator. The
// server never records `argsSchema` on deploy yet, so the walk seeds the
// function row directly into the `_nimbus.functions` system table the way
// `smoke.spec.ts` seeds a service.

import type { Page } from "@playwright/test";
import { expect, test } from "./fixtures/nimbus-server";

const TENANT_ID = "compute-e2e";
const BUNDLE_SHA = "b".repeat(64);
const FUNCTION_PATH = "agent:send";

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

async function insertSystemRow(
  page: Page,
  baseURL: string,
  table: string,
  fields: Record<string, unknown>,
): Promise<void> {
  const res = await page.request.post(`${baseURL}/convex/_nimbus/mutation`, {
    data: { mutation: { type: "insert", table, fields } },
    headers: {
      "Content-Type": "application/json",
      Accept: "application/json",
    },
  });
  expect(res.status(), await res.text()).toBe(200);
}

async function seedFunction(page: Page, baseURL: string): Promise<void> {
  const tenantRes = await page.request.post(`${baseURL}/api/tenants`, {
    data: { id: TENANT_ID },
    headers: {
      "Content-Type": "application/json",
      Accept: "application/json",
    },
  });
  expect(tenantRes.status(), await tenantRes.text()).toBe(201);

  await insertSystemRow(page, baseURL, "bundles", {
    sha256: BUNDLE_SHA,
    sizeBytes: 1024,
    status: "active",
  });
  await insertSystemRow(page, baseURL, "functions", {
    bundleId: BUNDLE_SHA,
    path: FUNCTION_PATH,
    kind: "mutation",
    argsSchema: {
      kind: "object",
      fields: {
        conversationId: { kind: "string" },
        text: { kind: "string" },
      },
    },
  });
}

test.describe("compute", () => {
  test.skip(
    ({ isMobile }) => isMobile,
    "The function tree lives in the desktop sub-panel; the sheet is covered by the smoke walk.",
  );

  test("lists a function, opens it, and prefills the runner from its validator", async ({
    page,
    nimbusServer,
  }) => {
    const { baseURL, readToken } = nimbusServer;
    await authenticate(page, baseURL, readToken());
    await seedFunction(page, baseURL);

    await page.goto(`${baseURL}/ui/developer/compute`);
    await expect(page.getByTestId("page-compute")).toBeVisible();
    await expect(page.getByTestId("compute-tabs")).toBeVisible();
    await expect(page.getByTestId("compute-tab-functions")).toBeVisible();

    // The Functions tab is a DataTable with the seeded row.
    const table = page.getByTestId("compute-functions");
    await expect(table).toBeVisible();
    await expect(table.getByRole("row").filter({ hasText: FUNCTION_PATH })).toHaveCount(1);

    // The sub-panel holds the function tree; the seeded function is a leaf.
    const leaf = page.getByTestId(`sub-panel-fn-${FUNCTION_PATH}`);
    await expect(leaf).toBeVisible();

    // A row click opens the function page on its Overview tab, which lists
    // the arguments read from the validator.
    await table.getByRole("row").filter({ hasText: FUNCTION_PATH }).click();
    await expect(page).toHaveURL(/\/developer\/compute\/agent%3Asend/);
    await expect(page.getByTestId("function-detail-tab-overview")).toBeVisible();
    await expect(page.getByTestId("function-overview-arg-conversationId")).toBeVisible();
    await expect(page.getByTestId("function-overview-arg-text")).toBeVisible();

    // The runner opens in form mode with one field per validator argument,
    // the active tenant shown read-only, and no tenant chooser.
    await page.getByTestId("function-runner-toggle").click();
    await expect(page.getByTestId("function-runner-mode-form")).toHaveAttribute("data-active", "true");
    await expect(page.getByTestId("function-runner-field-conversationId")).toBeVisible();
    await expect(page.getByTestId("function-runner-field-text")).toBeVisible();
    await expect(page.getByTestId("function-runner-tenant")).toHaveCount(0);
    await expect(page.getByTestId("function-runner-submit")).toContainText("Run function");

    // No source was captured for the seeded bundle: the Source tab is the
    // empty state that shows the capture command with a copy control.
    await page.getByTestId("function-detail-tab-source").click();
    await expect(page.getByTestId("function-source-missing")).toBeVisible();
    await expect(page.getByTestId("function-source-missing-snippet")).toContainText("nimbus dev --app-dir");
    await expect(
      page.getByTestId("function-source-missing-snippet").getByRole("button", { name: /copy command/i }),
    ).toBeVisible();

    // The tree leaf navigates too and lands on the Source tab.
    await leaf.click();
    await expect(page).toHaveURL(/tab=source/);
  });
});
