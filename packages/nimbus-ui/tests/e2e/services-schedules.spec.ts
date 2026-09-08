/**
 * UIR15 Services and Schedules e2e: the services table names every
 * lifecycle state with a pill and offers the lifecycle actions from a row
 * menu, the service detail carries the live Logs tab, and the schedules
 * page opens a job sheet whose Run now re-enqueues the job.
 */
import type { Page } from "@playwright/test";
import { expect, test } from "./fixtures/nimbus-server";

const TENANT_ID = "svc-e2e";
const PROOF_DIR = "../../docs/private/plans/proof/nimbus-ui-rebuild";
const JSON_HEADERS = {
  "Content-Type": "application/json",
  Accept: "application/json",
};

// One service per lifecycle pill the table has to be able to name.
const SERVICES = [
  { name: "api", kind: "sandbox", state: "ready" },
  { name: "worker", kind: "sandbox", state: "starting" },
  { name: "cache", kind: "container", state: "stopped" },
  { name: "mailer", kind: "sandbox", state: "failed" },
] as const;

async function authenticate(
  page: Page,
  baseURL: string,
  token: string,
): Promise<void> {
  const res = await page.request.post(`${baseURL}/ui/auth/session`, {
    data: { token },
    headers: JSON_HEADERS,
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
    headers: JSON_HEADERS,
  });
  expect(res.status(), await res.text()).toBe(200);
}

async function hideToasts(page: Page): Promise<void> {
  await page.addStyleTag({
    content: "[data-sonner-toaster] { display: none !important; }",
  });
}

async function createTenant(page: Page, baseURL: string): Promise<void> {
  const res = await page.request.post(`${baseURL}/api/tenants`, {
    data: { id: TENANT_ID },
    headers: JSON_HEADERS,
  });
  expect(res.status(), await res.text()).toBe(201);
}

async function seedServices(page: Page, baseURL: string): Promise<void> {
  for (const service of SERVICES) {
    await insertSystemRow(page, baseURL, "services", {
      tenantId: TENANT_ID,
      name: service.name,
      kind: service.kind,
      state: service.state,
      sourceGeneration: "1",
      attachmentId: `${service.name}-attachment`,
      generation: "1",
      attachmentProviderId: "local",
      observedPhase: "active",
      endpoints: [],
      conditions: [],
      cleanupState: "clear",
    });
  }
}

// Two jobs far enough out that the scheduler leaves them pending, and one
// cron. The Run now assertion counts rows, so the seed count matters.
async function seedSchedules(page: Page, baseURL: string): Promise<void> {
  for (const [index, table] of ["pings", "audits"].entries()) {
    const res = await page.request.post(
      `${baseURL}/api/tenants/${TENANT_ID}/schedule`,
      {
        data: {
          run_after_ms: 6 * 60 * 60 * 1000 + index * 60_000,
          mutation: { type: "insert", table, fields: { seeded: index } },
        },
        headers: JSON_HEADERS,
      },
    );
    expect(res.status(), await res.text()).toBe(201);
  }
  const cron = await page.request.post(
    `${baseURL}/api/tenants/${TENANT_ID}/crons`,
    {
      data: {
        name: "sweep",
        schedule: { type: "interval", seconds: 3600 },
        mutation: { type: "insert", table: "sweeps", fields: { by: "cron" } },
      },
      headers: JSON_HEADERS,
    },
  );
  expect(cron.status(), await cron.text()).toBe(201);
}

// The row testid carries the system document id
// `scheduled-job:{tenant}:{jobId}`; a job id is a ULID, so the last segment
// is the id the routes take.
async function rowIds(rows: ReturnType<Page["locator"]>): Promise<string[]> {
  const testids = await rows.evaluateAll((nodes) =>
    nodes.map((node) => node.getAttribute("data-testid") ?? ""),
  );
  return testids.map((id) => id.replace("schedules-scheduled-", ""));
}

function jobIdOf(documentId: string): string {
  return documentId.slice(documentId.lastIndexOf(":") + 1);
}

