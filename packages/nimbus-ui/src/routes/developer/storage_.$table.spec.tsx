import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactElement, ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (config: Record<string, unknown>) => config,
  Link: ({
    to,
    children,
    "data-testid": testId,
    className,
    "aria-current": ariaCurrent,
  }: {
    to: string;
    children: ReactNode;
    "data-testid"?: string;
    className?: string;
    "aria-current"?: "page";
  }) => (
    <a
      href={to}
      data-testid={testId}
      className={className}
      aria-current={ariaCurrent}
    >
      {children}
    </a>
  ),
}));

const { useQueryMock } = vi.hoisted(() => ({ useQueryMock: vi.fn() }));

vi.mock("@nimbus/nimbus/react", () => ({
  useQuery: (...args: unknown[]) => useQueryMock(...args),
  useNimbus: () => ({ url: "http://nimbus.example:9000/convex/_nimbus" }),
}));

const { removeMock, insertMock, updateMock, schemaGetMock } = vi.hoisted(
  () => ({
    removeMock: vi.fn(),
    insertMock: vi.fn(),
    updateMock: vi.fn(),
    schemaGetMock: vi.fn(async () => ({
      ok: false,
      error: "no schema",
      status: 404,
    })),
  }),
);

vi.mock("../../lib/api-mutations", () => ({
  documents: { remove: removeMock, insert: insertMock, update: updateMock },
  schema: { get: schemaGetMock, apply: vi.fn(), drop: vi.fn() },
}));

const { toastMock, refreshMock, pageRef } = vi.hoisted(() => {
  const error = vi.fn();
  const success = vi.fn();
  const base = Object.assign(vi.fn(), { error, success });
  return {
    toastMock: base,
    refreshMock: vi.fn(),
    pageRef: {
      current: { data: [], next_cursor: null, has_more: false } as {
        data: Record<string, unknown>[];
        next_cursor: string | null;
        has_more: boolean;
      },
    },
  };
});

vi.mock("sonner", () => ({ toast: toastMock }));

vi.mock("../../components/storage/tables-sub-panel", () => ({
  useTablesSubPanel: () => undefined,
}));

vi.mock("../../components/storage/use-document-page", () => ({
  useDocumentPage: () => ({
    page: pageRef.current,
    loading: false,
    pageError: null,
    pageNumber: 1,
    refresh: refreshMock,
    onNext: vi.fn(),
    onPrev: vi.fn(),
    reset: vi.fn(),
  }),
}));

import { useUiStore } from "../../store/ui-store";
import { routeComponent } from "../../test/route-internals";
import { Route } from "./storage_.$table";

const DOC_A = { _id: "doc_a", _creationTime: 1, body: "first" };
const DOC_B = { _id: "doc_b", _creationTime: 2, body: "second" };
const DOC_C = { _id: "doc_c", _creationTime: 3, body: "third" };

/** A promise the test resolves by hand, to hold one delete in flight. */
function deferred() {
  let resolve: () => void = () => undefined;
  const promise = new Promise<void>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

function renderPage(search: Record<string, unknown> = {}) {
  const route = Route as unknown as Record<string, unknown>;
  route.useParams = () => ({ table: "messages" });
  route.useSearch = () => search;
  route.useNavigate = () => () => undefined;
  const Component = routeComponent(Route);
  return render(<Component />);
}

// Every step waits for the element it is about to click: the bulk toolbar
// appears only once the selection lands, and the dialog only once that
// toolbar's Delete has been pressed. React commits those between the awaited
// click and the next line rather than during it.
async function selectAllAndDelete(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByLabelText("Select all on page"));
  await user.click(await screen.findByTestId("documents-bulk-delete"));
  await user.click(
    await screen.findByTestId("documents-delete-dialog-confirm"),
  );
}

beforeEach(() => {
  useQueryMock.mockReset();
  useQueryMock.mockReturnValue(undefined);
  removeMock.mockReset();
  toastMock.mockClear();
  toastMock.error.mockClear();
  toastMock.success.mockClear();
  refreshMock.mockClear();
  pageRef.current = {
    data: [DOC_A, DOC_B],
    next_cursor: null,
    has_more: false,
  };
  useUiStore.setState({ activeTenant: "acme" });
  window.localStorage.clear();
});

afterEach(() => {
  // Unmount before touching the store: the page subscribes to it, so resetting
  // the tenant with the tree still mounted re-renders it outside any act()
  // scope and every test reports an unwrapped-update warning.
  cleanup();
  useUiStore.setState({ activeTenant: null });
  window.localStorage.clear();
});

/**
 * A bulk delete counted its failures and threw away every message: the operator
 * read "Deleted 1/2 documents", the selection was cleared, and the surviving
 * documents came back on refresh with nothing marking them. The reason -- which
 * `ApiResult.error` already carries, and which differs per document -- was
 * never shown anywhere in the console.
 */
