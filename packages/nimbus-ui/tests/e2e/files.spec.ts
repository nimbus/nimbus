/**
 * UIR16 Files e2e: a fresh tenant has no bucket, New bucket opens one, an
 * upload through the console lands as an object the table lists, the sheet
 * previews it, the download route serves it, and delete removes it.
 */
import type { Page } from "@playwright/test";
import { expect, test } from "./fixtures/nimbus-server";

const TENANT_ID = "files-e2e";
const BUCKET = "assets";
const PROOF_DIR = "../../docs/private/plans/proof/nimbus-ui-rebuild";
const HELLO = "hello from nimbus";

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

async function hideToasts(page: Page): Promise<void> {
  await page.addStyleTag({
    content: "[data-sonner-toaster] { display: none !important; }",
  });
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

function objectUrl(baseURL: string, key: string): string {
  return `${baseURL}/api/tenants/${TENANT_ID}/objects/${BUCKET}/${key}`;
}

test.describe("files", () => {
  test.skip(
    ({ isMobile }) => isMobile,
    "The object table, the drop zone, and the sheet are desktop surfaces; the smoke walk covers the route on mobile.",
  );

  test("opens a bucket, uploads an object, previews it, and deletes it", async ({
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

    // A fresh tenant holds no bucket; the page says so instead of listing.
    await page.goto(`${baseURL}/ui/developer/files`);
    await hideToasts(page);
    await expect(page.getByTestId("page-files")).toBeVisible();
    await expect(page.getByTestId("files-empty-buckets")).toBeVisible();

    // New bucket opens the name; the listing under it is empty.
    await page.getByTestId("files-new-bucket").click();
    const dialog = page.getByTestId("files-new-bucket-dialog");
    await expect(dialog).toBeVisible();
    await dialog.getByTestId("files-new-bucket-name").fill(BUCKET);
    await dialog.getByTestId("files-new-bucket-dialog-confirm").click();
    await expect(page).toHaveURL(new RegExp(`bucket=${BUCKET}`));
    await expect(page.getByTestId("files-empty-prefix")).toBeVisible();
    await expect(page.getByTestId(`sub-panel-item-bucket-${BUCKET}`)).toBeVisible();

    // Upload through the chooser: the strip reports done and the table
    // lists the object with its size and declared type.
    await page.getByTestId("files-upload-input").setInputFiles({
      name: "hello.txt",
      mimeType: "text/plain",
      buffer: Buffer.from(HELLO),
    });
    await expect(page.getByTestId("files-uploads-item-hello.txt")).toHaveAttribute(
      "data-status",
      "done",
    );
    const helloRow = page.getByTestId("files-row-hello.txt");
    await expect(helloRow).toBeVisible();
    await expect(helloRow).toContainText(`${HELLO.length} B`);
    await expect(helloRow).toContainText("text/plain");

    // A second object under a folder, written through the same route the
    // console uses, folds into one folder row on reload.
    const put = await page.request.put(objectUrl(baseURL, "docs/readme.md"), {
      data: "# Readme",
      headers: { "Content-Type": "text/markdown" },
    });
    expect(put.status(), await put.text()).toBe(201);
    await page.reload();
    await hideToasts(page);
    const folderRow = page.getByTestId("files-row-docs/");
    await expect(folderRow).toBeVisible();
    await expect(folderRow).toContainText("1 item");
    await expect(page.getByTestId(`sub-panel-item-bucket-${BUCKET}`)).toContainText(
      "2",
    );
    await page.screenshot({ path: `${PROOF_DIR}/UIR16-files.png` });

    // The sheet previews the text object and links the download route.
    await page.getByTestId("files-row-hello.txt").click();
    await expect(page).toHaveURL(/object=hello\.txt/);
    const sheet = page.getByTestId("files-sheet");
    await expect(sheet).toBeVisible();
    await expect(sheet.getByTestId("files-sheet-type")).toContainText("text/plain");
    await expect(sheet.getByTestId("files-sheet-preview-text")).toContainText(
      HELLO,
    );
    const href = await sheet.getByTestId("files-sheet-download").getAttribute("href");
    expect(href).toContain("download=1");
    const download = await page.request.get(`${baseURL}${href}`);
    expect(download.status()).toBe(200);
    expect(download.headers()["content-disposition"]).toContain("attachment");
    expect(await download.text()).toBe(HELLO);
    await page.screenshot({
      path: `${PROOF_DIR}/UIR16-sheet.png`,
      animations: "disabled",
    });
    await page.keyboard.press("Escape");
    await expect(sheet).toBeHidden();

    // Folder navigation lives in the address; the breadcrumb walks back.
    await page.getByTestId("files-row-docs/").click();
    await expect(page).toHaveURL(/prefix=docs/);
    await expect(page.getByTestId("files-row-docs/readme.md")).toBeVisible();
    await page.getByTestId("files-breadcrumb").getByTestId("breadcrumb-link-0").click();
    await expect(page).not.toHaveURL(/prefix=/);
    await expect(page.getByTestId("files-row-hello.txt")).toBeVisible();

    // Delete from the row menu removes the object on the server.
    await page.getByTestId("files-row-hello.txt").click({ button: "right" });
    await page.getByTestId("files-row-menu-delete").click();
    await page.getByTestId("files-delete-confirm").click();
    await expect(page.getByTestId("files-row-hello.txt")).toBeHidden();
    const gone = await page.request.get(objectUrl(baseURL, "hello.txt"));
    expect(gone.status()).toBe(404);
  });
});
