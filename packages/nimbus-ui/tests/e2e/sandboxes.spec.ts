/**
 * UIR22 Sandboxes e2e: the Sandboxes page is reachable from the Run group
 * of the sidebar and reports, plainly, that this server mounts no sandbox
 * routes. The production binary runs no service manager, so the list and
 * the detail page show their unavailable and not-found states; the live
 * list, the console stream, and the create picker are covered by the unit
 * specs against an msw session backend.
 */
import type { Page } from "@playwright/test";
import { expect, test } from "./fixtures/nimbus-server";

const TENANT_ID = "sandbox-e2e";
const PROOF_DIR = "../../docs/private/plans/proof/nimbus-ui-rebuild";
const JSON_HEADERS = {
  "Content-Type": "application/json",
  Accept: "application/json",
};

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

async function createTenant(page: Page, baseURL: string): Promise<void> {
  const res = await page.request.post(`${baseURL}/api/tenants`, {
    data: { id: TENANT_ID },
    headers: JSON_HEADERS,
  });
  expect(res.status(), await res.text()).toBe(201);
}

async function hideToasts(page: Page): Promise<void> {
  await page.addStyleTag({
    content: "[data-sonner-toaster] { display: none !important; }",
  });
}

test.describe("sandboxes", () => {
  test.skip(
    ({ isMobile }) => isMobile,
    "The sidebar row and the detail header are desktop surfaces; the smoke walk covers the shell on mobile.",
  );

  test("reaches Sandboxes from the sidebar and names the missing routes", async ({
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

    // The server has no sandbox routes at all: the list route is a 404.
    const probe = await page.request.get(
      `${baseURL}/api/tenants/${TENANT_ID}/sandboxes`,
      { headers: JSON_HEADERS },
    );
    expect(probe.status()).toBe(404);

    await page.goto(`${baseURL}/ui/developer/services`);
    await hideToasts(page);
    await expect(page.getByTestId("page-services")).toBeVisible();
    const nav = page.getByTestId("sidebar-nav");
    await nav.getByTestId("nav-sandboxes").click();
    await expect(page).toHaveURL(/\/ui\/developer\/sandboxes$/);
    await expect(page.getByTestId("page-sandboxes")).toBeVisible();
    await expect(nav.getByTestId("nav-sandboxes")).toHaveAttribute(
      "aria-current",
      "page",
    );
    await expect(page.getByTestId("sandboxes-scope")).toContainText(
      TENANT_ID,
    );
    const unavailable = page.getByTestId("sandboxes-unavailable");
    await expect(unavailable).toBeVisible();
    await expect(unavailable).toContainText(
      "Sandbox routes not available on this server",
    );
    await expect(page.getByTestId("sandboxes-create")).toBeDisabled();
    await page.screenshot({
      path: `${PROOF_DIR}/UIR22-sandboxes-unavailable.png`,
    });

    // A direct link to a sandbox on such a server lands on the not-found
    // state with a way back, never a blank page.
    await page.goto(`${baseURL}/ui/developer/sandboxes/ghost?tab=console`);
    await hideToasts(page);
    await expect(page.getByTestId("page-sandbox-detail")).toBeVisible();
    const missing = page.getByTestId("sandbox-not-found");
    await expect(missing).toBeVisible();
    await expect(missing).toContainText("ghost");
    await page.screenshot({
      path: `${PROOF_DIR}/UIR22-sandbox-not-found.png`,
    });
    await missing.getByRole("link", { name: "Back to Sandboxes" }).click();
    await expect(page).toHaveURL(/\/ui\/developer\/sandboxes$/);
    await expect(page.getByTestId("sandboxes-unavailable")).toBeVisible();
  });
});