describe("bulk document delete partial failure", () => {
  it("names the document that failed and the reason it gave", async () => {
    const user = userEvent.setup();
    removeMock.mockImplementation(async (_t: string, _n: string, id: string) =>
      id === "doc_b"
        ? {
            ok: false,
            error: "Delete trigger rejected: 2 threads reference it",
          }
        : { ok: true, data: null },
    );
    renderPage();

    await selectAllAndDelete(user);

    await waitFor(() => expect(toastMock.error).toHaveBeenCalledTimes(1));
    const [message, options] = toastMock.error.mock.calls.at(-1) as [
      string,
      { description: ReactElement; duration: number },
    ];
    expect(message).toBe("Deleted 1/2 documents");
    // The reason is only ever shown here, so the toast has to outlive sonner's
    // four-second default.
    expect(options.duration).toBeGreaterThanOrEqual(10_000);

    const { container } = render(options.description);
    expect(container.textContent).toContain(
      "doc_b: Delete trigger rejected: 2 threads reference it",
    );
    // The document that was deleted is not listed as a failure.
    expect(container.textContent).not.toContain("doc_a");
  });

  it("leaves exactly the failures selected so the set can be retried", async () => {
    const user = userEvent.setup();
    removeMock.mockImplementation(async (_t: string, _n: string, id: string) =>
      id === "doc_b"
        ? { ok: false, error: "Permission denied" }
        : { ok: true, data: null },
    );
    renderPage();

    await selectAllAndDelete(user);

    await waitFor(() =>
      expect(screen.getByTestId("documents-bulk-toolbar")).toHaveTextContent(
        "1 selected",
      ),
    );
    expect(screen.getByTestId("documents-row-doc_b")).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.getByTestId("documents-row-doc_a")).not.toHaveAttribute(
      "aria-selected",
      "true",
    );
  });

  it("elides a long failure list rather than filling the screen", async () => {
    const user = userEvent.setup();
    pageRef.current = {
      data: Array.from({ length: 5 }, (_, i) => ({
        _id: `doc_${i}`,
        _creationTime: i,
        body: "x",
      })),
      next_cursor: null,
      has_more: false,
    };
    removeMock.mockResolvedValue({ ok: false, error: "Permission denied" });
    renderPage();

    await selectAllAndDelete(user);

    await waitFor(() => expect(toastMock.error).toHaveBeenCalledTimes(1));
    const [message, options] = toastMock.error.mock.calls.at(-1) as [
      string,
      { description: ReactElement },
    ];
    expect(message).toBe("Deleted 0/5 documents");
    const { container } = render(options.description);
    expect(container.textContent).toContain("doc_0: Permission denied");
    expect(container.textContent).toContain("and 2 more");
  });

  it("clears the selection and says nothing extra when every delete lands", async () => {
    const user = userEvent.setup();
    removeMock.mockResolvedValue({ ok: true, data: null });
    renderPage();

    await selectAllAndDelete(user);

    await waitFor(() =>
      expect(toastMock.success).toHaveBeenCalledWith("Deleted 2 documents"),
    );
    expect(toastMock.error).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(screen.queryByTestId("documents-bulk-toolbar")).toBeNull(),
    );
    expect(refreshMock).toHaveBeenCalled();
  });
});

/**
 * The delete loop is not tied to the component that starts it: it is a plain
 * async `for` over one request per document. Navigating away mid-delete --
 * sidebar, command palette, browser back -- used to leave it deleting the rest
 * of the set with nothing on screen saying so, because the only progress
 * surface is the confirm dialog and that leaves with the route.
 */
describe("bulk document delete outliving its route", () => {
  async function deleteThreeThenLeave(
    user: ReturnType<typeof userEvent.setup>,
  ) {
    pageRef.current = {
      data: [DOC_A, DOC_B, DOC_C],
      next_cursor: null,
      has_more: false,
    };
    const gate = deferred();
    const issued: string[] = [];
    removeMock.mockImplementation(
      async (_t: string, _n: string, id: string) => {
        issued.push(id);
        if (id === "doc_a") await gate.promise;
        return { ok: true, data: null };
      },
    );

    const { unmount } = renderPage();
    await selectAllAndDelete(user);
    // The first delete is in flight and the other two are still queued.
    await waitFor(() => expect(issued).toEqual(["doc_a"]));

    unmount();
    gate.resolve();
    await waitFor(() => expect(toastMock.error).toHaveBeenCalledTimes(1));
    return issued;
  }

  it("issues no further deletes once the table is gone", async () => {
    const user = userEvent.setup();
    const issued = await deleteThreeThenLeave(user);
    // doc_b and doc_c were never sent: the loop stopped at the first document
    // boundary after the route unmounted.
    expect(issued).toEqual(["doc_a"]);
    expect(refreshMock).not.toHaveBeenCalled();
  });

  it("accounts for the half-finished set instead of going quiet", async () => {
    const user = userEvent.setup();
    await deleteThreeThenLeave(user);

    const [message, options] = toastMock.error.mock.calls.at(-1) as [
      string,
      { description: ReactElement; duration: number },
    ];
    expect(message).toBe("Stopped after deleting 1 of 3 documents");
    expect(options.duration).toBeGreaterThanOrEqual(10_000);

    const { container } = render(options.description);
    // Not "Deleted 1/3", which reads as two failures that never happened.
    expect(container.textContent).toContain("2 documents were not touched");
    expect(container.textContent).toContain("messages");
  });
});

