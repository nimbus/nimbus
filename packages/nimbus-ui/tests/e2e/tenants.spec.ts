/**
 * UIR14 Tenants e2e: Create is the one header action and opens a dialog;
 * a created tenant announces itself with a toast and lands as a row; a
 * refused create stays in the dialog with the server's reason; Delete
 * lives in the row menu behind a confirm dialog.
 */
import type { Page } from "@playwright/test";
import { expect, test } from "./fixtures/nimbus-server";

const TENANT_ID = "tenants-e2e";
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

async function listTenants(page: Page, baseURL: string): Promise<string[]> {
  const res = await page.request.get(`${baseURL}/api/tenants`);
  expect(res.status()).toBe(200);
  const body = (await res.json()) as { tenants: string[] };
  return body.tenants;
}

// The overview shots read better without the session toast over them.
async function hideToasts(page: Page): Promise<void> {
  await page.addStyleTag({
    content: "[data-sonner-toaster] { display: none !important; }",
  });
}

test.describe("tenants", () => {
  test.skip(
    ({ isMobile }) => isMobile,
    "The row menu and the create dialog are desktop surfaces; the smoke walk covers the page on mobile.",
  );

  test("creates a tenant, refuses a duplicate, and deletes from the row menu", async ({
    page,
    nimbusServer,
  }) => {
    const { baseURL, readToken } = nimbusServer;
    await authenticate(page, baseURL, readToken());

    await page.goto(`${baseURL}/ui/operator/tenants`);
    await expect(page.getByTestId("page-tenants")).toBeVisible();
    await expect(
      page.getByTestId("tenants-table").or(page.getByTestId("tenants-empty")),
    ).toBeVisible();

    // Create: the dialog opens on the field, Create is inert until an id is
    // typed, and the write ends in a toast and a row.
    await page.getByTestId("tenants-create").click();
    const createDialog = page.getByTestId("tenants-create-dialog");
    await expect(createDialog).toBeVisible();
    const input = createDialog.getByTestId("tenants-create-input");
    await expect(input).toBeFocused();
    await expect(
      createDialog.getByTestId("tenants-create-submit"),
    ).toHaveAttribute("aria-disabled", "true");
    await input.fill(TENANT_ID);
    await page.screenshot({
      path: `${PROOF_DIR}/UIR14-create-dialog.png`,
      animations: "disabled",
    });
    await createDialog.getByTestId("tenants-create-submit").click();
    await expect(
      page.locator("[data-sonner-toast]").filter({
        hasText: `Created tenant ${TENANT_ID}`,
      }),
    ).toBeVisible();
    await expect(createDialog).toBeHidden();
    await expect(page.getByTestId(`tenants-row-${TENANT_ID}`)).toBeVisible();
    expect(await listTenants(page, baseURL)).toContain(TENANT_ID);
    await page.screenshot({ path: `${PROOF_DIR}/UIR14-tenants.png` });

    // Refusal: the same id again is refused by the server, and the reason
    // lands in the dialog next to the field instead of vanishing.
    await page.getByTestId("tenants-create").click();
    await expect(createDialog).toBeVisible();
    await createDialog.getByTestId("tenants-create-input").fill(TENANT_ID);
    await createDialog.getByTestId("tenants-create-submit").click();
    await expect(createDialog.getByRole("alert")).toContainText(
      "already exists",
    );
    await expect(createDialog).toBeVisible();
    await createDialog.getByTestId("tenants-create-cancel").click();
    await expect(createDialog).toBeHidden();

    // ?create=1 is the tenant selector's handoff: the dialog opens on
    // arrival and dismissing it clears the flag from the address.
    await page.goto(`${baseURL}/ui/operator/tenants?create=1`);
    await expect(createDialog).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(createDialog).toBeHidden();
    await expect(page).not.toHaveURL(/create=/);

    // Delete: the row menu opens on right-click, the confirm dialog names
    // the tenant, and the write ends in a toast and a missing row.
    const row = page.getByTestId(`tenants-row-${TENANT_ID}`);
    await row.click({ button: "right" });
    const menu = page.getByTestId("tenants-row-menu");
    await expect(menu).toBeVisible();
    await menu.getByTestId("tenants-row-menu-delete").click();
    const deleteDialog = page.getByTestId("tenants-delete-dialog");
    await expect(deleteDialog).toBeVisible();
    await expect(deleteDialog).toContainText(`Delete tenant "${TENANT_ID}"?`);
    await page.screenshot({
      path: `${PROOF_DIR}/UIR14-delete-dialog.png`,
      animations: "disabled",
    });
    await deleteDialog.getByTestId("tenants-delete-dialog-confirm").click();
    await expect(
      page.locator("[data-sonner-toast]").filter({
        hasText: `Deleted tenant ${TENANT_ID}`,
      }),
    ).toBeVisible();
    await expect(deleteDialog).toBeHidden();
    await expect(row).toBeHidden();
    expect(await listTenants(page, baseURL)).not.toContain(TENANT_ID);
  });

  test("reads the node overview with labelled counts and the machines summary", async ({
    page,
    nimbusServer,
  }) => {
    const { baseURL, readToken } = nimbusServer;
    await authenticate(page, baseURL, readToken());

    // Overview: one sentence in the headline, identity in the facts line,
    // and a tenant tile whose count matches the API.
    await page.goto(`${baseURL}/ui/operator`);
    await hideToasts(page);
    await expect(page.getByTestId("page-operator-nodes")).toBeVisible();
    await expect(page.getByTestId("nodes-sentence")).toContainText(
      "The node is up",
    );
    await expect(page.getByTestId("nodes-fact-version")).toBeVisible();
    await expect(page.getByTestId("nodes-fact-uptime")).toContainText("up");
    const tenants = await listTenants(page, baseURL);
    await expect(page.getByTestId("nodes-hosted-tenants-count")).toHaveText(
      String(tenants.length),
    );
    await expect(page.getByTestId("nodes-hosted-listeners-subline")).not.toBeEmpty();
    await expect(page.getByTestId("node-health")).toContainText("ok");
    await page.screenshot({ path: `${PROOF_DIR}/UIR14-nodes.png` });

    // Machines: the header says what the count is, never a bare total.
    await page.goto(`${baseURL}/ui/operator/machines`);
    await hideToasts(page);
    await expect(page.getByTestId("page-machines")).toBeVisible();
    const total = page.getByTestId("machines-total");
    await expect(total).not.toHaveText("loading…");
    await expect(total).toContainText(/machine/);
    await page.screenshot({ path: `${PROOF_DIR}/UIR14-machines.png` });
  });

});
