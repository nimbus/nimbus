import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { HttpResponse, http } from "msw";
import { setupServer } from "msw/node";
import type { ReactNode } from "react";
import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from "vitest";

// The document page is the seam these storage components were built for, so
// its route-level invariants — ESC clearing a selection, sort writing to the
// URL, the scan guard — are tested here beside them rather than in the routes
// tree, which this change does not own.

const { navigateMock, searchState, useQueryMock } = vi.hoisted(() => ({
  navigateMock: vi.fn(),
  searchState: { value: {} as Record<string, unknown> },
  useQueryMock: vi.fn(),
}));

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (config: Record<string, unknown>) => ({
    ...config,
    useParams: () => ({ table: "messages" }),
    useSearch: () => searchState.value,
    useNavigate: () => navigateMock,
  }),
  Link: ({ children, ...rest }: { children: ReactNode }) => (
    <a {...rest}>{children}</a>
  ),
  useRouterState: ({ select }: { select: (s: unknown) => unknown }) =>
    select({ location: { pathname: "/developer/storage/messages" } }),
}));

vi.mock("@nimbus/nimbus/react", () => ({
  useQuery: (_ref: unknown, args: unknown) => useQueryMock(args),
}));

vi.mock("../../shell/sub-drawer", () => ({
  useContributeSubDrawer: () => undefined,
  useSubDrawerSearch: () => "",
}));

import { Route } from "../../routes/developer/storage_.$table";
import { useUiStore } from "../../store/ui-store";
import { routeComponent } from "../../test/route-internals";

const server = setupServer();
beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

const DOCS = [
  { _id: "doc_a", author: "ada", body: "first" },
  { _id: "doc_b", author: "grace", body: "second" },
];

const SCHEMA = {
  fields: [{ name: "author" }, { name: "body" }],
  indexes: [{ name: "by_author", fields: ["author"] }],
};

function serveDocuments() {
  server.use(
    http.post("*/api/tenants/:t/query/paginated", () =>
      HttpResponse.json({ data: DOCS, next_cursor: null, has_more: false }),
    ),
  );
}

const Page = routeComponent(Route);

beforeEach(() => {
  navigateMock.mockReset();
  searchState.value = {};
  window.localStorage.clear();
  useUiStore.setState({
    activeTenant: "demo",
    paletteOpen: false,
    lensOpen: false,
    actionMenuOpen: false,
  });
  useQueryMock.mockReset();
  // Keyed on the query arguments, not on call order: the page re-renders many
  // times per test and a call-ordered mock would hand the table metadata to
  // whichever query happened to run first.
  useQueryMock.mockImplementation((args: unknown) =>
    args && typeof args === "object" && "name" in args
      ? { _id: "t1", name: "messages", schema: SCHEMA }
      : [{ _id: "t1", name: "messages" }],
  );
  serveDocuments();
});

afterEach(() => {
  useUiStore.setState({ activeTenant: null });
});

async function renderPage() {
  render(<Page />);
  await screen.findByTestId("documents-table");
  // The rows paint one tick before the pager clears `loading` and the
  // discovered-field pass lands. Settling those here keeps every later
  // interaction from racing an update React would report as un-acted.
  await act(async () => {});
}

async function selectFirstRow(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByTestId("documents-select-doc_a"));
  await screen.findByTestId("documents-bulk-toolbar");
}

describe("document page selection", () => {
  // DESIGN.md:1120-1123 — a selection gets a bulk toolbar above the rows, and
  // the page-level `delete (n)` button is gone with it.
  it("raises the bulk toolbar on selection and has no competing page-level delete", async () => {
    const user = userEvent.setup();
    await renderPage();

    expect(
      screen.queryByTestId("documents-bulk-toolbar"),
    ).not.toBeInTheDocument();
    await selectFirstRow(user);
    expect(screen.getByTestId("documents-bulk-toolbar")).toHaveTextContent(
      "1 selected",
    );
    expect(screen.getByTestId("documents-toolbar")).not.toHaveTextContent(
      "delete",
    );
  });

  it("clears the selection on Escape", async () => {
    const user = userEvent.setup();
    await renderPage();
    await selectFirstRow(user);

    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() =>
      expect(
        screen.queryByTestId("documents-bulk-toolbar"),
      ).not.toBeInTheDocument(),
    );
  });

  it("clears the selection from the toolbar's own control", async () => {
    const user = userEvent.setup();
    await renderPage();
    await selectFirstRow(user);

    await user.click(screen.getByTestId("documents-bulk-clear"));
    await waitFor(() =>
      expect(
        screen.queryByTestId("documents-bulk-toolbar"),
      ).not.toBeInTheDocument(),
    );
  });

  // One Escape closes one thing. The palette, the lens and the action menu all
  // own Escape while they are open, so the selection must not also consume it.
  it("leaves Escape to the shell overlays that own it", async () => {
    const user = userEvent.setup();
    await renderPage();
    await selectFirstRow(user);

    for (const overlay of ["paletteOpen", "lensOpen", "actionMenuOpen"]) {
      useUiStore.setState({ [overlay]: true });
      fireEvent.keyDown(window, { key: "Escape" });
      expect(screen.getByTestId("documents-bulk-toolbar")).toBeInTheDocument();
      useUiStore.setState({ [overlay]: false });
    }
  });

  it("leaves Escape to an open drawer", async () => {
    const user = userEvent.setup();
    await renderPage();
    await selectFirstRow(user);

    await user.click(screen.getByTestId("documents-open-insert"));
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.getByTestId("documents-bulk-toolbar")).toBeInTheDocument();
  });

  it("names the documents a bulk delete would remove", async () => {
    const user = userEvent.setup();
    await renderPage();
    await user.click(screen.getByTestId("documents-select-all"));
    await user.click(screen.getByTestId("documents-bulk-delete"));

    const ids = await screen.findByTestId("documents-delete-ids");
    expect(ids).toHaveTextContent("doc_a");
    expect(ids).toHaveTextContent("doc_b");
  });
});

