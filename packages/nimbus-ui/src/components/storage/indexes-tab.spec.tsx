import { fireEvent, render, screen, waitFor } from "@testing-library/react";
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

import type { TableSchemaShape } from "../../lib/types/table";
import { IndexesTab } from "./indexes-tab";

vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));

const server = setupServer();

beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

const SCHEMA: TableSchemaShape = {
  table: "users",
  fields: [{ name: "email", field_type: "string", required: true }],
  indexes: [
    { id: "i1", name: "by_email", fields: ["email"], state: "enabled" },
    {
      id: "i2",
      name: "by_org_role",
      fields: ["org", "role"],
      state: "enabled",
    },
  ],
};

/** The status poll reads the schema route; serve the given schema there. */
function serveSchema(schema: TableSchemaShape | null) {
  server.use(
    http.get("*/api/tenants/:t/schema/:table", () =>
      schema === null
        ? HttpResponse.json({ error: "not found" }, { status: 404 })
        : HttpResponse.json(schema),
    ),
  );
}

function serveApply(report: Record<string, unknown>) {
  const bodies: unknown[] = [];
  server.use(
    http.post("*/api/tenants/:t/schema/:table/apply", async ({ request }) => {
      bodies.push(await request.json());
      return HttpResponse.json(report);
    }),
  );
  return bodies;
}

const APPLIED = {
  applied: true,
  dry_run: false,
  scanned: 4,
  violation_count: 0,
  violations: [],
};

function renderTab(schema: TableSchemaShape | null, onChanged = vi.fn()) {
  render(
    <IndexesTab
      tenant="demo"
      table="users"
      schema={schema}
      onChanged={onChanged}
      pollMs={20}
    />,
  );
  return onChanged;
}

