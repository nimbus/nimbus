/**
 * UIR17 Onboarding e2e: a brand-new tenant sees, on every empty page, the
 * command that puts the first row there, addressed to this server and this
 * tenant, and the Overview shows the first-run panel until a function runs.
 */
import type { Page } from "@playwright/test";
import { expect, test } from "./fixtures/nimbus-server";

const TENANT_ID = "onboard-e2e";
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

async function createTenant(page: Page, baseURL: string): Promise<void> {
  const res = await page.request.post(`${baseURL}/api/tenants`, {
    data: { id: TENANT_ID },
    headers: {
      "Content-Type": "application/json",
      Accept: "application/json",
    },
  });
  expect(res.status(), await res.text()).toBe(201);
}

async function hideToasts(page: Page): Promise<void> {
  await page.addStyleTag({
    content: "[data-sonner-toaster] { display: none !important; }",
  });
}

test.describe("onboarding", () => {
  test.skip(
    ({ isMobile }) => isMobile,
    "The command blocks are desktop surfaces; the smoke walk covers the first-run panel on mobile.",
  );

  test("every empty page names the command that fills it, for this server and tenant", async ({
    page,
    nimbusServer,
  }) => {
    const { baseURL, readToken } = nimbusServer;
    await authenticate(page, baseURL, readToken());
    await createTenant(page, baseURL);
    await page.addInitScript(
      ([key, tenant]) => localStorage.setItem(key, tenant),
      ["nimbus-ui:active-tenant", TENANT_ID],
    );

    // Overview: no functions and no runs, so the first-run panel stands in
    // for the stats, with every step open.
    await page.goto(`${baseURL}/ui/developer/`);
    await hideToasts(page);
    const onboarding = page.getByTestId("overview-onboarding");
    await expect(onboarding).toBeVisible();
    await expect(
      onboarding.getByTestId("overview-onboarding-progress"),
    ).toHaveText("0 of 3 steps done");
    await expect(page.getByTestId("overview-stats")).toHaveCount(0);

    // Storage: the tenant has no tables; the next action is the insert.
    await page.goto(`${baseURL}/ui/developer/storage`);
    await hideToasts(page);
    const tablesEmpty = page.getByTestId("tenant-tables-empty");
    await expect(tablesEmpty).toContainText("No tables");
    await expect(tablesEmpty.getByTestId("tenant-tables-empty-snippet")).toContainText(
      `POST ${baseURL}/api/tenants/${TENANT_ID}/documents`,
    );
    await page.screenshot({ path: `${PROOF_DIR}/UIR17-storage-empty.png` });

    // Schedules: both sections, each with its own request.
    await page.goto(`${baseURL}/ui/developer/schedules`);
    await hideToasts(page);
    await expect(
      page.getByTestId("schedules-scheduled-empty-snippet"),
    ).toContainText(`POST ${baseURL}/api/tenants/${TENANT_ID}/schedule`);
    await page.screenshot({ path: `${PROOF_DIR}/UIR17-schedules-empty.png` });
    await page.goto(`${baseURL}/ui/developer/schedules?section=cron`);
    await hideToasts(page);
    await expect(page.getByTestId("schedules-cron-empty-snippet")).toContainText(
      `POST ${baseURL}/api/tenants/${TENANT_ID}/crons`,
    );

    // Observability: no runs, so the next action is a function call, not
    // a link to Compute.
    await page.goto(`${baseURL}/ui/developer/observability?tab=logs`);
    await hideToasts(page);
    await expect(
      page.getByTestId("observability-log-empty-snippet"),
    ).toContainText(`nimbus run ${baseURL} functions`);
    await expect(
      page.getByTestId("observability-log-empty-snippet"),
    ).toContainText(`--tenant ${TENANT_ID}`);
    await expect(page.getByTestId("observability-log-empty-cta")).toHaveCount(0);
    await page.screenshot({ path: `${PROOF_DIR}/UIR17-logs-empty.png` });

    // Files: no buckets; the next action is the first put.
    await page.goto(`${baseURL}/ui/developer/files`);
    await hideToasts(page);
    await expect(page.getByTestId("files-empty-buckets-snippet")).toContainText(
      `PUT ${baseURL}/api/tenants/${TENANT_ID}/objects/assets/hello.txt`,
    );
  });
});
