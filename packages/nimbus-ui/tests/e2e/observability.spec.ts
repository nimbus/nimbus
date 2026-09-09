/**
 * UIR12 Observability e2e: the Logs tab groups log lines under the runs
 * that wrote them, the Runs tab opens a detail sheet that hands off to the
 * Logs tab, and the operator page reads every tenant by default.
 */
import type { Page } from "@playwright/test";
import { expect, test } from "./fixtures/nimbus-server";

const TENANT_ID = "obs-e2e";
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

// The mutation answers with the inserted document id; the events that
// belong to a run correlate on it, so the seed needs it back.
async function insertSystemRow(
  page: Page,
  baseURL: string,
  table: string,
  fields: Record<string, unknown>,
): Promise<string> {
  const res = await page.request.post(`${baseURL}/convex/_nimbus/mutation`, {
    data: { mutation: { type: "insert", table, fields } },
    headers: {
      "Content-Type": "application/json",
      Accept: "application/json",
    },
  });
  const text = await res.text();
  expect(res.status(), text).toBe(200);
  const id = findId(JSON.parse(text));
  expect(id, `no document id in mutation response: ${text}`).toBeTruthy();
  return id as string;
}

function findId(value: unknown): string | undefined {
  if (typeof value === "string") return value;
  if (typeof value !== "object" || value === null) return undefined;
  const record = value as Record<string, unknown>;
  for (const key of ["_id", "id", "documentId", "result", "value"]) {
    if (key in record) {
      const found = findId(record[key]);
      if (found) return found;
    }
  }
  return undefined;
}

async function hideToasts(page: Page): Promise<void> {
  await page.addStyleTag({
    content: "[data-sonner-toaster] { display: none !important; }",
  });
}

type Seed = { runIds: string[]; erroredRunId: string };

async function seed(page: Page, baseURL: string): Promise<Seed> {
  const tenantRes = await page.request.post(`${baseURL}/api/tenants`, {
    data: { id: TENANT_ID },
    headers: {
      "Content-Type": "application/json",
      Accept: "application/json",
    },
  });
  expect(tenantRes.status(), await tenantRes.text()).toBe(201);

  const now = Date.now();
  const erroredRunId = await insertSystemRow(page, baseURL, "runs", {
    functionPath: "messages:send",
    kind: "mutation",
    status: "error",
    durationMs: 1200,
    error: { message: "commit rejected" },
    startedAt: now - 3_000,
  });
  const listRunId = await insertSystemRow(page, baseURL, "runs", {
    functionPath: "messages:list",
    kind: "query",
    status: "ok",
    durationMs: 40,
    startedAt: now - 2_000,
  });
  const cronRunId = await insertSystemRow(page, baseURL, "runs", {
    functionPath: "crons:sweep",
    kind: "action",
    status: "ok",
    durationMs: 310,
    startedAt: now - 1_000,
  });
  await insertSystemRow(page, baseURL, "events", {
    source: "runtime",
    level: "error",
    category: "function",
    message: "commit rejected: conversation missing",
    correlationId: erroredRunId,
    createdAt: now - 2_900,
  });
  await insertSystemRow(page, baseURL, "events", {
    source: "http",
    level: "info",
    category: "request",
    message: "GET /api/health 200",
    createdAt: now - 500,
  });
  return { runIds: [erroredRunId, listRunId, cronRunId], erroredRunId };
}