describe("IndexesTab", () => {
  it("says when the schema declares no indexes", () => {
    serveSchema(null);
    renderTab(null);
    expect(screen.getByTestId("documents-indexes-empty")).toHaveTextContent(
      "No indexes defined",
    );
    expect(
      screen.queryByTestId("documents-indexes-table"),
    ).not.toBeInTheDocument();
  });

  it("lists each index with its fields and a status pill", async () => {
    serveSchema(SCHEMA);
    renderTab(SCHEMA);
    const table = screen.getByTestId("documents-indexes-table");
    expect(table).toHaveAttribute("aria-rowcount", "3");
    const rows = screen.getAllByTestId("documents-indexes-table-row");
    expect(rows[0]).toHaveTextContent("by_email");
    expect(rows[0]).toHaveTextContent("email");
    expect(rows[1]).toHaveTextContent("org, role");
    await waitFor(() =>
      expect(
        screen.getByTestId("documents-index-state-by_email"),
      ).toHaveAttribute("data-state", "enabled"),
    );
  });

  // The pill is polled, not copied from the row: a build that finishes
  // after the schema row was read must still turn the pill green.
  it("polls the schema route until every index reads enabled", async () => {
    // The server keeps answering "backfilling" until the test releases it,
    // so the intermediate pill is observable however fast the poll runs.
    let built = false;
    let reads = 0;
    server.use(
      http.get("*/api/tenants/:t/schema/:table", () => {
        const state = built ? "enabled" : "backfilling";
        reads += 1;
        return HttpResponse.json({
          ...SCHEMA,
          indexes: [{ name: "by_email", fields: ["email"], state }],
        });
      }),
    );
    renderTab({
      ...SCHEMA,
      indexes: [{ name: "by_email", fields: ["email"], state: "pending" }],
    });
    // The cell re-renders with each poll, so the pill is read fresh.
    const pill = () => screen.getByTestId("documents-index-state-by_email");
    expect(pill()).toHaveAttribute("data-state", "pending");
    await waitFor(() =>
      expect(pill()).toHaveAttribute("data-state", "backfilling"),
    );
    built = true;
    await waitFor(() =>
      expect(pill()).toHaveAttribute("data-state", "enabled"),
    );
    // Settled: no further reads.
    const settledReads = reads;
    await new Promise((resolve) => setTimeout(resolve, 60));
    expect(reads).toBe(settledReads);
  });

  it("creates an index by re-applying the schema with the index appended", async () => {
    serveSchema(SCHEMA);
    const bodies = serveApply(APPLIED);
    const onChanged = renderTab(SCHEMA);

    fireEvent.click(screen.getByTestId("documents-indexes-add"));
    fireEvent.change(screen.getByTestId("documents-index-name"), {
      target: { value: "by_created" },
    });
    fireEvent.change(screen.getByTestId("documents-index-fields"), {
      target: { value: "createdAt, org" },
    });
    fireEvent.click(screen.getByTestId("documents-index-create"));

    await waitFor(() => expect(onChanged).toHaveBeenCalled());
    expect(bodies).toEqual([
      {
        table: "users",
        fields: [{ name: "email", field_type: "string", required: true }],
        indexes: [
          { name: "by_email", fields: ["email"] },
          { name: "by_org_role", fields: ["org", "role"] },
          { name: "by_created", fields: ["createdAt", "org"] },
        ],
      },
    ]);
    expect(screen.queryByTestId("documents-index-form")).toBeNull();
  });

  it("gives a schemaless table an index-only schema", async () => {
    serveSchema(null);
    const bodies = serveApply(APPLIED);
    const onChanged = renderTab(null);

    fireEvent.click(screen.getByTestId("documents-indexes-add"));
    fireEvent.change(screen.getByTestId("documents-index-name"), {
      target: { value: "by_seq" },
    });
    fireEvent.change(screen.getByTestId("documents-index-fields"), {
      target: { value: "seq" },
    });
    fireEvent.click(screen.getByTestId("documents-index-create"));

    await waitFor(() => expect(onChanged).toHaveBeenCalled());
    expect(bodies).toEqual([
      {
        table: "users",
        fields: [],
        indexes: [{ name: "by_seq", fields: ["seq"] }],
      },
    ]);
  });

  it("refuses a duplicate name or an empty field list before calling the server", () => {
    serveSchema(SCHEMA);
    const bodies = serveApply(APPLIED);
    renderTab(SCHEMA);

    fireEvent.click(screen.getByTestId("documents-indexes-add"));
    fireEvent.change(screen.getByTestId("documents-index-name"), {
      target: { value: "by_email" },
    });
    fireEvent.change(screen.getByTestId("documents-index-fields"), {
      target: { value: "email" },
    });
    fireEvent.click(screen.getByTestId("documents-index-create"));
    expect(screen.getByTestId("documents-index-form-error")).toHaveTextContent(
      "already exists",
    );

    fireEvent.change(screen.getByTestId("documents-index-name"), {
      target: { value: "by_nothing" },
    });
    fireEvent.change(screen.getByTestId("documents-index-fields"), {
      target: { value: " , " },
    });
    fireEvent.click(screen.getByTestId("documents-index-create"));
    expect(screen.getByTestId("documents-index-form-error")).toHaveTextContent(
      "at least one field",
    );
    expect(bodies).toHaveLength(0);
  });

  // An index rides in the schema, so a table that violates its schema
  // cannot take a new index either. The refusal points at the Schema tab,
  // where the violations are listed.
  it("reports a refused apply and keeps the index list", async () => {
    serveSchema(SCHEMA);
    serveApply({
      applied: false,
      dry_run: false,
      scanned: 40,
      violation_count: 3,
      violations: [{ id: "d1", message: "missing required field: email" }],
    });
    const onChanged = renderTab(SCHEMA);

    fireEvent.click(screen.getByTestId("documents-indexes-add"));
    fireEvent.change(screen.getByTestId("documents-index-name"), {
      target: { value: "by_org" },
    });
    fireEvent.change(screen.getByTestId("documents-index-fields"), {
      target: { value: "org" },
    });
    fireEvent.click(screen.getByTestId("documents-index-create"));

    const error = await screen.findByTestId("documents-indexes-error");
    expect(error).toHaveTextContent("3 of 40 documents");
    expect(error).toHaveTextContent("Schema tab");
    expect(onChanged).not.toHaveBeenCalled();
  });

  it("drops an index behind a confirmation by re-applying without it", async () => {
    serveSchema(SCHEMA);
    const bodies = serveApply(APPLIED);
    const onChanged = renderTab(SCHEMA);

    fireEvent.click(screen.getByTestId("documents-index-drop-by_email"));
    expect(
      screen.getByTestId("documents-drop-index-dialog"),
    ).toBeInTheDocument();
    expect(bodies).toHaveLength(0);

    fireEvent.click(screen.getByTestId("documents-drop-index-dialog-confirm"));
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
    expect(bodies).toEqual([
      {
        table: "users",
        fields: [{ name: "email", field_type: "string", required: true }],
        indexes: [{ name: "by_org_role", fields: ["org", "role"] }],
      },
    ]);
  });
});
