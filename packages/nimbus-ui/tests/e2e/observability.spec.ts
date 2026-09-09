/**
 * UIR12 Observability e2e: the Logs tab groups log lines under the runs
 * that wrote them, the Runs tab opens a detail sheet that hands off to the
 * Logs tab, and the operator page reads every tenant by default.
 *
 * UIR19: the search field reads a page of matching lines from the server
 * with a count, and a run detail page opens the Logs tab narrowed to that
 * run, where the same field searches the run alone.
 *
 * UIR20: the Errors tab folds failed runs by fingerprint and drills into
 * the Runs tab narrowed to the group; the Traces tab draws a run's spans
 * as a waterfall with the error glyph on the span that failed.
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

// The failure identity the server writes for a thrown error: sixteen hex
// characters over the function, the class, and the normalized message.
const ERROR_FINGERPRINT = "3f9a1c0be7d24a58";

// The spans the server records for the errored mutation: its own span,
// one ok read, and the insert that threw. Seeded in the shape
// `RunSpanRecorder::snapshot` writes (crates/nimbus-system/src/records/trace.rs).
const ERRORED_SPANS = [
  {
    name: "messages:send",
    kind: "function",
    parent: null,
    startMs: 0,
    durationMs: 1200,
    status: "error",
  },
  {
    name: "convex.ctx.db.get",
    kind: "db",
    parent: 0,
    startMs: 12,
    durationMs: 6,
    status: "ok",
  },
  {
    name: "convex.ctx.db.insert",
    kind: "db",
    parent: 0,
    startMs: 900,
    durationMs: 280,
    status: "error",
  },
];

function erroredRunFields(startedAt: number): Record<string, unknown> {
  return {
    tenantId: TENANT_ID,
    functionPath: "messages:send",
    kind: "mutation",
    status: "error",
    durationMs: 1200,
    error: {
      message: "commit rejected",
      class: "function_thrown",
      location: "messages:12",
    },
    fingerprint: ERROR_FINGERPRINT,
    spans: ERRORED_SPANS,
    startedAt,
  };
}

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
  const erroredRunId = await insertSystemRow(
    page,
    baseURL,
    "runs",
    erroredRunFields(now - 3_000),
  );
  const listRunId = await insertSystemRow(page, baseURL, "runs", {
    tenantId: TENANT_ID,
    functionPath: "messages:list",
    kind: "query",
    status: "ok",
    durationMs: 40,
    startedAt: now - 2_000,
  });
  const cronRunId = await insertSystemRow(page, baseURL, "runs", {
    tenantId: TENANT_ID,
    functionPath: "crons:sweep",
    kind: "action",
    status: "ok",
    durationMs: 310,
    startedAt: now - 1_000,
  });
  await insertSystemRow(page, baseURL, "events", {
    tenantId: TENANT_ID,
    source: "runtime",
    level: "error",
    category: "function",
    message: "commit rejected: conversation missing",
    correlationId: erroredRunId,
    createdAt: now - 2_900,
  });
  await insertSystemRow(page, baseURL, "events", {
    tenantId: TENANT_ID,
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

  test("searches the lines and reads one run's lines from its detail page", async ({
    page,
    nimbusServer,
  }) => {
    const { baseURL, readToken } = nimbusServer;
    await authenticate(page, baseURL, readToken());
    const { erroredRunId } = await seed(page, baseURL);
    await page.addInitScript(
      ([key, tenant]) => localStorage.setItem(key, tenant),
      ["nimbus-ui:active-tenant", TENANT_ID],
    );

    // ?q= reads the search page: one seeded line says "commit", so the
    // count is one and the only group is the run that wrote it.
    await page.goto(`${baseURL}/ui/developer/observability?tab=logs&q=commit`);
    await hideToasts(page);
    const count = page.getByTestId("observability-log-search-count");
    await expect(count).toContainText("1 line matches");
    await expect(count).toContainText("in every recorded line");
    await expect(
      page.locator('[data-testid^="observability-log-group-head-"]'),
    ).toHaveCount(1);
    await expect(
      page.getByTestId(`observability-log-group-${erroredRunId}`),
    ).toContainText("commit rejected");
    await expect(page.getByTestId("observability-log-search")).toHaveValue(
      "commit",
    );
    await page.screenshot({ path: `${PROOF_DIR}/UIR19-search.png` });

    // The run detail page links to the Logs tab narrowed to the run; the
    // field there searches the run's lines and commits after a quiet gap.
    await page.goto(`${baseURL}/ui/developer/compute/runs/${erroredRunId}`);
    await hideToasts(page);
    await expect(page.getByTestId("run-detail-events-count")).toContainText(
      "1 line",
    );
    await page.getByTestId("run-detail-open-logs").click();
    await expect(page).toHaveURL(/tab=logs/);
    await expect(page).toHaveURL(
      new RegExp(`correlationId=${encodeURIComponent(erroredRunId)}`),
    );
    const field = page.getByTestId("observability-log-search");
    await expect(field).toHaveAttribute("placeholder", "search this run");
    await field.fill("missing");
    await expect(page).toHaveURL(/q=missing/);
    await expect(count).toContainText("1 line matches");
    await expect(
      page.getByTestId(`observability-log-group-${erroredRunId}`),
    ).toContainText("conversation missing");
    await page.screenshot({ path: `${PROOF_DIR}/UIR19-run-logs.png` });

    // A search the run's lines do not satisfy is a zero, not an empty
    // stream blamed on nothing.
    await field.fill("health");
    await expect(page).toHaveURL(/q=health/);
    await expect(count).toContainText("0 lines match");
    await expect(page.getByTestId("observability-log-empty")).toContainText(
      "Nothing matches the current filters",
    );
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

  test("folds failed runs into error groups and draws a run's trace", async ({
    page,
    nimbusServer,
  }) => {
    const { baseURL, readToken } = nimbusServer;
    await authenticate(page, baseURL, readToken());
    const { erroredRunId } = await seed(page, baseURL);
    // A second run that fails the same way: same fingerprint, so the
    // Errors tab folds the two into one group of two.
    const secondErroredRunId = await insertSystemRow(
      page,
      baseURL,
      "runs",
      erroredRunFields(Date.now() - 400),
    );
    await page.addInitScript(
      ([key, tenant]) => localStorage.setItem(key, tenant),
      ["nimbus-ui:active-tenant", TENANT_ID],
    );

    // Errors: one group for the two thrown runs, with its count, class,
    // and location; the scan note says the fold covered every failure.
    await page.goto(`${baseURL}/ui/developer/observability?tab=errors`);
    await hideToasts(page);
    await expect(page.getByTestId("observability-errors-table")).toBeVisible();
    const group = page.getByTestId(
      `observability-error-row-${ERROR_FINGERPRINT}`,
    );
    await expect(group).toBeVisible();
    await expect(group).toContainText("messages:send");
    await expect(group).toContainText("function_thrown");
    await expect(group).toContainText("commit rejected");
    await expect(group).toContainText("messages:12");
    await expect(
      page.locator('[data-testid^="observability-error-row-"]'),
    ).toHaveCount(1);
    await expect(page.getByTestId("observability-error-scan")).toContainText(
      "2 failed runs grouped",
    );
    await page.screenshot({ path: `${PROOF_DIR}/UIR20-errors.png` });

    // Drill-in: the Runs tab narrowed to the group lists the two runs and
    // names the group in a chip; clearing the chip shows all four.
    await group.click();
    await expect(page).toHaveURL(/tab=runs/);
    await expect(page).toHaveURL(
      new RegExp(`fingerprint=${ERROR_FINGERPRINT}`),
    );
    await expect(
      page.locator('[data-testid^="observability-run-row-"]'),
    ).toHaveCount(2);
    for (const runId of [erroredRunId, secondErroredRunId]) {
      await expect(
        page.getByTestId(`observability-run-row-${runId}`),
      ).toBeVisible();
    }
    const chip = page.getByTestId("observability-filter-run-fingerprint");
    await expect(chip).toContainText(ERROR_FINGERPRINT.slice(0, 8));
    await chip.click();
    await expect(page).not.toHaveURL(/fingerprint=/);
    await expect(
      page.locator('[data-testid^="observability-run-row-"]'),
    ).toHaveCount(4);

    // Traces: the run list carries a span count; picking the errored run
    // draws its waterfall with the error glyph on the insert that threw
    // and no glyph on the read that succeeded.
    await page.getByTestId("observability-tab-traces").click();
    await expect(page).toHaveURL(/tab=traces/);
    await expect(page.getByTestId("observability-traces-table")).toBeVisible();
    await expect(
      page.getByTestId("observability-trace-empty-title"),
    ).toContainText("Pick a run to read its trace");
    await page.getByTestId(`observability-trace-row-${erroredRunId}`).click();
    await expect(page).toHaveURL(
      new RegExp(`run=${encodeURIComponent(erroredRunId)}`),
    );
    const waterfall = page.getByTestId("observability-trace-waterfall");
    await expect(waterfall).toBeVisible();
    await expect(waterfall).toContainText("3 spans");
    await expect(
      page.getByTestId("observability-trace-waterfall-bar").getByRole("img"),
    ).toHaveAccessibleName("error");
    await expect(
      page.getByTestId("observability-trace-waterfall-span-1"),
    ).toContainText("convex.ctx.db.get");
    await expect(
      page.getByTestId("observability-trace-waterfall-span-1").getByRole("img"),
    ).toHaveCount(0);
    const failedSpan = page.getByTestId("observability-trace-waterfall-span-2");
    await expect(failedSpan).toContainText("convex.ctx.db.insert");
    await expect(failedSpan.getByRole("img")).toHaveAccessibleName("error");
    await page.screenshot({ path: `${PROOF_DIR}/UIR20-traces.png` });

    // The same waterfall sits in the run sheet, so the two tabs agree.
    await page.getByTestId("observability-tab-runs").click();
    await expect(page).toHaveURL(/tab=runs/);
    const sheet = page.getByTestId("observability-run-sheet");
    await expect(sheet).toBeVisible();
    await expect(
      page.getByTestId("observability-run-sheet-trace-span-2").getByRole("img"),
    ).toHaveAccessibleName("error");
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
