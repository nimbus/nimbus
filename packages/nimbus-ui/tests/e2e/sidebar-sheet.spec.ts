// The sidebar below 640px: a top bar with a menu button, and the sidebar
// body in a sheet behind it. Runs only in the mobile project; the desktop
// sidebar is covered by the unit spec and the smoke walk.

import type { Page } from "@playwright/test";
import { expect, test } from "./fixtures/nimbus-server";

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

test.describe("sidebar sheet", () => {
  test.skip(({ isMobile }) => !isMobile, "small-screen layout only");

  test("a 400px viewport shows the top bar and opens the sidebar in a sheet", async ({
    page,
    nimbusServer,
  }) => {
    const baseURL = nimbusServer.baseURL;
    await authenticate(page, baseURL, nimbusServer.readToken());
    await page.setViewportSize({ width: 400, height: 800 });

    await page.goto(`${baseURL}/ui/developer/`);
    await expect(page.getByTestId("page-overview")).toBeVisible();
    await expect(page.getByTestId("mobile-top-bar")).toBeVisible();
    await expect(page.getByTestId("sidebar")).toHaveCount(0);

    await page.getByTestId("mobile-menu-button").click();
    const sheet = page.getByTestId("sidebar-sheet");
    await expect(sheet).toBeVisible();
    await expect(sheet.getByTestId("view-switcher")).toBeVisible();
    await expect(sheet.getByTestId("nav-overview")).toHaveAttribute(
      "aria-current",
      "page",
    );
    await expect(sheet.getByTestId("nav-compute")).toBeVisible();
    await expect(sheet.getByTestId("sidebar-theme-toggle")).toBeVisible();
    await expect(sheet.getByTestId("sidebar-toggle")).toHaveCount(0);
    await page.screenshot({
      path: "test-results/sidebar-sheet-400.png",
      fullPage: false,
    });

    // A tap on a row navigates and closes the sheet behind it.
    await sheet.getByTestId("nav-compute").click();
    await expect(page.getByTestId("page-compute")).toBeVisible();
    await expect(sheet).toHaveCount(0);

    // The view switch inside the sheet lands on the operator home page.
    await page.getByTestId("mobile-menu-button").click();
    await page.getByTestId("view-switcher-operator").click();
    await expect(page.getByTestId("page-operator-nodes")).toBeVisible();
    await expect(page.getByTestId("sidebar-sheet")).toHaveCount(0);
  });
});
