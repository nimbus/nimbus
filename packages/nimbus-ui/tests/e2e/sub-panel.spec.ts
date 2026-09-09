// The resizable sub-panel on the desktop tier: drag, keyboard, collapse and
// per-section persistence in a real layout engine. The unit spec covers the
// same contract with a geometry shim; this spec proves the react-resizable-
// panels group measures the columns the way the shell expects in Chromium.
// Runs only in the chromium project: below the desktop tier the panel is a
// rail and its list opens as a sheet, which the smoke walk covers.

import type { Page } from "@playwright/test";
import { expect, test } from "./fixtures/nimbus-server";

const SETTINGS_KEY = "nimbus-ui:panel:settings";
const DEFAULT_WIDTH = 240;
const MAX_WIDTH = 400;
const RAIL_WIDTH = 32;

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

async function panelWidth(page: Page): Promise<number> {
  const box = await page.getByTestId("sub-panel").boundingBox();
  if (!box) throw new Error("sub-panel has no box");
  return Math.round(box.width);
}

async function storedPrefs(
  page: Page,
): Promise<{ width: number; collapsed: boolean } | null> {
  return page.evaluate((key) => {
    const raw = window.localStorage.getItem(key);
    return raw ? (JSON.parse(raw) as { width: number; collapsed: boolean }) : null;
  }, SETTINGS_KEY);
}

async function dragSeparator(page: Page, deltaX: number): Promise<void> {
  const handle = page.getByRole("separator", { name: "Resize sub-panel" });
  const box = await handle.boundingBox();
  if (!box) throw new Error("separator has no box");
  const x = box.x + box.width / 2;
  const y = box.y + box.height / 2;
  await page.mouse.move(x, y);
  await page.mouse.down();
  // Two intermediate moves so the library sees a drag, not a click.
  await page.mouse.move(x + deltaX / 2, y);
  await page.mouse.move(x + deltaX, y);
  await page.mouse.up();
}

test.describe("sub-panel resizing", () => {
  test.skip(({ isMobile }) => isMobile, "desktop tier only");

  test("drags, persists per section, resizes from the keyboard and collapses to the rail", async ({
    page,
    nimbusServer,
  }) => {
    const baseURL = nimbusServer.baseURL;
    await authenticate(page, baseURL, nimbusServer.readToken());
    await page.setViewportSize({ width: 1280, height: 800 });

    await page.goto(`${baseURL}/ui/developer/settings`);
    await expect(page.getByTestId("page-settings")).toBeVisible();
    const panel = page.getByTestId("sub-panel");
    await expect(panel).toHaveAttribute("data-collapsed", "false");
    expect(await panelWidth(page)).toBe(DEFAULT_WIDTH);
    await expect
      .poll(() => storedPrefs(page))
      .toEqual({ width: DEFAULT_WIDTH, collapsed: false });
    await page.screenshot({ path: "test-results/sub-panel-default.png" });

    // Drag the separator 80px to the right.
    await dragSeparator(page, 80);
    await expect.poll(() => panelWidth(page)).toBe(DEFAULT_WIDTH + 80);
    await expect
      .poll(() => storedPrefs(page))
      .toEqual({ width: DEFAULT_WIDTH + 80, collapsed: false });
    await page.screenshot({ path: "test-results/sub-panel-dragged.png" });

    // The width survives a reload, and is the Settings width only.
    await page.reload();
    await expect(page.getByTestId("page-settings")).toBeVisible();
    await expect.poll(() => panelWidth(page)).toBe(DEFAULT_WIDTH + 80);
    await page.goto(`${baseURL}/ui/developer/storage`);
    await expect(page.getByTestId("page-tenant-tables")).toBeVisible();
    await expect.poll(() => panelWidth(page)).toBe(DEFAULT_WIDTH);

    // Back on Settings, the separator is a keyboard target. One arrow step
    // is 5% of the group, so the assertions check direction and the limits
    // rather than a pixel count. `Home` is not asserted: on a collapsible
    // panel the library treats it as "collapse", the same as a step past
    // the minimum.
    await page.goto(`${baseURL}/ui/developer/settings`);
    await expect(page.getByTestId("page-settings")).toBeVisible();
    await expect.poll(() => panelWidth(page)).toBe(DEFAULT_WIDTH + 80);
    const handle = page.getByRole("separator", { name: "Resize sub-panel" });
    await handle.focus();
    await expect(handle).toBeFocused();
    await page.keyboard.press("ArrowLeft");
    await expect.poll(() => panelWidth(page)).toBeLessThan(DEFAULT_WIDTH + 80);
    await page.keyboard.press("End");
    await expect.poll(() => panelWidth(page)).toBe(MAX_WIDTH);
    await expect
      .poll(() => storedPrefs(page))
      .toEqual({ width: MAX_WIDTH, collapsed: false });

    // Enter on the separator collapses to the rail; the last width stays
    // stored so the expand button restores it.
    await page.keyboard.press("Enter");
    await expect(panel).toHaveAttribute("data-collapsed", "true");
    await expect.poll(() => panelWidth(page)).toBe(RAIL_WIDTH);
    await expect
      .poll(() => storedPrefs(page))
      .toEqual({ width: MAX_WIDTH, collapsed: true });
    await page.screenshot({ path: "test-results/sub-panel-collapsed.png" });

    // The collapsed state survives a reload.
    await page.reload();
    await expect(page.getByTestId("page-settings")).toBeVisible();
    await expect(panel).toHaveAttribute("data-collapsed", "true");
    await expect.poll(() => panelWidth(page)).toBe(RAIL_WIDTH);

    await page.getByTestId("sub-panel-toggle").click();
    await expect(panel).toHaveAttribute("data-collapsed", "false");
    await expect.poll(() => panelWidth(page)).toBe(MAX_WIDTH);
    await expect
      .poll(() => storedPrefs(page))
      .toEqual({ width: MAX_WIDTH, collapsed: false });

    // The collapse button in the header is the pointer path to the rail.
    await page.getByTestId("sub-panel-toggle").click();
    await expect(panel).toHaveAttribute("data-collapsed", "true");
    await expect.poll(() => panelWidth(page)).toBe(RAIL_WIDTH);
  });
});
