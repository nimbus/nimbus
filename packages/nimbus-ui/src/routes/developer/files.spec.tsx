import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { useSearchMock, navigateMock, subPanelMock } = vi.hoisted(() => ({
  useSearchMock: vi.fn(),
  navigateMock: vi.fn(),
  subPanelMock: vi.fn(),
}));

// The current address the page reads; set per test and applied to the
// search reducer the page hands to navigate.
let currentSearch: Record<string, unknown> = {};
function setSearchTo(search: Record<string, unknown>) {
  currentSearch = search;
  useSearchMock.mockReturnValue(search);
}

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (config: Record<string, unknown>) => config,
  useSearch: (..._args: unknown[]) => useSearchMock(),
  useNavigate: () => navigateMock,
  Link: ({
    children,
    "data-testid": testId,
  }: {
    children: React.ReactNode;
    "data-testid"?: string;
  }) => (
    <a href="#mock" data-testid={testId}>
      {children}
    </a>
  ),
}));

vi.mock("../../shell/sub-panel", () => ({
  useContributeSubPanel: (spec: unknown) => {
    subPanelMock(spec);
  },
  useSubPanelSearch: () => "",
}));

const { objectsApiMock, toastMock } = vi.hoisted(() => ({
  objectsApiMock: {
    buckets: vi.fn(),
    list: vi.fn(),
    url: vi.fn(),
    readText: vi.fn(),
    upload: vi.fn(),
    remove: vi.fn(),
  },
  toastMock: Object.assign(vi.fn(), { success: vi.fn(), error: vi.fn() }),
}));

vi.mock("../../lib/api-mutations", () => ({ objects: objectsApiMock }));
vi.mock("sonner", () => ({ toast: toastMock }));

import { useUiStore } from "../../store/ui-store";
import { routeComponent } from "../../test/route-internals";
import { Route } from "./files";

const FilesPage = routeComponent(Route);

const NOW = Date.now();

const BUCKETS = [
  { bucket: "assets", objectCount: 4, totalBytes: 4_348 },
  { bucket: "backups", objectCount: 1, totalBytes: 2_048 },
];

const OBJECTS = [
  {
    bucket: "assets",
    key: "docs/readme.md",
    size: 120,
    contentType: "text/markdown",
    etag: "e1",
    lastModifiedMillis: NOW - 60_000,
  },
  {
    bucket: "assets",
    key: "docs/guide.md",
    size: 100,
    contentType: "text/markdown",
    etag: "e2",
    lastModifiedMillis: NOW - 50_000,
  },
  {
    bucket: "assets",
    key: "logo.png",
    size: 4_096,
    contentType: "image/png",
    etag: "e3",
    lastModifiedMillis: NOW - 40_000,
  },
  {
    bucket: "assets",
    key: "notes.txt",
    size: 12,
    contentType: "text/plain",
    etag: "e4",
    lastModifiedMillis: NOW - 30_000,
  },
];

function listing(objects = OBJECTS, truncated = false) {
  return {
    ok: true as const,
    data: { bucket: "assets", prefix: "", objects, truncated },
  };
}

// The mock navigate receives a search reducer; the tests apply it to the
// current search to see what address the page asked for.
function requestedSearch(call = -1): Record<string, unknown> {
  const calls = navigateMock.mock.calls;
  const args = calls.at(call)?.[0] as
    | { search: (prev: Record<string, unknown>) => Record<string, unknown> }
    | undefined;
  if (!args) throw new Error("navigate was not called");
  return args.search(currentSearch);
}

function lastSubPanel(): { title: string; children: React.ReactNode } {
  const spec = subPanelMock.mock.calls.at(-1)?.[0];
  if (!spec) throw new Error("no sub-panel contributed");
  return spec as { title: string; children: React.ReactNode };
}

function headerLabels(container: HTMLElement) {
  return Array.from(container.querySelectorAll('[role="columnheader"]')).map(
    (cell) => cell.textContent?.trim(),
  );
}