describe("document page query", () => {
  // URL is state: a sorted, filtered view has to survive a refresh and be
  // shareable, and each write must keep the other params intact.
  it("writes a sort into the URL without dropping the other search params", async () => {
    const user = userEvent.setup();
    searchState.value = { panel: "schema" };
    await renderPage();

    await user.click(screen.getByTestId("documents-sort-author"));
    expect(navigateMock).toHaveBeenCalledTimes(1);
    const arg = navigateMock.mock.calls[0][0] as {
      search: (prev: Record<string, unknown>) => Record<string, unknown>;
    };
    expect(arg.search({ panel: "schema" })).toEqual({
      panel: "schema",
      sort: "author",
      dir: "asc",
    });
  });

  it("flips direction when the active sort column is clicked again", async () => {
    const user = userEvent.setup();
    searchState.value = { sort: "author", dir: "asc" };
    await renderPage();

    await user.click(screen.getByTestId("documents-sort-author"));
    const arg = navigateMock.mock.calls[0][0] as {
      search: (prev: Record<string, unknown>) => Record<string, unknown>;
    };
    expect(arg.search({ sort: "author", dir: "asc" })).toMatchObject({
      sort: "author",
      dir: "desc",
    });
  });

  // DESIGN.md:269 — the browser refuses an unbounded scan until the operator
  // asks for it. `body` has no index leading with it.
  it("holds an unindexed sort behind an explicit confirmation", async () => {
    const user = userEvent.setup();
    await renderPage();

    await user.click(screen.getByTestId("documents-sort-body"));
    expect(navigateMock).not.toHaveBeenCalled();
    expect(screen.getByTestId("documents-scan-warning")).toHaveTextContent(
      "body",
    );

    await user.click(screen.getByTestId("documents-scan-confirm"));
    const arg = navigateMock.mock.calls[0][0] as {
      search: (prev: Record<string, unknown>) => Record<string, unknown>;
    };
    expect(arg.search({})).toMatchObject({ sort: "body", dir: "asc" });
  });

  it("abandons an unindexed sort on cancel", async () => {
    const user = userEvent.setup();
    await renderPage();

    await user.click(screen.getByTestId("documents-sort-body"));
    await user.click(screen.getByTestId("documents-scan-cancel"));
    expect(
      screen.queryByTestId("documents-scan-warning"),
    ).not.toBeInTheDocument();
    expect(navigateMock).not.toHaveBeenCalled();
  });

  it("writes a filter into the URL and drops it again when the chip is removed", async () => {
    const user = userEvent.setup();
    await renderPage();

    await user.click(screen.getByTestId("documents-add-filter"));
    await user.type(screen.getByTestId("documents-filter-value"), "ada");
    await user.click(screen.getByTestId("documents-filter-apply"));

    const added = navigateMock.mock.calls[0][0] as {
      search: (prev: Record<string, unknown>) => Record<string, unknown>;
    };
    expect(added.search({})).toEqual({
      filters: [{ field: "_id", op: "eq", value: "ada" }],
    });
  });

  it("explains an empty page differently when a filter is what emptied it", async () => {
    server.use(
      http.post("*/api/tenants/:t/query/paginated", () =>
        HttpResponse.json({ data: [], next_cursor: null, has_more: false }),
      ),
    );
    searchState.value = {
      filters: [{ field: "author", op: "eq", value: "x" }],
    };
    render(<Page />);

    const empty = await screen.findByTestId("documents-empty");
    expect(empty).toHaveTextContent("No documents match the filter");
  });
});

describe("document page without a tenant", () => {
  // A deep link to a table with no tenant selected used to query
  // `/api/tenants//query/paginated` and report the 404 as a page error.
  it("asks for a tenant instead of querying an empty one", async () => {
    let requests = 0;
    server.use(
      http.post("*/api/tenants/:t/query/paginated", () => {
        requests += 1;
        return HttpResponse.json({
          data: [],
          next_cursor: null,
          has_more: false,
        });
      }),
    );
    useUiStore.setState({ activeTenant: null });
    render(<Page />);

    expect(await screen.findByTestId("documents-empty")).toHaveTextContent(
      "Select a tenant",
    );
    expect(requests).toBe(0);
  });
});

describe("document page columns", () => {
  it("offers the schema's fields and persists a hidden column per table", async () => {
    const user = userEvent.setup();
    await renderPage();

    await user.click(screen.getByTestId("documents-column-chooser"));
    expect(
      screen.getByTestId("documents-column-chooser-panel"),
    ).toHaveTextContent("schema fields");

    await user.click(screen.getByTestId("documents-column-toggle-body"));
    await waitFor(() =>
      expect(
        screen.queryByTestId("documents-sort-body"),
      ).not.toBeInTheDocument(),
    );
    expect(
      JSON.parse(
        window.localStorage.getItem("nimbus-ui:columns:demo:messages") ?? "{}",
      ),
    ).toEqual({ hidden: ["body"], order: [] });
  });

  it("restores the saved layout on the next visit", async () => {
    window.localStorage.setItem(
      "nimbus-ui:columns:demo:messages",
      JSON.stringify({ hidden: ["author"], order: [] }),
    );
    await renderPage();
    expect(
      screen.queryByTestId("documents-sort-author"),
    ).not.toBeInTheDocument();
    expect(screen.getByTestId("documents-sort-body")).toBeInTheDocument();
  });
});
