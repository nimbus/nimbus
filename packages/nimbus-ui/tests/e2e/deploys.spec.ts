/**
 * UIR23 Deploys e2e: the Deploys page is reachable from the Build group of
 * the sidebar, reads the local-admin deploy history, and says that a
 * deploy arrives through the CLI when the server has recorded none. The
 * production binary here starts without an app directory, so it records
 * no activation; the history table, the function-path diff, and the
 * rollback confirmation are covered by the unit spec against an msw
 * backend, and the rollback itself by the server tests.
 */
import type { Page } from "@playwright/test";
import { expect, test } from "./fixtures/nimbus-server";

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

async function hideToasts(page: Page): Promise<void> {
  await page.addStyleTag({
    content: "[data-sonner-toaster] { display: none !important; }",
  });
}

test.describe("deploys", () => {
  test.skip(
    ({ isMobile }) => isMobile,
    "The sidebar row is a desktop surface; the smoke walk covers the shell on mobile.",
  );

  test("reaches Deploys from the sidebar and reads an empty history", async ({
    page,
    nimbusServer,
  }) => {
    const { baseURL, readToken } = nimbusServer;
    await authenticate(page, baseURL, readToken());

    // The route answers over the console session, and a server started
    // without an app directory has recorded no activation.
    const probe = await page.request.get(`${baseURL}/api/admin/deploys`, {
      headers: JSON_HEADERS,
    });
    expect(probe.status(), await probe.text()).toBe(200);
    expect(await probe.json()).toEqual({ active: null, activations: [] });

    await page.goto(`${baseURL}/ui/developer/compute`);
    await hideToasts(page);
    await expect(page.getByTestId("page-compute")).toBeVisible();
    const nav = page.getByTestId("sidebar-nav");
    await nav.getByTestId("nav-deploys").click();
    await expect(page).toHaveURL(/\/ui\/developer\/deploys$/);
    await expect(page.getByTestId("page-deploys")).toBeVisible();
    await expect(nav.getByTestId("nav-deploys")).toHaveAttribute(
      "aria-current",
      "page",
    );

    const empty = page.getByTestId("deploys-empty");
    await expect(empty).toBeVisible();
    await expect(empty).toContainText("No deploys yet");
    await expect(empty).toContainText("nimbus deploy");
    // The sub-panel reads the same empty history: no active bundle, no
    // activations.
    const panel = page.getByTestId("deploys-panel");
    await expect(panel).toContainText("none");
    await expect(page.getByTestId("deploys-panel-count")).toHaveText(
      "0 (0 retained)",
    );
    await page.screenshot({ path: `${PROOF_DIR}/UIR23-deploys-empty.png` });

    // Settings no longer lists Deploys: the page moved here.
    await page.goto(`${baseURL}/ui/operator/settings?section=deploys`);
    await hideToasts(page);
    await expect(page).toHaveURL(/section=general/);
  });
});