function transfer(files: File[], types = ["Files"]) {
  return { dataTransfer: { types, files, dropEffect: "none" } };
}

beforeEach(() => {
  navigateMock.mockReset();
  subPanelMock.mockReset();
  useSearchMock.mockReset();
  for (const fn of Object.values(objectsApiMock)) fn.mockReset();
  toastMock.mockReset();
  toastMock.success.mockReset();
  toastMock.error.mockReset();
  objectsApiMock.buckets.mockResolvedValue({
    ok: true,
    data: { buckets: BUCKETS },
  });
  objectsApiMock.list.mockResolvedValue(listing());
  objectsApiMock.url.mockImplementation(
    (
      tenant: string,
      bucket: string,
      key: string,
      options?: { download?: boolean },
    ) =>
      `/api/tenants/${tenant}/objects/${bucket}/${key}${
        options?.download ? "?download=1" : ""
      }`,
  );
  objectsApiMock.readText.mockResolvedValue({ ok: true, data: "hello nimbus" });
  objectsApiMock.upload.mockResolvedValue({ ok: true, data: OBJECTS[3] });
  objectsApiMock.remove.mockResolvedValue({ ok: true, data: undefined });
  setSearchTo({ bucket: "assets" });
  useUiStore.setState({ activeTenant: "acme" });
});

afterEach(() => {
  useUiStore.setState({ activeTenant: null });
});

describe("FilesPage scope", () => {
  it("asks for a tenant before it reads anything", () => {
    useUiStore.setState({ activeTenant: null });
    render(<FilesPage />);
    expect(screen.getByTestId("files-empty-tenant")).toBeInTheDocument();
    expect(objectsApiMock.buckets).not.toHaveBeenCalled();
    expect(objectsApiMock.list).not.toHaveBeenCalled();
  });

  it("lands on the first bucket when the address names none", async () => {
    setSearchTo({});
    render(<FilesPage />);
    await waitFor(() => expect(navigateMock).toHaveBeenCalled());
    expect(requestedSearch()).toMatchObject({ bucket: "assets" });
    expect(navigateMock.mock.calls.at(-1)?.[0]).toMatchObject({
      replace: true,
    });
  });

  it("lists the buckets in the sub-panel with their counts and sizes", async () => {
    render(<FilesPage />);
    await waitFor(() =>
      expect(objectsApiMock.buckets).toHaveBeenCalledWith("acme"),
    );
    await screen.findByTestId("files-row-logo.png");
    const spec = lastSubPanel();
    expect(spec.title).toBe("Files");
    const panel = render(spec.children as React.ReactElement);
    const assets = panel.getByTestId("sub-panel-item-bucket-assets");
    expect(assets).toHaveTextContent("assets");
    expect(assets).toHaveTextContent("4");
    expect(assets).toHaveTextContent("4.2 KiB");
    expect(panel.getByTestId("sub-panel-item-bucket-backups")).toBeVisible();
  });

  it("keeps a bucket the address names even before it holds an object", async () => {
    setSearchTo({ bucket: "media" });
    objectsApiMock.list.mockResolvedValue(listing([]));
    render(<FilesPage />);
    await screen.findByTestId("files-empty-prefix");
    const panel = render(lastSubPanel().children as React.ReactElement);
    expect(panel.getByTestId("sub-panel-item-bucket-media")).toBeVisible();
    expect(navigateMock).not.toHaveBeenCalled();
  });

  it("explains an empty account and offers the first upload", async () => {
    setSearchTo({});
    objectsApiMock.buckets.mockResolvedValue({
      ok: true,
      data: { buckets: [] },
    });
    render(<FilesPage />);
    expect(await screen.findByTestId("files-empty-buckets")).toBeVisible();
    expect(navigateMock).not.toHaveBeenCalled();
    expect(objectsApiMock.list).not.toHaveBeenCalled();
  });

  it("shows a failed bucket read as a failure, not an empty account", async () => {
    objectsApiMock.buckets.mockResolvedValue({
      ok: false,
      error: "Request failed: 503",
    });
    render(<FilesPage />);
    const failed = await screen.findByTestId("files-load-failed");
    expect(failed).toHaveTextContent("Could not load buckets");
    expect(failed).toHaveTextContent("Request failed: 503");
  });
});

