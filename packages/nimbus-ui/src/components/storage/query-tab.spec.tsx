import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { HttpResponse, http } from "msw";
import { setupServer } from "msw/node";
import {
  afterAll,
  afterEach,
  beforeAll,
  describe,
  expect,
  it,
  vi,
} from "vitest";

import { documents } from "../../lib/api-mutations";
import { QueryTab } from "./query-tab";
import {
  compileDocumentQuery,
  DOCUMENT_PAGE_SIZE,
  type DocumentFilter,
} from "./table-query";

vi.mock("@nimbus/nimbus/react", () => ({
  useNimbus: () => ({ url: "http://nimbus.example:9000/convex/_nimbus" }),
}));

// Shiki is async and irrelevant here; the text is what the test reads.
vi.mock("../code-block", () => ({
  CodeBlock: ({ code, testid }: { code: string; testid?: string }) => (
    <pre data-testid={testid}>{code}</pre>
  ),
}));

const server = setupServer();

beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

function renderTab(overrides: Partial<Parameters<typeof QueryTab>[0]> = {}) {
  const props = {
    tenant: "demo",
    table: "messages",
    fields: ["_id", "author", "seq", "body"],
    indexBacked: new Set(["_id", "author"]),
    filters: [] as DocumentFilter[],
    order: null,
    onRun: vi.fn(),
    ...overrides,
  };
  render(<QueryTab {...props} />);
  return props;
}

function shownBody(): unknown {
  return JSON.parse(
    screen.getByTestId("documents-query-code-body").textContent ?? "",
  );
}

describe("QueryTab", () => {
  // The acceptance criterion of the tab: what "Show as code" prints is the
  // request the Documents tab sends, not a lookalike. The same compiled
  // query goes through the real client under msw and the two bodies are
  // compared whole.
  it("shows the exact body the document client sends for the same query", async () => {
    const user = userEvent.setup();
    let sent: unknown = null;
    server.use(
      http.post("*/api/tenants/:t/query/paginated", async ({ request }) => {
        sent = await request.json();
        return HttpResponse.json({
          data: [],
          next_cursor: null,
          has_more: false,
        });
      }),
    );
    renderTab();

    await user.click(screen.getByTestId("documents-query-add-row"));
    await user.click(screen.getByTestId("documents-query-field-0"));
    await user.click(screen.getByTestId("documents-query-field-0-option-seq"));
    await user.click(screen.getByTestId("documents-query-op-0"));
    await user.click(screen.getByTestId("documents-query-op-0-option-gte"));
    fireEvent.change(screen.getByTestId("documents-query-value-0"), {
      target: { value: "100" },
    });
    await user.click(screen.getByTestId("documents-query-sort-field"));
    await user.click(
      screen.getByTestId("documents-query-sort-field-option-author"),
    );
    await user.click(screen.getByTestId("documents-query-code-toggle"));

    const filters: DocumentFilter[] = [{ field: "seq", op: "gte", value: 100 }];
    const order = { field: "author", direction: "asc" as const };
    await documents.queryPaginated(
      "demo",
      compileDocumentQuery("messages", filters, order),
      DOCUMENT_PAGE_SIZE,
      null,
    );

    expect(sent).not.toBeNull();
    expect(shownBody()).toEqual(sent);
    expect(shownBody()).toEqual({
      query: { table: "messages", filters, order, limit: null },
      page_size: 200,
      after: null,
    });
    expect(
      screen.getByTestId("documents-query-code-curl").textContent,
    ).toContain("http://nimbus.example:9000/api/tenants/demo/query/paginated");
  });

  it("runs the compiled filters and sort through the URL", async () => {
    const user = userEvent.setup();
    const { onRun } = renderTab();

    await user.click(screen.getByTestId("documents-query-add-row"));
    await user.click(screen.getByTestId("documents-query-field-0"));
    await user.click(
      screen.getByTestId("documents-query-field-0-option-author"),
    );
    fireEvent.change(screen.getByTestId("documents-query-value-0"), {
      target: { value: '"grace"' },
    });
    await user.click(screen.getByTestId("documents-query-run"));

    expect(onRun).toHaveBeenCalledWith(
      [{ field: "author", op: "eq", value: "grace" }],
      null,
    );
  });

  it("seeds its rows from the query the URL already carries", () => {
    renderTab({
      filters: [{ field: "author", op: "neq", value: "ada" }],
      order: { field: "author", direction: "desc" },
    });
    expect(screen.getByTestId("documents-query-row-0")).toBeInTheDocument();
    // A string seeds unquoted, as the chip shows it; it parses back as one.
    expect(screen.getByTestId("documents-query-value-0")).toHaveValue("ada");
    fireEvent.click(screen.getByTestId("documents-query-code-toggle"));
    expect(shownBody()).toMatchObject({
      query: {
        filters: [{ field: "author", op: "neq", value: "ada" }],
        order: { field: "author", direction: "desc" },
      },
    });
  });

  // DESIGN.md:269: an unindexed sort is an unbounded scan and needs an
  // explicit yes. Run stays disabled until the operator gives one.
  it("gates an unindexed sort behind scan anyway", async () => {
    const user = userEvent.setup();
    const { onRun } = renderTab();

    await user.click(screen.getByTestId("documents-query-sort-field"));
    await user.click(
      screen.getByTestId("documents-query-sort-field-option-seq"),
    );
    expect(
      screen.getByTestId("documents-query-scan-warning"),
    ).toHaveTextContent("seq");
    expect(screen.getByTestId("documents-query-run")).toBeDisabled();

    await user.click(screen.getByTestId("documents-query-scan-anyway"));
    expect(screen.getByTestId("documents-query-run")).toBeEnabled();
    await user.click(screen.getByTestId("documents-query-run"));
    expect(onRun).toHaveBeenCalledWith([], {
      field: "seq",
      direction: "asc",
    });

    // Switching to an indexed field removes the gate.
    await user.click(screen.getByTestId("documents-query-sort-field"));
    await user.click(
      screen.getByTestId("documents-query-sort-field-option-author"),
    );
    expect(screen.queryByTestId("documents-query-scan-warning")).toBeNull();
  });

  it("marks each field as indexed or scan in the pickers", async () => {
    const user = userEvent.setup();
    renderTab();
    await user.click(screen.getByTestId("documents-query-add-row"));
    await user.click(screen.getByTestId("documents-query-field-0"));
    expect(
      screen.getByTestId("documents-query-field-0-option-author"),
    ).toHaveTextContent("indexed");
    expect(
      screen.getByTestId("documents-query-field-0-option-body"),
    ).toHaveTextContent("scan");
  });
});
