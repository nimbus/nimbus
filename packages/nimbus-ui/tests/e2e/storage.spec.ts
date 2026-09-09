// Storage in a real server: the Tables index as a DataTable, the documents
// grid virtualized past the threshold, column preferences that survive a
// reload, the right-click row menu, and the Schema and Indexes tabs. The walk
// seeds a tenant and its documents through the public REST routes; the
// server projects the `tables` directory row from those writes.

import type { Page } from "@playwright/test";
import { expect, test } from "./fixtures/nimbus-server";

const TENANT_ID = "storage-e2e";
const TABLE = "messages";
// One page is 200 rows. Seeding past it proves the pager and puts the grid
// over the virtualization threshold (100 rows).
const DOCUMENT_COUNT = 230;
const ACTIVE_TENANT_KEY = "nimbus-ui:active-tenant";
const COLUMN_PREFS_KEY = `nimbus-ui:columns:${TENANT_ID}:${TABLE}`;

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

async function seedTenant(page: Page, baseURL: string): Promise<void> {
  const res = await page.request.post(`${baseURL}/api/tenants`, {
    data: { id: TENANT_ID },
    headers: {
      "Content-Type": "application/json",
      Accept: "application/json",
    },
  });
  expect(res.status(), await res.text()).toBe(201);
}

async function seedDocuments(page: Page, baseURL: string): Promise<void> {
  const url = `${baseURL}/api/tenants/${TENANT_ID}/documents`;
  const headers = {
    "Content-Type": "application/json",
    Accept: "application/json",
  };
  for (let start = 0; start < DOCUMENT_COUNT; start += 25) {
    const batch = Array.from(
      { length: Math.min(25, DOCUMENT_COUNT - start) },
      (_, offset) => start + offset,
    );
    const results = await Promise.all(
      batch.map((i) =>
        page.request.post(url, {
          data: {
            table: TABLE,
            fields: {
              author: i % 2 === 0 ? "ada" : "grace",
              seq: i,
              body: `message ${i}`,
            },
          },
          headers,
        }),
      ),
    );
    for (const res of results) {
      expect(res.status(), await res.text()).toBe(201);
    }
  }
}

async function seedSchema(page: Page, baseURL: string): Promise<void> {
  const res = await page.request.put(
    `${baseURL}/api/tenants/${TENANT_ID}/schema/${TABLE}`,
    {
      data: {
        table: TABLE,
        fields: [
          { name: "author", field_type: "string", required: true },
          { name: "seq", field_type: "number", required: true },
          { name: "body", field_type: "string", required: false },
        ],
        indexes: [{ name: "by_author", fields: ["author"] }],
      },
      headers: {
        "Content-Type": "application/json",
        Accept: "application/json",
      },
    },
  );
  expect(res.status(), await res.text()).toBe(204);
}

// The server announces a newer release through a toast anchored bottom-right,
// over the pager. It is environment noise for this walk (its version comes
// from the release feed, so it cannot be pre-dismissed), so hide the toaster.
async function hideToasts(page: Page): Promise<void> {
  await page.addStyleTag({
    content: "[data-sonner-toaster] { display: none !important; }",
  });
}

async function renderedDocumentRows(page: Page): Promise<number> {
  return page.locator('[role="row"][data-testid^="documents-row-"]').count();
}