describe("FilesPage listing", () => {
  it("holds the table geometry while the listing is in flight", () => {
    objectsApiMock.list.mockReturnValue(new Promise(() => undefined));
    render(<FilesPage />);
    const table = screen.getByTestId("files-table");
    expect(table).toHaveAttribute("aria-busy", "true");
    expect(headerLabels(table)).toEqual([
      "Name",
      "Size",
      "Type",
      "Modified",
      "",
    ]);
  });

  it("reads the bucket at the prefix and folds the next level into folders", async () => {
    render(<FilesPage />);
    await waitFor(() =>
      expect(objectsApiMock.list).toHaveBeenCalledWith(
        "acme",
        "assets",
        "",
        1000,
      ),
    );
    const folder = await screen.findByTestId("files-row-docs/");
    expect(folder).toHaveTextContent("docs");
    expect(folder).toHaveTextContent("2 items");
    const logo = screen.getByTestId("files-row-logo.png");
    expect(logo).toHaveTextContent("logo.png");
    expect(logo).toHaveTextContent("4 KiB");
    expect(logo).toHaveTextContent("image/png");
    expect(screen.queryByTestId("files-row-docs/readme.md")).toBeNull();
    // Folders come first, then objects by name.
    const rows = Array.from(
      screen
        .getByTestId("files-table")
        .querySelectorAll('[data-testid^="files-row-"]'),
    )
      .map((row) => row.getAttribute("data-testid"))
      .filter((id) => !id?.startsWith("files-row-actions-"));
    expect(rows).toEqual([
      "files-row-docs/",
      "files-row-logo.png",
      "files-row-notes.txt",
    ]);
  });

  it("opens a folder into the prefix and an object into the sheet", async () => {
    render(<FilesPage />);
    fireEvent.click(await screen.findByTestId("files-row-docs/"));
    expect(requestedSearch()).toMatchObject({
      bucket: "assets",
      prefix: "docs/",
    });
    fireEvent.click(screen.getByTestId("files-row-notes.txt"));
    expect(requestedSearch()).toMatchObject({
      bucket: "assets",
      object: "notes.txt",
    });
  });

  it("walks the prefix with the breadcrumb", async () => {
    setSearchTo({ bucket: "assets", prefix: "docs/" });
    render(<FilesPage />);
    await waitFor(() =>
      expect(objectsApiMock.list).toHaveBeenCalledWith(
        "acme",
        "assets",
        "docs/",
        1000,
      ),
    );
    const crumbs = screen.getByTestId("files-breadcrumb");
    expect(crumbs).toHaveTextContent("assets");
    expect(crumbs).toHaveTextContent("docs");
    expect(within(crumbs).getByTestId("breadcrumb-link-0")).toBeVisible();
    expect(
      await screen.findByTestId("files-row-docs/readme.md"),
    ).toHaveTextContent("readme.md");
  });

  it("says when the listing stopped short of the bucket", async () => {
    objectsApiMock.list.mockResolvedValue(listing(OBJECTS, true));
    render(<FilesPage />);
    expect(await screen.findByTestId("files-truncated")).toHaveTextContent(
      "first 1000",
    );
  });

  it("names an empty prefix and an empty bucket differently", async () => {
    objectsApiMock.list.mockResolvedValue(listing([]));
    const { unmount } = render(<FilesPage />);
    expect(await screen.findByTestId("files-empty-prefix")).toHaveTextContent(
      "No objects in assets",
    );
    unmount();
    setSearchTo({ bucket: "assets", prefix: "docs/" });
    render(<FilesPage />);
    expect(await screen.findByTestId("files-empty-prefix")).toHaveTextContent(
      "Nothing under docs/",
    );
  });

  it("shows a failed listing as a failure with a retry", async () => {
    objectsApiMock.list.mockResolvedValueOnce({
      ok: false,
      error: "Request failed: 500",
    });
    render(<FilesPage />);
    const failed = await screen.findByTestId("files-load-failed");
    expect(failed).toHaveTextContent("Could not load objects");
    fireEvent.click(within(failed).getByRole("button", { name: /try again/i }));
    await screen.findByTestId("files-row-logo.png");
    expect(objectsApiMock.list).toHaveBeenCalledTimes(2);
  });
});