/** Answers the `tables.byName` query with a row and every other query with nothing. */
function serveTableMeta(schema: Record<string, unknown> | null) {
  useQueryMock.mockImplementation((_ref: unknown, args: unknown) =>
    args && typeof args === "object" && "name" in args
      ? { _id: "t1", tenantId: "demo", name: "messages", schema }
      : undefined,
  );
}

/**
 * Documents, Schema and Indexes are peers of one table, addressed by the
 * `tab` search param so each has a URL and the back button works.
 */
describe("table views", () => {
  it("shows the documents grid by default and links the peer views", () => {
    renderPage();
    expect(screen.getByTestId("documents-table")).toBeInTheDocument();
    expect(screen.getByTestId("documents-tab-documents")).toHaveAttribute(
      "aria-current",
      "page",
    );
    expect(screen.getByTestId("documents-tab-schema")).toBeInTheDocument();
    expect(screen.getByTestId("documents-tab-indexes")).toBeInTheDocument();
    expect(screen.getByTestId("documents-tab-query")).toBeInTheDocument();
  });

  it("gives the schema view the page instead of a side panel", () => {
    serveTableMeta({ table: "messages", fields: [], indexes: [] });
    renderPage({ tab: "schema" });
    expect(screen.getByTestId("documents-schema-tab")).toBeInTheDocument();
    expect(screen.queryByTestId("documents-table")).toBeNull();
    expect(screen.getByTestId("documents-tab-schema")).toHaveAttribute(
      "aria-current",
      "page",
    );
  });

  // The editor seeds its draft on mount. Opening the tab straight from a
  // link, before the table row has loaded, must not seed an empty draft
  // that then hides the real schema behind a stale textarea.
  it("waits for the table row before it seeds the schema editor", () => {
    renderPage({ tab: "schema" });
    expect(screen.getByTestId("documents-schema-loading")).toBeInTheDocument();
    expect(screen.queryByTestId("documents-schema-textarea")).toBeNull();
  });

  it("seeds the editor with the table's schema", () => {
    serveTableMeta({
      table: "messages",
      fields: [{ name: "author", field_type: "string", required: true }],
      indexes: [{ name: "by_author", fields: ["author"] }],
    });
    renderPage({ tab: "schema" });
    const textarea = screen.getByTestId(
      "documents-schema-textarea",
    ) as HTMLTextAreaElement;
    expect(textarea.value).toContain("by_author");
  });

  it("shows the indexes view on its own tab", () => {
    renderPage({ tab: "indexes" });
    expect(screen.getByTestId("documents-indexes-tab")).toBeInTheDocument();
    expect(screen.queryByTestId("documents-table")).toBeNull();
  });
});

describe("DocumentsPage empty state", () => {
  it("offers the insert drawer and the insert command for this table", async () => {
    pageRef.current = { data: [], next_cursor: null, has_more: false };
    renderPage();

    const empty = await screen.findByTestId("documents-empty");
    expect(empty).toHaveTextContent("No documents");
    expect(screen.getByTestId("documents-empty-snippet")).toHaveTextContent(
      "http://nimbus.example:9000/api/tenants/acme/documents",
    );
    expect(screen.getByTestId("documents-empty-snippet")).toHaveTextContent(
      '"table": "messages"',
    );
    await userEvent.setup().click(screen.getByTestId("documents-empty-cta"));
    expect(await screen.findByTestId("documents-insert-drawer")).toBeVisible();
  });

  it("blames the filter, with no command, when a filter is set", async () => {
    pageRef.current = { data: [], next_cursor: null, has_more: false };
    renderPage({ filters: [{ field: "body", op: "eq", value: "x" }] });

    const empty = await screen.findByTestId("documents-empty");
    expect(empty).toHaveTextContent("No documents match the filter");
    expect(screen.queryByTestId("documents-empty-snippet")).toBeNull();
  });
});