test.describe("services and schedules", () => {
  test.skip(
    ({ isMobile }) => isMobile,
    "The row menu, the detail tabs, and the schedule sheet are desktop surfaces; the smoke walk covers the services table on mobile.",
  );

  test("names every lifecycle state, offers row actions, and shows the live logs", async ({
    page,
    nimbusServer,
  }) => {
    const { baseURL, readToken } = nimbusServer;
    await authenticate(page, baseURL, readToken());
    await createTenant(page, baseURL);
    await seedServices(page, baseURL);
    await page.addInitScript(
      ([key, tenant]) => localStorage.setItem(key, tenant),
      ["nimbus-ui:active-tenant", TENANT_ID],
    );

    await page.goto(`${baseURL}/ui/developer/services`);
    await hideToasts(page);
    await expect(page.getByTestId("page-services")).toBeVisible();
    await expect(page.getByTestId("services-table")).toBeVisible();
    for (const service of SERVICES) {
      const pill = page.getByTestId(`services-state-${service.name}`);
      await expect(pill).toHaveAttribute("data-state", service.state);
      // A state the pill cannot name falls back to the question glyph.
      await expect(pill).not.toHaveAttribute("data-glyph", "question");
    }

    // The row menu offers the actions the state allows: a ready service
    // can stop or restart, and cannot start again.
    await page.getByTestId("services-row-actions-api").click();
    const menu = page.getByTestId("services-row-menu");
    await expect(menu).toBeVisible();
    await expect(menu.getByTestId("services-row-menu-stop")).toBeVisible();
    await expect(menu.getByTestId("services-row-menu-restart")).toBeVisible();
    await expect(menu.getByTestId("services-row-menu-start")).toHaveCount(0);
    await expect(menu.getByTestId("services-row-menu-logs")).toBeVisible();
    await page.screenshot({ path: `${PROOF_DIR}/UIR15-services.png` });
    await page.keyboard.press("Escape");
    await expect(menu).toHaveCount(0);

    // A stopped service offers start; a service in flight offers nothing.
    await page.getByTestId("services-row-actions-cache").click();
    await expect(
      page.getByTestId("services-row-menu-start"),
    ).toBeVisible();
    await expect(page.getByTestId("services-row-menu-stop")).toHaveCount(0);
    await page.keyboard.press("Escape");
    await page.getByTestId("services-row-actions-worker").click();
    await expect(page.getByTestId("services-row-menu-open")).toBeVisible();
    await expect(page.getByTestId("services-row-menu-start")).toHaveCount(0);
    await expect(page.getByTestId("services-row-menu-stop")).toHaveCount(0);
    await page.keyboard.press("Escape");

    // The seeded row has no service definition behind it, so the lifecycle
    // route refuses; the row keeps its real state and says why.
    await page.getByTestId("services-row-actions-mailer").click();
    await page.getByTestId("services-row-menu-start").click();
    await expect(page.getByTestId("services-row-error-mailer")).toBeVisible();
    await expect(page.getByTestId("services-state-mailer")).toHaveAttribute(
      "data-state",
      "failed",
    );

    // Detail: the tabs are Overview, Logs, Config; Logs reads the service
    // event stream and links to observability.
    await page.getByTestId("services-link-api").click();
    await expect(page.getByTestId("page-service-detail")).toBeVisible();
    await expect(page.getByTestId("service-detail-state")).toHaveAttribute(
      "data-state",
      "ready",
    );
    await expect(page.getByTestId("service-detail-action-stop")).toBeVisible();
    await expect(page.getByTestId("service-tab-overview")).toBeVisible();
    await page.getByTestId("service-detail-tab-logs").click();
    await expect(page).toHaveURL(/tab=logs/);
    await expect(page.getByTestId("service-tab-logs")).toBeVisible();
    await expect(page.getByTestId("service-tab-logs-empty")).toBeVisible();
    await expect(page.getByTestId("service-tab-logs-open")).toHaveAttribute(
      "href",
      /developer\/observability/,
    );
    await page.screenshot({ path: `${PROOF_DIR}/UIR15-service-detail.png` });
    await page.getByTestId("service-detail-tab-config").click();
    await expect(page.getByTestId("service-tab-config")).toBeVisible();
  });

  test("opens a job sheet and re-enqueues the job with Run now", async ({
    page,
    nimbusServer,
  }) => {
    const { baseURL, readToken } = nimbusServer;
    await authenticate(page, baseURL, readToken());
    await createTenant(page, baseURL);
    await seedSchedules(page, baseURL);
    await page.addInitScript(
      ([key, tenant]) => localStorage.setItem(key, tenant),
      ["nimbus-ui:active-tenant", TENANT_ID],
    );

    await page.goto(`${baseURL}/ui/developer/schedules`);
    await hideToasts(page);
    await expect(page.getByTestId("page-schedules")).toBeVisible();
    const rows = page.locator(
      '[role="row"][data-testid^="schedules-scheduled-"]',
    );
    await expect(rows).toHaveCount(2);
    await page.screenshot({ path: `${PROOF_DIR}/UIR15-schedules.png` });

    // Row activation opens the sheet on that job.
    await rows.first().click();
    await expect(page).toHaveURL(/job=/);
    const sheet = page.getByTestId("schedules-sheet");
    await expect(sheet).toBeVisible();
    await expect(sheet.getByTestId("schedules-sheet-status")).toHaveAttribute(
      "data-state",
      "pending",
    );
    await expect(sheet.getByTestId("schedules-sheet-args")).toContainText(
      '"type": "insert"',
    );
    await page.screenshot({
      path: `${PROOF_DIR}/UIR15-schedule-sheet.png`,
      animations: "disabled",
    });

    // Run now enqueues the same mutation as a new job: the table gains a
    // row. The scheduler runs it at once, and writes the outcome into the
    // system record when the history route is read, so the test reads it
    // and then expects the row to settle on completed.
    const seededIds = await rowIds(rows);
    await sheet.getByTestId("schedules-sheet-run").click();
    await expect(rows).toHaveCount(3);
    const newId = (await rowIds(rows)).find((id) => !seededIds.includes(id));
    expect(newId, "no new scheduled row").toBeTruthy();
    const history = await page.request.get(
      `${baseURL}/api/tenants/${TENANT_ID}/schedule/history/${jobIdOf(newId as string)}`,
      { headers: JSON_HEADERS },
    );
    expect(history.status(), await history.text()).toBe(200);
    await expect(
      page.getByTestId(`schedules-scheduled-${newId}`).locator("[data-state]"),
    ).toHaveAttribute("data-state", "completed");
    await expect(rows.locator('[data-state="pending"]')).toHaveCount(2);

    // Cron: the sheet closes, the sub-panel switches the section, the row
    // says its interval, and the sheet opens on ?cron=.
    await page.keyboard.press("Escape");
    await expect(sheet).toBeHidden();
    await page.getByTestId("sub-panel-item-cron").click();
    await expect(page).toHaveURL(/section=cron/);
    await expect(page.getByTestId("schedules-cron-sweep")).toContainText(
      "every 1h",
    );
    await page.getByTestId("schedules-cron-sweep").click();
    await expect(page).toHaveURL(/cron=sweep/);
    await expect(page.getByTestId("schedules-sheet")).toContainText("sweep");
  });
});