describe("FilesPage upload", () => {
  it("uploads a dropped file under the prefix, reports progress, and reloads", async () => {
    setSearchTo({ bucket: "assets", prefix: "docs/" });
    let progress: ((p: { loaded: number; total: number }) => void) | undefined;
    let finish: (() => void) | undefined;
    objectsApiMock.upload.mockImplementation(
      (
        _tenant: string,
        _bucket: string,
        _key: string,
        _body: Blob,
        options: { onProgress?: typeof progress },
      ) => {
        progress = options.onProgress;
        return new Promise((resolve) => {
          finish = () => resolve({ ok: true, data: OBJECTS[0] });
        });
      },
    );
    render(<FilesPage />);
    await screen.findByTestId("files-row-docs/readme.md");
    expect(objectsApiMock.list).toHaveBeenCalledTimes(1);

    const file = new File(["hello"], "hello.txt", { type: "text/plain" });
    fireEvent.drop(screen.getByTestId("files-drop-zone"), transfer([file]));
    await waitFor(() =>
      expect(objectsApiMock.upload).toHaveBeenCalledWith(
        "acme",
        "assets",
        "docs/hello.txt",
        file,
        expect.objectContaining({ contentType: "text/plain" }),
      ),
    );
    const item = await screen.findByTestId("files-uploads-item-docs/hello.txt");
    expect(item).toHaveAttribute("data-status", "uploading");
    progress?.({ loaded: 2, total: 5 });
    await waitFor(() =>
      expect(
        screen.getByTestId("files-uploads-percent-docs/hello.txt"),
      ).toHaveTextContent("40%"),
    );
    finish?.();
    await waitFor(() =>
      expect(
        screen.getByTestId("files-uploads-item-docs/hello.txt"),
      ).toHaveAttribute("data-status", "done"),
    );
    await waitFor(() => expect(objectsApiMock.list).toHaveBeenCalledTimes(2));
    expect(toastMock.success).toHaveBeenCalledWith("Uploaded hello.txt");
  });

  it("takes files from the Upload button's chooser", async () => {
    render(<FilesPage />);
    await screen.findByTestId("files-row-logo.png");
    const input = screen.getByTestId("files-upload-input") as HTMLInputElement;
    expect(input.type).toBe("file");
    expect(input.multiple).toBe(true);
    const file = new File(["a,b"], "table.csv", { type: "text/csv" });
    Object.defineProperty(input, "files", {
      value: [file],
      configurable: true,
    });
    fireEvent.change(input);
    await waitFor(() =>
      expect(objectsApiMock.upload).toHaveBeenCalledWith(
        "acme",
        "assets",
        "table.csv",
        file,
        expect.objectContaining({ contentType: "text/csv" }),
      ),
    );
  });

  it("keeps a refused upload on the strip with the server's reason", async () => {
    objectsApiMock.upload.mockResolvedValue({
      ok: false,
      error: "Request failed: 413",
    });
    render(<FilesPage />);
    await screen.findByTestId("files-row-logo.png");
    const file = new File(["x"], "big.bin", { type: "" });
    fireEvent.drop(screen.getByTestId("files-drop-zone"), transfer([file]));
    const item = await screen.findByTestId("files-uploads-item-big.bin");
    await waitFor(() => expect(item).toHaveAttribute("data-status", "error"));
    expect(screen.getByTestId("files-uploads-error-big.bin")).toHaveTextContent(
      "Request failed: 413",
    );
    expect(objectsApiMock.upload).toHaveBeenCalledWith(
      "acme",
      "assets",
      "big.bin",
      file,
      expect.not.objectContaining({ contentType: expect.anything() }),
    );
    expect(toastMock.error).toHaveBeenCalled();
    expect(objectsApiMock.list).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByTestId("files-uploads-dismiss-big.bin"));
    expect(screen.queryByTestId("files-uploads-item-big.bin")).toBeNull();
  });
});