test.describe("storage", () => {
  test.skip(
    ({ isMobile }) => isMobile,
    "The tables index and the documents grid are desktop surfaces; the sheet is covered by the smoke walk.",
  );

  test("lists tables, virtualizes a full page, keeps column prefs, and switches tabs", async ({
    page,
    nimbusServer,
  }) => {
    const { baseURL, readToken } = nimbusServer;
    await authenticate(page, baseURL, readToken());
    await seedTenant(page, baseURL);
    await seedSchema(page, baseURL);
    await seedDocuments(page, baseURL);

    // The console reads the active tenant from localStorage at boot, the
    // same place the sidebar selector writes it.
    await page.addInitScript(
      ([key, tenant]) => {
        window.localStorage.setItem(key, tenant);
      },
      [ACTIVE_TENANT_KEY, TENANT_ID] as const,
    );

    // 1. Tables index: one DataTable row per table, with the projected
    //    row count, and a right-click menu that opens the schema view.
    await page.goto(`${baseURL}/ui/developer/storage`);
    await expect(page.getByTestId("page-tenant-tables")).toBeVisible();
    const tablesTable = page.getByTestId("tenant-tables-table");
    await expect(tablesTable).toBeVisible();
    const tableRow = page.getByTestId(`tenant-table-row-${TABLE}`);
    await expect(tableRow).toBeVisible();
    await expect(tableRow).toContainText("defined");
    await expect(tableRow).toContainText(String(DOCUMENT_COUNT));
    await expect(tablesTable).not.toHaveAttribute("aria-busy", "true");
    await page.screenshot({ path: "test-results/storage-tables.png" });

    await tableRow.click({ button: "right" });
    const tableMenu = page.getByTestId("tenant-table-row-menu");
    await expect(tableMenu).toBeVisible();
    await page.getByTestId("tenant-table-row-menu-schema").click();
    await expect(page).toHaveURL(
      new RegExp(`/developer/storage/${TABLE}\\?tab=schema`),
    );

    // 2. Schema tab: the seeded schema is the draft, full width, with drop
    //    enabled. Indexes tab: the seeded index, read-only.
    await expect(page.getByTestId("documents-schema-tab")).toBeVisible();
    await expect(page.getByTestId("documents-schema-textarea")).toHaveValue(
      /by_author/,
    );
    await expect(page.getByTestId("documents-schema-drop")).toBeEnabled();
    await expect(page.getByTestId("documents-table")).toHaveCount(0);
    await page.screenshot({ path: "test-results/storage-schema-tab.png" });

    await page.getByTestId("documents-tab-indexes").click();
    await expect(page).toHaveURL(/tab=indexes/);
    const indexesTable = page.getByTestId("documents-indexes-table");
    await expect(indexesTable).toBeVisible();
    await expect(indexesTable).toContainText("by_author");
    await expect(indexesTable).toContainText("author");
    await page.screenshot({ path: "test-results/storage-indexes-tab.png" });

    // 3. Documents tab: a 200-row page renders as a virtual grid with far
    //    fewer than 200 row elements, and the pager knows there is more.
    await page.getByTestId("documents-tab-documents").click();
    await expect(page).not.toHaveURL(/tab=/);
    const grid = page.getByTestId("documents-table");
    await expect(grid).toBeVisible();
    await expect(grid).not.toHaveAttribute("aria-busy", "true");
    await expect(grid).toHaveAttribute("data-virtual", "true");
    await expect(grid).toHaveAttribute("aria-rowcount", "201");
    await expect(page.getByTestId("documents-pagination")).toContainText(
      "200 rows",
    );
    await expect.poll(() => renderedDocumentRows(page)).toBeLessThanOrEqual(60);
    expect(await renderedDocumentRows(page)).toBeGreaterThan(0);
    await expect(page.getByTestId("documents-next-page")).toBeEnabled();
    await page.screenshot({ path: "test-results/storage-documents.png" });

    // Scrolling the grid swaps the rendered window without growing it.
    await grid.evaluate((el) => {
      el.scrollTop = el.scrollHeight;
    });
    await expect.poll(() => renderedDocumentRows(page)).toBeLessThanOrEqual(60);

    // 4. Column preferences: hide `body`, and the choice survives a reload
    //    because it lives in localStorage per tenant and table.
    await page.getByTestId("documents-column-chooser").click();
    await expect(page.getByTestId("documents-column-chooser-panel")).toBeVisible();
    await page.getByTestId("documents-column-toggle-body").click();
    await expect(page.getByTestId("documents-columns-hidden")).toContainText(
      "+1 hidden",
    );
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("documents-column-chooser-panel")).toHaveCount(
      0,
    );
    await expect(page.getByTestId("documents-sort-body")).toHaveCount(0);
    await expect
      .poll(() =>
        page.evaluate(
          (key) => window.localStorage.getItem(key),
          COLUMN_PREFS_KEY,
        ),
      )
      .toContain('"body"');

    await page.reload();
    await expect(page.getByTestId("documents-table")).toBeVisible();
    await expect(page.getByTestId("documents-columns-hidden")).toContainText(
      "+1 hidden",
    );
    await expect(page.getByTestId("documents-sort-body")).toHaveCount(0);
    await expect(page.getByTestId("documents-sort-author")).toBeVisible();

    // 5. The pager walks to the second page and back through the URL.
    await hideToasts(page);
    await page.getByTestId("documents-next-page").click();
    await expect(page).toHaveURL(/cursors=/);
    await expect(page.getByTestId("documents-pagination")).toContainText(
      `${DOCUMENT_COUNT - 200} rows`,
    );
    await expect(page.getByTestId("documents-next-page")).toBeDisabled();
    await page.getByTestId("documents-prev-page").click();
    await expect(page).not.toHaveURL(/cursors=/);
    await expect(page.getByTestId("documents-pagination")).toContainText(
      "200 rows",
    );
  });
});