test.describe("observability", () => {
  test.skip(
    ({ isMobile }) => isMobile,
    "The log stream, the run sheet, and the facet bar are desktop surfaces; the smoke walk covers the tab strip on mobile.",
  );

  test("groups log lines under runs, opens a run sheet, and hands off to the logs", async ({
    page,
    nimbusServer,
  }) => {
    const { baseURL, readToken } = nimbusServer;
    await authenticate(page, baseURL, readToken());
    const { runIds, erroredRunId } = await seed(page, baseURL);
    await page.addInitScript(
      ([key, tenant]) => localStorage.setItem(key, tenant),
      ["nimbus-ui:active-tenant", TENANT_ID],
    );

    // Logs: one group per seeded run plus the server group for the
    // uncorrelated line; the errored run's line sits under its run.
    await page.goto(`${baseURL}/ui/developer/observability?tab=logs`);
    await hideToasts(page);
    await expect(page.getByTestId("page-observability")).toBeVisible();
    await expect(page.getByTestId("observability-log-table")).toBeVisible();
    await expect(page.getByTestId("observability-filter-tenant")).toContainText(
      TENANT_ID,
    );
    for (const runId of runIds) {
      await expect(
        page.getByTestId(`observability-log-group-${runId}`),
      ).toBeVisible();
    }
    await expect(page.getByTestId("observability-log-group-server")).toBeVisible();
    const erroredGroup = page.getByTestId(
      `observability-log-group-${erroredRunId}`,
    );
    await expect(erroredGroup).toContainText("messages:send");
    await expect(erroredGroup).toContainText("commit rejected");
    await expect(
      page.locator('[data-testid^="observability-log-group-head-"]'),
    ).toHaveCount(4);
    await page.screenshot({ path: `${PROOF_DIR}/UIR12-logs.png` });

    // Runs: the DataTable lists the three runs; activating a row opens the
    // sheet with the run's status and error.
    await page.getByTestId("observability-tab-runs").click();
    await expect(page).toHaveURL(/tab=runs/);
    const table = page.getByTestId("observability-runs-table");
    await expect(table).toBeVisible();
    await expect(
      page.locator('[data-testid^="observability-run-row-"]'),
    ).toHaveCount(3);
    await page.screenshot({ path: `${PROOF_DIR}/UIR12-runs.png` });

    // The id carries a colon, which the address encodes.
    const encodedRunId = encodeURIComponent(erroredRunId);
    await page.getByTestId(`observability-run-row-${erroredRunId}`).click();
    await expect(page).toHaveURL(new RegExp(`run=${encodedRunId}`));
    const sheet = page.getByTestId("observability-run-sheet");
    await expect(sheet).toBeVisible();
    await expect(
      sheet.getByTestId("observability-run-sheet-head-status"),
    ).toContainText("error");
    await expect(
      sheet.getByTestId("observability-run-sheet-error"),
    ).toContainText("commit rejected");
    await expect(
      sheet.getByTestId(
        `observability-run-sheet-event-${await eventIdIn(sheet)}`,
      ),
    ).toBeVisible();
    // The sheet slides in; the shot waits for the transition to settle.
    await page.screenshot({
      path: `${PROOF_DIR}/UIR12-run-sheet.png`,
      animations: "disabled",
    });

    // Show in logs: the Logs tab opens narrowed to the run's correlation.
    await sheet.getByTestId("observability-run-sheet-show-logs").click();
    await expect(page).toHaveURL(/tab=logs/);
    await expect(page).toHaveURL(
      new RegExp(`correlationId=${encodedRunId}`),
    );
    await expect(
      page.locator('[data-testid^="observability-log-group-head-"]'),
    ).toHaveCount(1);
    await expect(
      page.getByTestId(`observability-log-group-${erroredRunId}`),
    ).toContainText("commit rejected");
  });

  test("reads every tenant on the operator page until one is chosen", async ({
    page,
    nimbusServer,
  }) => {
    const { baseURL, readToken } = nimbusServer;
    await authenticate(page, baseURL, readToken());
    await seed(page, baseURL);

    await page.goto(`${baseURL}/ui/operator/observability`);
    await hideToasts(page);
    await expect(page.getByTestId("page-admin-observability")).toBeVisible();
    await expect(page.getByTestId("observability-filter-tenant")).toContainText(
      "all tenants",
    );
    await expect(
      page.locator('[data-testid^="observability-log-group-head-"]'),
    ).toHaveCount(4);
    await page.screenshot({ path: `${PROOF_DIR}/UIR12-operator.png` });

    await page.goto(
      `${baseURL}/ui/operator/observability?tab=runs&tenant=${TENANT_ID}`,
    );
    await hideToasts(page);
    await expect(page.getByTestId("observability-filter-tenant")).toContainText(
      TENANT_ID,
    );
    await expect(
      page.locator('[data-testid^="observability-run-row-"]'),
    ).toHaveCount(3);
  });
});

// The sheet lists the run's correlated lines by event id; the id is the
// server's, so the test reads it off the first item.
async function eventIdIn(
  sheet: ReturnType<Page["getByTestId"]>,
): Promise<string> {
  const item = sheet.locator('[data-testid^="observability-run-sheet-event-"]');
  await expect(item).toHaveCount(1);
  const testid = await item.first().getAttribute("data-testid");
  return (testid ?? "").replace("observability-run-sheet-event-", "");
}