describe("FilesPage sheet", () => {
  it("shows the object's facts and a text preview, and links the download", async () => {
    setSearchTo({ bucket: "assets", object: "notes.txt" });
    render(<FilesPage />);
    const sheet = await screen.findByTestId("files-sheet");
    expect(sheet).toHaveTextContent("notes.txt");
    expect(within(sheet).getByTestId("files-sheet-size")).toHaveTextContent(
      "12 B",
    );
    expect(within(sheet).getByTestId("files-sheet-type")).toHaveTextContent(
      "text/plain",
    );
    expect(
      await within(sheet).findByTestId("files-sheet-preview-text"),
    ).toHaveTextContent("hello nimbus");
    expect(objectsApiMock.readText).toHaveBeenCalledWith(
      "acme",
      "assets",
      "notes.txt",
    );
    expect(within(sheet).getByTestId("files-sheet-download")).toHaveAttribute(
      "href",
      "/api/tenants/acme/objects/assets/notes.txt?download=1",
    );
  });

  it("renders an image object through the object route", async () => {
    setSearchTo({ bucket: "assets", object: "logo.png" });
    render(<FilesPage />);
    const image = await screen.findByTestId("files-sheet-preview-image");
    expect(image).toHaveAttribute(
      "src",
      "/api/tenants/acme/objects/assets/logo.png",
    );
    expect(objectsApiMock.readText).not.toHaveBeenCalled();
  });

  it("copies the console's object link", async () => {
    setSearchTo({ bucket: "assets", object: "notes.txt" });
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true,
    });
    render(<FilesPage />);
    const sheet = await screen.findByTestId("files-sheet");
    fireEvent.click(within(sheet).getByTestId("files-sheet-copy-link"));
    await waitFor(() =>
      expect(writeText).toHaveBeenCalledWith(
        `${window.location.origin}/api/tenants/acme/objects/assets/notes.txt`,
      ),
    );
    expect(toastMock).toHaveBeenCalled();
  });

  it("says when the address names a key the listing does not hold", async () => {
    setSearchTo({ bucket: "assets", object: "gone.txt" });
    render(<FilesPage />);
    expect(
      await screen.findByTestId("files-sheet-missing"),
    ).toBeInTheDocument();
    expect(screen.getByTestId("files-sheet")).toHaveTextContent(
      "Not in this listing",
    );
  });

  it("closes into the same bucket and prefix", async () => {
    setSearchTo({
      bucket: "assets",
      prefix: "docs/",
      object: "docs/readme.md",
    });
    render(<FilesPage />);
    const sheet = await screen.findByTestId("files-sheet");
    fireEvent.keyDown(sheet, { key: "Escape" });
    await waitFor(() => expect(navigateMock).toHaveBeenCalled());
    expect(requestedSearch()).toEqual({
      bucket: "assets",
      prefix: "docs/",
      object: undefined,
    });
  });
});

describe("FilesPage row actions", () => {
  it("opens the row menu from the actions button and from a right click", async () => {
    render(<FilesPage />);
    const row = await screen.findByTestId("files-row-notes.txt");
    fireEvent.contextMenu(row);
    let menu = await screen.findByTestId("files-row-menu");
    expect(within(menu).getByTestId("files-row-menu-open")).toBeVisible();
    expect(within(menu).getByTestId("files-row-menu-download")).toBeVisible();
    expect(within(menu).getByTestId("files-row-menu-copy-link")).toBeVisible();
    expect(within(menu).getByTestId("files-row-menu-delete")).toBeVisible();
    fireEvent.keyDown(menu, { key: "Escape" });
    await waitFor(() =>
      expect(screen.queryByTestId("files-row-menu")).toBeNull(),
    );

    fireEvent.click(screen.getByTestId("files-row-actions-docs/"));
    menu = await screen.findByTestId("files-row-menu");
    expect(within(menu).getByTestId("files-row-menu-open")).toHaveTextContent(
      "Open folder",
    );
    expect(within(menu).queryByTestId("files-row-menu-delete")).toBeNull();
  });

  it("deletes an object after the confirm and reloads the listing", async () => {
    render(<FilesPage />);
    fireEvent.contextMenu(await screen.findByTestId("files-row-notes.txt"));
    const menu = await screen.findByTestId("files-row-menu");
    fireEvent.click(within(menu).getByTestId("files-row-menu-delete"));
    const dialog = await screen.findByTestId("files-delete");
    expect(dialog).toHaveTextContent("notes.txt");
    expect(objectsApiMock.remove).not.toHaveBeenCalled();
    fireEvent.click(within(dialog).getByTestId("files-delete-confirm"));
    await waitFor(() =>
      expect(objectsApiMock.remove).toHaveBeenCalledWith(
        "acme",
        "assets",
        "notes.txt",
      ),
    );
    await waitFor(() => expect(objectsApiMock.list).toHaveBeenCalledTimes(2));
    expect(toastMock.success).toHaveBeenCalledWith("Deleted notes.txt");
    await waitFor(() =>
      expect(screen.queryByTestId("files-delete")).toBeNull(),
    );
  });

  it("deletes from the sheet and closes it", async () => {
    setSearchTo({ bucket: "assets", object: "notes.txt" });
    render(<FilesPage />);
    const sheet = await screen.findByTestId("files-sheet");
    fireEvent.click(within(sheet).getByTestId("files-sheet-delete"));
    const dialog = await screen.findByTestId("files-delete");
    fireEvent.click(within(dialog).getByTestId("files-delete-confirm"));
    await waitFor(() => expect(objectsApiMock.remove).toHaveBeenCalled());
    await waitFor(() => expect(navigateMock).toHaveBeenCalled());
    expect(requestedSearch()).toMatchObject({ object: undefined });
  });

  it("keeps the row when the delete is refused", async () => {
    objectsApiMock.remove.mockResolvedValue({
      ok: false,
      error: "Request failed: 404",
    });
    render(<FilesPage />);
    fireEvent.contextMenu(await screen.findByTestId("files-row-notes.txt"));
    fireEvent.click(
      within(await screen.findByTestId("files-row-menu")).getByTestId(
        "files-row-menu-delete",
      ),
    );
    const dialog = await screen.findByTestId("files-delete");
    fireEvent.click(within(dialog).getByTestId("files-delete-confirm"));
    await waitFor(() =>
      expect(screen.getByTestId("files-delete")).toHaveTextContent(
        "Request failed: 404",
      ),
    );
    expect(objectsApiMock.list).toHaveBeenCalledTimes(1);
  });
});

describe("FilesPage new bucket", () => {
  it("validates the name the way the server does and opens the bucket", async () => {
    render(<FilesPage />);
    await screen.findByTestId("files-row-logo.png");
    fireEvent.click(screen.getByTestId("files-new-bucket"));
    const dialog = await screen.findByTestId("files-new-bucket-dialog");
    const confirm = within(dialog).getByTestId(
      "files-new-bucket-dialog-confirm",
    );
    const input = within(dialog).getByTestId("files-new-bucket-name");
    expect(confirm).toHaveAttribute("aria-disabled", "true");
    fireEvent.change(input, { target: { value: "bad/name" } });
    expect(confirm).toHaveAttribute("aria-disabled", "true");
    expect(dialog).toHaveTextContent("cannot contain");
    fireEvent.change(input, { target: { value: "media" } });
    expect(confirm).toHaveAttribute("aria-disabled", "false");
    fireEvent.click(confirm);
    expect(requestedSearch()).toEqual({
      bucket: "media",
      prefix: undefined,
      object: undefined,
    });
    await waitFor(() =>
      expect(screen.queryByTestId("files-new-bucket-dialog")).toBeNull(),
    );
  });
});
